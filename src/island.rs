//! One island per output: the hover, drag, pin and tuck state machine, the
//! spring animation, and the frame loop. A single large surface stays put
//! while the pill morphs inside it, because resizing a layer surface takes a
//! configure round trip per frame. The input region tracks the visible shape,
//! so the transparent rest of the surface never eats clicks.
use crate::{
    Model,
    canvas::Canvas,
    scale::Scale,
    modules::{self, MAX_COMPACT_W, MAX_PANEL_H, PANEL_W, PILL_H},
    pen::{Action, Content, Rect, Scroll, View, is_slider},
    icons::Icons,
    settings::Config,
    style::{self, Style},
    viz::{self, Live},
};
use cairo::{Context, Format, ImageSurface};
use smithay_client_toolkit::{
    compositor::{CompositorState, FrameCallbackData, Region},
    reexports::client::{QueueHandle, protocol::{wl_output, wl_surface::WlSurface}},
    shell::{WaylandSurface, wlr_layer::{KeyboardInteractivity, Layer, LayerSurface}},
    shm::Shm,
};
use std::time::{Duration, Instant};

const SHADOW: f64 = 26.0;
/// Opening runs a little past the panel's height; that needs somewhere to go.
const SLACK: f64 = 24.0;
/// A panel stops this short of the screen's edge, and scrolls for the rest.
const EDGE: f64 = 8.0;
const SCROLL_STEP: f64 = 48.0;
const PANEL_R: f64 = 30.0;
const SNAP: f64 = 56.0; // release this close to home and the pill docks
const DRAG_START: f64 = 5.0;
const STALLED: Duration = Duration::from_secs(1);
const FLASH: Duration = Duration::from_millis(1300);
const DOUBLE_CLICK: Duration = Duration::from_millis(280);
const TUCK_W: f64 = 72.0;
const TUCK_H: f64 = 5.0;

/// `period` is the undamped oscillation time, `zeta` the damping ratio: 1 never
/// overshoots, and just under it gives the slight settle Apple's spring has.
#[derive(Clone, Copy)]
pub struct Spring {
    pub x: f64,
    pub v: f64,
    pub target: f64,
    k: f64,
    c: f64,
    eps: f64,
}

impl Spring {
    pub(crate) fn new(x: f64, period: f64, zeta: f64, eps: f64) -> Self {
        let w = std::f64::consts::TAU / period;
        Spring { x, v: 0.0, target: x, k: w * w, c: 2.0 * zeta * w, eps }
    }

    pub(crate) fn tune(&mut self, period: f64, zeta: f64) {
        let w = std::f64::consts::TAU / period;
        self.k = w * w;
        self.c = 2.0 * zeta * w;
    }

    pub(crate) fn step(&mut self, dt: f64) {
        // Fixed substeps keep the integrator stable at any refresh rate.
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(0.001);
            self.v += (-self.k * (self.x - self.target) - self.c * self.v) * h;
            self.x += self.v * h;
            left -= h;
        }
        if self.settled() {
            self.x = self.target;
            self.v = 0.0;
        }
    }

    pub(crate) fn settled(&self) -> bool {
        (self.x - self.target).abs() < self.eps && self.v.abs() < self.eps * 10.0
    }

    pub(crate) fn snap(&mut self, x: f64) {
        self.x = x;
        self.target = x;
        self.v = 0.0;
    }
}

pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub(crate) fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A rounded rect whose corners ease into the straights instead of meeting them
/// abruptly. `s` is 0 for a true pill, 1 for full continuous corners.
fn squircle(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64, s: f64) {
    let lead = (r * (1.0 + 0.3 * s)).min(w / 2.0).min(h / 2.0);
    let c = lead * lerp(0.4477, 0.26, s);
    let (rx, by) = (x + w, y + h);
    cr.new_path();
    cr.move_to(x + lead, y);
    cr.line_to(rx - lead, y);
    cr.curve_to(rx - c, y, rx, y + c, rx, y + lead);
    cr.line_to(rx, by - lead);
    cr.curve_to(rx, by - c, rx - c, by, rx - lead, by);
    cr.line_to(x + lead, by);
    cr.curve_to(x + c, by, x, by - c, x, by - lead);
    cr.line_to(x, y + lead);
    cr.curve_to(x, y + c, x + c, y, x + lead, y);
    cr.close_path();
}

struct Press {
    button: u32,
    at: (f64, f64),
    region: Option<crate::pen::Region>,
}

/// What a pointer event means to the app, once the island has digested it.
pub enum Outcome {
    Act(Action, Rect01),
    Moved,
    None,
}

/// Slider position under the pointer, 0..=1, for Action::Volume.
pub type Rect01 = f64;

pub struct Island {
    pub output: wl_output::WlOutput,
    pub name: String,
    pub out: (f64, f64),
    pub scale: Scale,
    pub layer: LayerSurface,
    configured: bool,
    canvas: Canvas,
    chrome: Option<Chrome>,
    style: Style,
    /// The frame's palette grows out of the bottom border, and goes back into it
    /// before the island is home at the top again.
    flipped: bool,
    /// The split style's bubble: how far it has parted, and how wide it last was.
    bud: Spring,
    bud_w: f64,
    size: (f64, f64),
    /// How solid the background is, and whether the compositor is to blur behind it.
    alpha: f64,
    blurred: bool,

    /// Pill centre x and top y. None is docked at top centre.
    pub pos: Option<(f64, f64)>,
    place: (Spring, Spring),
    placed: Option<(i32, i32)>,
    input: Option<(i32, i32, i32, i32)>,

    t: Spring,
    cw: Spring,
    ph: Spring,
    pw: Spring,
    k: Spring,
    last: Option<Instant>,
    /// ISLAND_TIME_SCALE=0.1 runs animations at a tenth speed, for tuning them.
    time_scale: f64,

    pub pinned: bool,
    pub tucked: bool,
    toast: bool,
    /// An inline reply is open here: it holds the keyboard, and the card with it.
    typing: bool,
    raised: bool,
    hovered: bool,
    open: bool,
    suppress: bool,
    press: Option<Press>,
    dragging: bool,
    sliding: bool,
    pointer: (f64, f64),
    hover: Option<usize>,
    pin_at: Option<Instant>,

    open_at: Option<Instant>,
    close_at: Option<Instant>,
    flash_until: Option<Instant>,

    pub view: View,
    compact: Option<Content>,
    panel: Option<Content>,
    /// How far down the panel is scrolled, the height of the whole of it, and which view that's for.
    scroll: f64,
    total: f64,
    scrolled_view: View,
    compact_dirty: bool,
    panel_dirty: bool,

    frame_pending: bool,
    drawn_at: Option<Instant>,
    dirty: bool,
    /// Only the levels moved.
    live_dirty: bool,
}

/// The resting pill's shadow and background. The shadow strokes are the costliest
/// part of a frame, and content-only redraws like a workspace switch keep them.
/// ponytail: pill only; a pinned panel's chrome would hold megabytes for less gain.
struct Chrome {
    key: [f64; 11],
    surf: ImageSurface,
    at: (f64, f64),
}

#[derive(Clone, Copy)]
struct Shape {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    cx: f64,
}

impl Island {
    #[allow(clippy::too_many_arguments)]
    pub fn new(output: wl_output::WlOutput, name: String, out: (f64, f64), scale: Scale, layer: LayerSurface,
               shm: &Shm, style: Style, state_pos: Option<(f64, f64)>, pinned: bool, tucked: bool) -> Option<Self> {
        let size = surface_size(out, style);
        let canvas = Canvas::new(shm, size, scale)?;
        // An attached style keeps where the pill was for when it's a pill again.
        let pos = state_pos.map(|p| clamp_pos(p, out, 200.0));
        let at = pos.filter(|_| !style.attached()).unwrap_or(home(out, style));
        // The frame starts drawn in, until there's something to show.
        let retracted = tucked || style == Style::Frame;
        Some(Island {
            output, name, out, scale, layer, configured: false, canvas, chrome: None, size, pos,
            style, alpha: 1.0, blurred: false, flipped: false, bud: Spring::new(0.0, 0.44, 0.8, 0.002), bud_w: PILL_H,
            place: (Spring::new(at.0, 0.34, 0.82, 0.3), Spring::new(at.1, 0.34, 0.82, 0.3)),
            placed: None, input: None,
            t: Spring::new(if pinned { 1.0 } else { 0.0 }, 0.42, 0.78, 0.002),
            cw: Spring::new(200.0, 0.36, 0.86, 0.3),
            ph: Spring::new(320.0, 0.36, 0.86, 0.3),
            pw: Spring::new(PANEL_W, 0.36, 0.86, 0.3),
            k: Spring::new(if retracted { 1.0 } else { 0.0 }, 0.34, 0.86, 0.002),
            last: None, time_scale: std::env::var("ISLAND_TIME_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            pinned, tucked, toast: false, typing: false, raised: false, hovered: false, open: false, suppress: false, press: None,
            dragging: false, sliding: false, pointer: (0.0, 0.0), hover: None, pin_at: None,
            open_at: None, close_at: None, flash_until: None, view: View::Main,
            compact: None, panel: None, scroll: 0.0, total: 0.0, scrolled_view: View::Main,
            compact_dirty: true, panel_dirty: true,
            frame_pending: false, drawn_at: None, dirty: true, live_dirty: false,
        })
    }

    pub fn surface_size(&self) -> (i32, i32) {
        self.scale.size(self.size)
    }

    pub fn surface(&self) -> &WlSurface {
        self.layer.wl_surface()
    }

    pub fn configured(&mut self) {
        self.configured = true;
        self.dirty = true;
    }

    pub fn wants_panel(&self) -> bool {
        (self.pinned || self.open || self.toast || self.typing || self.view == View::Palette) && !self.dragging
    }

    /// Stats only matter while the panel proper is up, not a toast or the palette.
    pub fn wants_stats(&self) -> bool {
        (self.pinned || self.open) && matches!(self.view, View::Main | View::Menu { .. })
    }

    pub fn hovered(&self) -> bool {
        self.hovered
    }

    /// The overlay layer is what lets these show above a fullscreen window, and
    /// keyboard focus is held only while the palette is open.
    fn raise(&mut self, overlay: bool, keyboard: bool) {
        self.layer.set_layer(if overlay { Layer::Overlay } else { Layer::Top });
        self.layer.set_keyboard_interactivity(if keyboard { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
        self.layer.commit();
        self.raised = overlay;
    }

    pub fn open_palette(&mut self) {
        if self.style == Style::Frame {
            self.flipped = true;
            self.t.snap(0.0);
            self.panel = None;
            self.place.1.snap(self.out.1 - style::BORDER - PILL_H);
        }
        self.view = View::Palette;
        self.toast = false;
        self.raise(true, true);
        self.panel_dirty = true;
        self.dirty = true;
    }

    pub fn close_palette(&mut self) {
        self.raise(false, false);
        // Toast with no toast is the closing state: the palette's last frame stays
        // up while the shape shrinks, and rebuild() settles it on Main.
        self.view = if (self.pinned || self.open) && !self.flipped { View::Main } else { View::Toast };
        self.panel_dirty = true;
        self.dirty = true;
    }

    /// Show the note the app put in `Center::showing`. Critical ones come above
    /// fullscreen; everything else waits politely under your game.
    pub fn show_toast(&mut self, critical: bool) {
        self.view = View::Toast;
        self.toast = true;
        if critical {
            self.raise(true, false);
        }
        self.panel_dirty = true;
        self.dirty = true;
    }

    /// Keyboard focus for an inline reply, the same way the palette takes it. Letting
    /// go keeps a critical toast above fullscreen; end_toast lowers it after.
    pub fn set_typing(&mut self, on: bool) {
        if self.typing != on {
            self.typing = on;
            self.raise(on || (self.raised && self.toast), on);
            self.panel_dirty = true;
            self.dirty = true;
        }
    }

    pub fn end_toast(&mut self) {
        if !self.toast {
            return;
        }
        self.toast = false;
        if self.raised {
            self.raise(false, false);
        }
        self.dirty = true;
    }

    /// Stats and panel contents are only worth producing while one is visible.
    pub fn panel_visible(&self) -> bool {
        self.wants_panel() || self.t.x > 0.001
    }

    /// On its way back into the bottom border after the palette closed.
    fn returning(&self) -> bool {
        self.flipped && self.view != View::Palette
    }

    fn panel_cx(&self, x: f64, w: f64) -> f64 {
        x.clamp(w / 2.0 + 8.0, (self.out.0 - w / 2.0 - 8.0).max(w / 2.0 + 8.0))
    }

    pub fn invalidate(&mut self, compact: bool, panel: bool) {
        self.compact_dirty |= compact;
        self.panel_dirty |= panel;
        self.dirty |= (compact && self.t.x < 0.999) || (panel && self.panel_visible());
    }

    /// Tips are stale, but nothing on screen is: refresh on the next draw.
    pub fn titles_changed(&mut self) {
        self.compact_dirty = true;
        self.panel_dirty = true;
    }

    /// Hold the panel for a tray menu asked for from the rail or the bar.
    pub fn open_briefly(&mut self) {
        self.open = true;
        self.close_at = (!self.hovered).then(|| Instant::now() + Duration::from_secs(5));
        self.dirty = true;
    }

    /// What's on screen with visualizers in it, if anything.
    fn live_content(&self) -> Option<&Content> {
        self.active().map(|(c, ..)| c).filter(|c| !c.live.is_empty())
    }

    /// Frames stop when it's covered or the screen's off: then nothing is listening.
    pub fn wants_levels(&self) -> bool {
        let seen = !self.frame_pending || self.drawn_at.is_none_or(|t| t.elapsed() < STALLED);
        seen && self.live_content().is_some()
    }

    pub fn levels_changed(&mut self) {
        self.live_dirty |= self.live_content().is_some();
    }

    pub fn holds_panel(&self) -> bool {
        self.panel.is_some()
    }

    pub fn flash(&mut self) {
        if !self.wants_panel() {
            self.flash_until = Some(Instant::now() + FLASH);
            self.invalidate(true, false);
        }
    }

    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
        self.panel_dirty = true;
        self.dirty = true;
    }

    pub fn set_tucked(&mut self, tucked: bool) {
        self.tucked = tucked;
        self.panel_dirty = true;
        self.dirty = true;
    }

    pub fn set_output(&mut self, out: (f64, f64), scale: Scale) {
        if scale != self.scale {
            self.scale = scale;
            self.chrome = None;
            self.compact_dirty = true;
            self.panel_dirty = true;
        }
        let size = surface_size(out, self.style);
        if size != self.size {
            self.size = size;
            let (w, h) = scale.size(size);
            self.layer.set_size(w as u32, h as u32);
        }
        self.canvas.resize(size, scale);
        self.out = out;
        self.pos = self.pos.map(|p| clamp_pos(p, out, self.cw.target));
        let mut at = self.pos.filter(|_| !self.style.attached()).unwrap_or(self.home());
        if self.flipped {
            at.1 = out.1 - style::BORDER - PILL_H;
        }
        self.place.0.snap(at.0);
        self.place.1.snap(at.1);
        self.placed = None;
        self.dirty = true;
    }

    fn home(&self) -> (f64, f64) {
        home(self.out, self.style)
    }

    fn body_alpha(&self, k: f64) -> f64 {
        let a = lerp(0.97, 0.72, k);
        if self.alpha < 1.0 { self.alpha * a / 0.97 } else { a }
    }

    fn opens_down(&self) -> bool {
        if self.style.attached() { !self.flipped } else { self.place.1.x + PILL_H / 2.0 < self.out.1 / 2.0 }
    }

    /// Surface top-left on the output, keeping the whole panel on screen.
    fn surface_pos(&self) -> (f64, f64) {
        let (px, py) = (self.place.0.x, self.place.1.x);
        let x = (px - self.size.0 / 2.0).clamp(0.0, (self.out.0 - self.size.0).max(0.0));
        let y = if self.opens_down() { (py - SHADOW).max(0.0) } else { py + PILL_H + SHADOW - self.size.1 };
        (x.round(), y.clamp(0.0, (self.out.1 - self.size.1).max(0.0)).round())
    }

    /// The tallest the panel can be from where the island sits.
    fn room(&self) -> f64 {
        let py = self.place.1.target;
        let space = if self.opens_down() { self.out.1 - py - EDGE } else { py + PILL_H - EDGE };
        space.clamp(PILL_H, MAX_PANEL_H).floor()
    }

    fn overflows(&self) -> bool {
        self.panel.as_ref().is_some_and(|c| self.total > c.h)
    }

    pub fn scroll_by(&mut self, notches: f64) {
        let Some(shown) = self.panel.as_ref().map(|c| c.h) else { return };
        let to = (self.scroll + notches * SCROLL_STEP).clamp(0.0, (self.total - shown).max(0.0));
        if to != self.scroll {
            self.scroll = to;
            self.panel_dirty = true;
            self.dirty = true;
        }
    }

    fn shape(&self) -> Shape {
        let (sx, sy) = self.surface_pos();
        let (px, py) = (self.place.0.x - sx, self.place.1.x - sy);
        let t = self.t.x;
        let k = self.k.x;
        let attached = self.style.attached();
        // The panel slides sideways as it opens if the pill sits near an edge.
        let panel_cx = self.panel_cx(self.place.0.x, self.pw.x) - sx;
        let cx = lerp(px, panel_cx, t.clamp(0.0, 1.0));
        // Closed, an attached island draws back into its edge rather than into a sliver.
        let (rest_w, rest_h) = match () {
            _ if self.flipped => (self.pw.x * 0.6, 0.0),
            _ if attached => (lerp(self.cw.x, self.cw.x * 0.6, k), lerp(PILL_H, 0.0, k)),
            _ => (lerp(self.cw.x, TUCK_W, k), lerp(PILL_H, TUCK_H, k)),
        };
        let w = lerp(rest_w, self.pw.x, t);
        let h = lerp(rest_h, self.ph.x, t).max(0.0);
        let top = if attached { py } else { lerp(py, if self.pos.is_none() { 2.0 } else { py }, k) };
        // The spring's overshoot never runs off the surface.
        let down = self.opens_down();
        let h = h.min(if down { self.size.1 - top } else { py + PILL_H }).max(0.0);
        let y = if down { top } else { py + PILL_H - h };
        let r = lerp(PILL_H / 2.0, PANEL_R, t.clamp(0.0, 1.0));
        let r = if attached { r } else { r.min(h / 2.0) };
        Shape { x: cx - w / 2.0, y, w, h, r, cx }
    }

    /// Where the input region should sit at rest for the current target.
    fn target_rect(&self) -> Rect {
        let (sx, sy) = self.surface_pos();
        let (px, py) = (self.place.0.target - sx, self.place.1.target - sy);
        let attached = self.style.attached();
        if self.returning() {
            (0.0, 0.0, 0.0, 0.0)
        } else if self.t.target > 0.5 {
            let w = self.pw.target;
            let cx = self.panel_cx(self.place.0.target, w) - sx;
            let down = self.opens_down();
            let y = if down { py } else { py + PILL_H - self.ph.target };
            // An attached panel takes in its band, or crossing it would close the panel.
            match (attached, down) {
                (true, true) => (cx - w / 2.0, 0.0, w, y + self.ph.target),
                (true, false) => (cx - w / 2.0, y, w, self.size.1 - y),
                _ => (cx - w / 2.0, y, w, self.ph.target),
            }
        } else if self.k.target > 0.5 {
            if attached {
                // All that's left is the band it drew back into: that's the way in.
                let w = self.cw.target.max(240.0);
                return (px - w / 2.0, 0.0, w, self.style.insets().top + 4.0);
            }
            // A thin sliver is a hard target: keep a generous hot zone around it.
            let y = if self.pos.is_none() { 0.0 } else { py };
            (px - 70.0, y, 140.0, 12.0)
        } else if attached {
            (px - self.cw.target / 2.0, 0.0, self.cw.target, py + PILL_H)
        } else {
            (px - self.cw.target / 2.0, py, self.cw.target, PILL_H)
        }
    }

    fn apply_input(&mut self, compositor: &CompositorState) {
        let r = self.target_rect();
        let u = self.scale.ui;
        let rect = ((r.0 * u).floor() as i32, (r.1 * u).floor() as i32, (r.2 * u).ceil() as i32, (r.3 * u).ceil() as i32);
        if self.input == Some(rect) {
            return;
        }
        if let Ok(region) = Region::new(compositor) {
            region.add(rect.0, rect.1, rect.2, rect.3);
            self.surface().set_input_region(Some(region.wl_region()));
            self.input = Some(rect);
        }
    }

    /// The island floats over windows and never reserves space, so one anchor
    /// and two margins cover every position.
    fn apply_place(&mut self) {
        let (x, y) = self.surface_pos();
        let want = (self.scale.wl(x), self.scale.wl(y));
        if self.placed != Some(want) {
            self.placed = Some(want);
            self.layer.set_margin(want.1, 0, 0, want.0);
        }
    }

    fn update_targets(&mut self) {
        let opening = self.wants_panel() && !self.returning();
        if opening && self.t.target < 0.5 {
            self.t.tune(0.42, 0.74);
        } else if !opening && self.t.target > 0.5 {
            self.t.tune(0.34, 0.92);
        }
        self.t.target = if opening { 1.0 } else { 0.0 };
        // Content with no height is nothing to show: back into the edge.
        let empty = self.compact.as_ref().is_some_and(|c| c.h == 0.0);
        self.k.target = if (self.tucked || empty) && !opening && !self.dragging { 1.0 } else { 0.0 };
        let parted = !opening && self.compact.as_ref().is_some_and(|c| c.split.is_some());
        self.bud.target = if parted { 1.0 } else { 0.0 };
    }

    fn springs_settled(&self) -> bool {
        [&self.t, &self.cw, &self.ph, &self.pw, &self.k, &self.bud, &self.place.0, &self.place.1].iter().all(|s| s.settled())
    }

    fn rebuild(&mut self, m: &Model, icons: &mut Icons, cfg: &Config) {
        if self.compact_dirty {
            self.compact_dirty = false;
            let flash = self.flash_until.is_some();
            if let Some(c) = modules::pill(m, icons, cfg, &self.name, self.scale, flash) {
                if self.compact.is_none() {
                    self.cw.snap(c.w);
                }
                self.cw.target = c.w;
                if let Some(at) = c.split {
                    self.bud_w = c.w - at;
                }
                self.compact = Some(c);
            }
        }
        // A hover that outlives a toast carries on as the normal panel.
        if self.view == View::Toast && !self.toast && (self.pinned || self.open) && !self.returning() {
            self.view = View::Main;
            self.panel_dirty = true;
        }
        if self.panel_visible() {
            // While closing, keep showing what was open rather than swapping it out.
            if (self.panel_dirty && self.wants_panel() && !self.returning()) || self.panel.is_none() {
                self.panel_dirty = false;
                let first = self.panel.is_none();
                if first || self.scrolled_view != self.view {
                    self.scroll = 0.0;
                    self.scrolled_view = self.view.clone();
                }
                let room = self.room();
                let draw = |icons: &mut Icons, at: f64| {
                    modules::panel(m, icons, cfg, &self.name, &self.view, (room, at), self.scale, self.pinned, self.tucked)
                };
                let mut drawn = draw(icons, self.scroll);
                // What's shown got shorter under the scroll: come back up to its end.
                let most = drawn.as_ref().map_or(0.0, |(c, total)| (total - c.h).max(0.0));
                if self.scroll > most {
                    drawn = draw(icons, most);
                    self.scroll = most;
                }
                if let Some((c, total)) = drawn {
                    self.total = total;
                    if first && self.t.x < 0.01 {
                        self.ph.snap(c.h);
                        self.pw.snap(c.w);
                    }
                    self.ph.target = c.h;
                    self.pw.target = c.w;
                    self.panel = Some(c);
                }
            }
        } else if self.panel.take().is_some() {
            // Nothing shows it until the next hover; give the memory back.
            self.view = View::Main;
            self.panel_dirty = true;
        }
    }

    /// Draw now if the compositor is ready for a frame, otherwise on its next callback.
    pub fn flush(&mut self, m: &Model, icons: &mut Icons, cfg: &Config, compositor: &CompositorState, qh: &QueueHandle<crate::App>) {
        if self.configured && (self.dirty || self.live_dirty) && !self.frame_pending {
            self.render(m, icons, cfg, compositor, qh);
        }
    }

    pub fn frame_done(&mut self) {
        self.frame_pending = false;
    }

    fn render(&mut self, m: &Model, icons: &mut Icons, cfg: &Config, compositor: &CompositorState, qh: &QueueHandle<crate::App>) {
        let only_live = !self.dirty;
        self.live_dirty = false;
        self.rebuild(m, icons, cfg);
        self.update_targets();

        let now = Instant::now();
        // The first frame after idle has no meaningful gap to integrate over.
        let dt = self.last.map_or(1.0 / 240.0, |l| (now - l).as_secs_f64()).min(1.0 / 30.0) * self.time_scale;
        for s in [&mut self.t, &mut self.cw, &mut self.ph, &mut self.pw, &mut self.k, &mut self.bud, &mut self.place.0, &mut self.place.1] {
            s.step(dt);
        }
        let mut animating = !self.springs_settled();
        if self.returning() && self.t.settled() && self.t.target == 0.0 {
            // All the way into the bottom border: carry on from the top one.
            self.flipped = false;
            self.panel = None;
            self.panel_dirty = true;
            let home = self.home();
            self.place.1.snap(home.1);
            animating = true;
        }
        self.last = animating.then_some(now);

        self.apply_place();
        self.apply_input(compositor);
        // Regions change with the state and the content; indices don't carry over.
        if self.hovered && !self.dragging {
            self.hover = self.hit(self.pointer);
        }

        let shape = self.shape();
        let bounds = if self.style.bar_like() {
            // The band's middle is always the island's, all the way across.
            (0.0, 0.0, self.size.0, (shape.y + shape.h + SHADOW).max(self.style.insets().top))
        } else {
            (shape.x - SHADOW, shape.y - SHADOW, shape.w + 2.0 * SHADOW, shape.h + 2.0 * SHADOW)
        };
        self.alpha = cfg.alpha(self.style);
        self.blurred = cfg.blurred(self.style);
        // A bar's chrome is a flat fill wider than the cache would be.
        let resting = !animating && self.t.x == 0.0 && !self.style.bar_like();
        if resting {
            self.cache_chrome(shape, &m.pal);
        } else {
            self.chrome = None;
        }
        let area = if only_live && !animating {
            match self.active().and_then(|(c, x, y)| live_bounds(&c.live, (x, y))) {
                Some(area) => Some(area),
                None => return,
            }
        } else {
            None
        };
        let frame = match area {
            Some(area) => self.canvas.begin_live(bounds, area),
            None => self.canvas.begin(bounds),
        };
        let Some(frame) = frame else { return };
        self.paint(&frame.cr, frame.patch, shape, &m.pal, animating, cfg, &m.levels);
        self.drawn_at = Some(now);

        let surface = self.layer.wl_surface();
        self.canvas.present(frame, surface);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.layer.commit();
        self.frame_pending = true;
        self.dirty = animating;
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(&self, cr: &Context, patch: Option<Rect>, sh: Shape, pal: &crate::theme::Palette, animating: bool, cfg: &Config, levels: &viz::Levels) {
        let t = self.t.x.clamp(0.0, 1.0);
        let k = self.k.x.clamp(0.0, 1.0);
        match (&self.chrome, patch) {
            (Some(c), _) => {
                if cr.set_source_surface(&c.surf, c.at.0, c.at.1).is_ok() {
                    let _ = cr.paint();
                }
            }
            // A visualizer's patch is well inside a still shape, where chrome is a flat fill.
            (None, Some((x, y, w, h))) => {
                let (r, g, b) = pal.bg;
                let solid = self.style.attached() || self.parted(sh, t).is_some();
                cr.set_source_rgba(r, g, b, if solid { self.alpha } else { self.body_alpha(k) });
                cr.rectangle(x, y, w, h);
                let _ = cr.fill();
            }
            (None, None) => self.paint_chrome(cr, sh, pal),
        }

        let a_compact = if self.flipped { 0.0 } else { (1.0 - smooth(t / 0.45)) * (1.0 - k) };
        let a_panel = smooth((t - 0.32) / 0.68);
        let morphing = animating && (t > 0.0 && t < 1.0 || k > 0.0 && k < 1.0);
        // What scrolls runs under the rounded corners.
        let clipped = morphing || (a_panel > 0.003 && self.overflows());
        if clipped {
            let _ = cr.save();
            self.outline(cr, sh, t);
            cr.clip();
        }
        if let (true, Some(c)) = (a_compact > 0.003, &self.compact) {
            let (at_x, y) = self.compact_at(sh, c);
            match (c.split, self.parted(sh, t)) {
                // While they part, each half rides its own side of the shape.
                (Some(at), Some((main_w, bubble))) => {
                    blit(cr, c, sh.x, y, main_w.min(at - style::SPLIT_GAP), PILL_H, a_compact);
                    for l in &c.live {
                        let x = if l.rect.0 < at { sh.x } else { sh.x + sh.w - c.w };
                        viz::paint(cr, l, (x, y), levels, pal.accent, a_compact * smooth(self.bud.x));
                    }
                    let _ = cr.save();
                    cr.rectangle(bubble, y, sh.x + sh.w - bubble, PILL_H);
                    cr.clip();
                    blit(cr, c, sh.x + sh.w - c.w, y, c.w, PILL_H, a_compact * smooth(self.bud.x));
                    let _ = cr.restore();
                }
                _ => {
                    let x = at_x;
                    blit(cr, c, x, y, sh.w.min(c.w), PILL_H, a_compact);
                    for l in &c.live {
                        // Not until the shape has grown out to it.
                        if l.rect.0 + l.rect.2 <= sh.w + 1.0 {
                            viz::paint(cr, l, (x, y), levels, pal.accent, a_compact);
                        }
                    }
                }
            }
        }
        if let (true, Some(c)) = (a_panel > 0.003, &self.panel) {
            let (x, y) = self.panel_at(sh, c);
            let visualize = |under: bool| {
                for l in c.live.iter().filter(|l| l.look.under() == under) {
                    if l.rect.1 + l.rect.3 <= sh.h + 8.0 {
                        viz::paint(cr, l, (x, y), levels, pal.accent, a_panel);
                    }
                }
            };
            visualize(true);
            blit(cr, c, x, y, c.w, c.h.min(sh.h + 8.0), a_panel);
            visualize(false);
        }
        if clipped {
            let _ = cr.restore();
        }

        // Hover feedback sits over the cached content, so moving the pointer
        // never re-lays anything out.
        if let (false, Some(i)) = (self.dragging, self.hover) {
            if let Some((c, ox, oy)) = self.active() {
                if let Some(reg) = c.regions.get(i) {
                    let (x, y, w, h) = reg.rect;
                    if !reg.inert() {
                        let _ = cr.save();
                        self.outline(cr, sh, t);
                        cr.clip();
                        // A whole card lights as it's shaped, tab and all.
                        if (x, y, w, h) == (0.0, 0.0, c.w, c.h) {
                            self.outline(cr, sh, t);
                        } else {
                            squircle(cr, ox + x, oy + y, w, h, reg.radius, 0.0);
                        }
                        let (fr, fg, fb) = pal.fg;
                        cr.set_source_rgba(fr, fg, fb, 0.1);
                        let _ = cr.fill();
                        let _ = cr.restore();
                    }
                    if let (Some(tip), true) = (&reg.tip, self.t.target > 0.5 && !tip_is_empty(reg.tip.as_deref())) {
                        if matches!(reg.left, Action::PaletteTool(_)) {
                            // The tool tabs live in the search row, so their tip goes beside them, not over the results.
                            let left = c.regions.iter().filter(|r| matches!(r.left, Action::PaletteTool(_)))
                                .map(|r| r.rect.0).fold(f64::MAX, f64::min);
                            self.paint_row_tip(cr, tip, (ox + left - 8.0, oy + y + h / 2.0), pal, cfg);
                        } else {
                            self.paint_tip(cr, tip, (ox + x, oy + y, w, h), sh, pal, cfg);
                        }
                    }
                }
            }
        }
    }

    fn cache_chrome(&mut self, sh: Shape, pal: &crate::theme::Palette) {
        let key = [sh.x, sh.y, sh.w, sh.h, self.k.x, self.bud.x, pal.bg.0, pal.bg.1, pal.bg.2, self.alpha, f64::from(self.blurred)];
        if self.chrome.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        self.chrome = None;
        // Pixel-aligned, so the cached copy lands exactly where a direct draw would.
        let s = self.scale.px();
        let (x, y) = (((sh.x - SHADOW) * s).floor() / s, ((sh.y - SHADOW) * s).floor() / s);
        let w = ((sh.x + sh.w + SHADOW - x) * s).ceil() as i32;
        let h = ((sh.y + sh.h + SHADOW - y) * s).ceil() as i32;
        let Ok(surf) = ImageSurface::create(Format::ARgb32, w, h) else { return };
        surf.set_device_scale(s, s);
        let Ok(cr) = Context::new(&surf) else { return };
        cr.translate(-x, -y);
        self.paint_chrome(&cr, sh, pal);
        drop(cr);
        self.chrome = Some(Chrome { key, surf, at: (x, y) });
    }

    fn outline(&self, cr: &Context, sh: Shape, t: f64) {
        if !self.style.attached() {
            return squircle(cr, sh.x, sh.y, sh.w, sh.h, sh.r, t);
        }
        let (band, end) = if self.flipped {
            (style::BORDER, self.size.1 - sh.y)
        } else {
            (self.style.insets().top, sh.y + sh.h)
        };
        // Glass floats, so its band starts below the screen's edge.
        let top = if self.style == Style::Glass { style::FLOAT } else { 0.0 };
        let _ = cr.save();
        if self.flipped {
            cr.translate(0.0, self.size.1);
            cr.scale(1.0, -1.0);
        }
        cr.translate(0.0, top);
        style::tab(cr, (sh.x, sh.x + sh.w), band - top, end - top, style::FILLET, sh.r);
        let _ = cr.restore();
    }

    /// While the split style has two bodies: the pill's width, and where the bubble starts.
    fn parted(&self, sh: Shape, t: f64) -> Option<(f64, f64)> {
        let b = self.bud.x * (1.0 - smooth(t / 0.35)) * (1.0 - self.k.x.clamp(0.0, 1.0));
        if b < 0.002 {
            return None;
        }
        let bubble = (sh.x + sh.w - self.bud_w.max(sh.h)).max(sh.x + sh.h);
        let main = lerp(sh.w, bubble - style::SPLIT_GAP - sh.x, b).max(sh.h);
        Some((main, bubble))
    }

    fn paint_chrome(&self, cr: &Context, sh: Shape, pal: &crate::theme::Palette) {
        let t = self.t.x.clamp(0.0, 1.0);
        let k = self.k.x.clamp(0.0, 1.0);
        let (r, g, b) = pal.bg;
        if self.style.attached() {
            // Flat, exactly like the band it's joined to: that's what hides the seam.
            let _ = cr.save();
            let border = self.style.insets().top;
            if self.alpha < 1.0 && self.style == Style::Frame {
                // Two see-through layers would show the border it grows out of as a darker band.
                let (y, h) = if self.flipped { (0.0, self.size.1 - border) } else { (border, self.size.1 - border) };
                cr.rectangle(0.0, y, self.size.0, h);
                cr.clip();
            }
            self.outline(cr, sh, t);
            if self.style.bar_like() {
                // The strip leaves this stretch of the band to the island: one fill, one layer.
                let sx = self.surface_pos().0;
                let (x, y, w, h) = self.style.band(self.out.0);
                let (x0, x1) = ((x - sx).max(0.0), (x + w - sx).min(self.size.0));
                cr.rectangle(x0, y, x1 - x0, h);
            }
            cr.set_source_rgba(r, g, b, self.alpha);
            let _ = cr.fill();
            let _ = cr.restore();
            if self.style == Style::Glass {
                // The capsule the workspaces sit in, until it opens.
                let a = 0.08 * (1.0 - smooth(t / 0.45)) * (1.0 - k);
                if a > 0.001 {
                    crate::pen::rounded_rect(cr, sh.x, sh.y + 3.0, sh.w, PILL_H - 6.0, (PILL_H - 6.0) / 2.0);
                    let (fr, fg, fb) = pal.fg;
                    cr.set_source_rgba(fr, fg, fb, a);
                    let _ = cr.fill();
                }
            }
            return;
        }
        let parted = self.parted(sh, t);
        let see_through = self.alpha < 1.0 || self.blurred;
        let bodies = match parted {
            Some((main, bubble)) => [Some((sh.x, main)), Some((bubble, sh.x + sh.w - bubble))],
            None => [Some((sh.x, sh.w)), None],
        };
        // Soft shadow as nested strokes: only the rim is touched, so a big panel
        // costs what a small one does, and no blur pass is needed.
        // None under a see-through one: it would show through, and be blurred into a halo.
        let lift = if see_through { 0.0 } else { 1.0 - k };
        if lift > 0.0 {
            let depth = lerp(12.0, 24.0, t);
            for i in (1..=6).rev() {
                let f = i as f64 / 6.0;
                for &(x, w) in bodies.iter().flatten() {
                    squircle(cr, x, sh.y + 2.0, w, sh.h, sh.r, t);
                    cr.set_line_width(2.0 * depth * f);
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.055 * lift * (1.0 - f * 0.7));
                    let _ = cr.stroke();
                }
            }
        }
        // Two bodies and the drop between them overlap, so those have to be solid.
        // Two bodies and the drop between them overlap, so see-through ones are drawn solid, then thinned as one.
        let group = see_through && parted.is_some();
        if group {
            cr.push_group();
        }
        cr.set_source_rgba(r, g, b, if parted.is_some() { 1.0 } else { self.body_alpha(k) });
        for &(x, w) in bodies.iter().flatten() {
            squircle(cr, x, sh.y, w, sh.h, sh.r, t);
            let _ = cr.fill();
        }
        if let Some((main, bubble)) = parted {
            if style::neck(cr, (sh.x, sh.y, main, sh.h), bubble) {
                let _ = cr.fill();
            }
        }
        if group && cr.pop_group_to_source().is_ok() {
            let _ = cr.paint_with_alpha(self.alpha);
        }
    }

    /// What the pointer is on, said beside it: under the target, or over it where
    /// there's no room. A wide row's tip keeps to its right, clear of the next row's text.
    fn paint_tip(&self, cr: &Context, tip: &str, of: Rect, sh: Shape, pal: &crate::theme::Palette, cfg: &Config) {
        const MOST: f64 = 300.0;
        let layout = pangocairo::functions::create_layout(cr);
        let mut d = pango::FontDescription::new();
        d.set_family(&cfg.font);
        d.set_weight(pango::Weight::Medium);
        d.set_absolute_size(11.0 * pango::SCALE as f64);
        layout.set_font_description(Some(&d));
        layout.set_text(tip);
        if f64::from(layout.pixel_size().0) > MOST {
            layout.set_width((MOST * pango::SCALE as f64) as i32);
            layout.set_ellipsize(pango::EllipsizeMode::End);
        }
        let (tw, th) = layout.pixel_size();
        let (w, h) = (f64::from(tw) + 16.0, f64::from(th) + 8.0);
        let (x, y, rw, rh) = of;
        let at = if rw > MOST { x + rw - w } else { x + (rw - w) / 2.0 };
        let left = sh.x + EDGE;
        let x = at.clamp(left, (sh.x + sh.w - EDGE - w).max(left));
        let under = y + rh + 3.0;
        let y = if under + h <= sh.y + sh.h - 4.0 { under } else { y - h - 3.0 };
        squircle(cr, x, y, w, h, 9.0, 0.0);
        let (r, g, b) = crate::theme::mix(pal.bg, pal.fg, 0.14);
        cr.set_source_rgba(r, g, b, 1.0);
        let _ = cr.fill();
        cr.move_to(x + 8.0, y + 4.0);
        let (r, g, b) = pal.fg;
        cr.set_source_rgba(r, g, b, 0.9);
        pangocairo::functions::show_layout(cr, &layout);
    }

    /// A tip right-aligned to `end` (x, centre y), on a backing that hides the search text under it.
    fn paint_row_tip(&self, cr: &Context, tip: &str, end: (f64, f64), pal: &crate::theme::Palette, cfg: &Config) {
        let layout = pangocairo::functions::create_layout(cr);
        let mut d = pango::FontDescription::new();
        d.set_family(&cfg.font);
        d.set_weight(pango::Weight::Medium);
        d.set_absolute_size(11.5 * pango::SCALE as f64);
        layout.set_font_description(Some(&d));
        layout.set_text(tip);
        let (w, h) = layout.pixel_size();
        let (x, y) = (end.0 - w as f64, end.1 - h as f64 / 2.0);
        squircle(cr, x - 9.0, y - 4.0, w as f64 + 18.0, h as f64 + 8.0, 9.0, 0.0);
        let (r, g, b) = pal.bg;
        cr.set_source_rgba(r, g, b, 1.0);
        let _ = cr.fill();
        cr.move_to(x, y);
        let (r, g, b) = pal.fg;
        cr.set_source_rgba(r, g, b, 0.8);
        pangocairo::functions::show_layout(cr, &layout);
    }

    /// The content the pointer can currently reach, and its origin in surface coordinates.
    fn active(&self) -> Option<(&Content, f64, f64)> {
        // Where it's drawn this frame, not where it's headed: that's what's being pointed at.
        let sh = self.shape();
        if self.returning() {
            None
        } else if self.t.target > 0.5 {
            let c = self.panel.as_ref()?;
            let (x, y) = self.panel_at(sh, c);
            Some((c, x, y))
        } else if self.k.target > 0.5 {
            None
        } else {
            let c = self.compact.as_ref()?;
            let (x, y) = self.compact_at(sh, c);
            Some((c, x, y))
        }
    }

    fn panel_at(&self, sh: Shape, c: &Content) -> (f64, f64) {
        (sh.cx - c.w / 2.0, if self.opens_down() { sh.y } else { sh.y + sh.h - c.h })
    }

    fn compact_at(&self, sh: Shape, c: &Content) -> (f64, f64) {
        let x = if sh.w > c.w { sh.cx - c.w / 2.0 } else { sh.x };
        let y = match () {
            // Drawn back, it slides up into the edge with it.
            _ if self.style.attached() => sh.y - PILL_H * self.k.x.clamp(0.0, 1.0),
            _ if self.opens_down() => sh.y,
            _ => sh.y + sh.h - PILL_H,
        };
        (x, y)
    }

    fn hit(&self, (x, y): (f64, f64)) -> Option<usize> {
        let sh = self.shape();
        if x < sh.x || x >= sh.x + sh.w || y < sh.y || y >= sh.y + sh.h {
            return None;
        }
        // Not until it has faded in far enough to see.
        if self.t.target > 0.5 && self.t.x < 0.5 {
            return None;
        }
        let (c, ox, oy) = self.active()?;
        c.hit((x - ox, y - oy))
    }

    fn slider_frac(&self, held: &crate::pen::Region) -> f64 {
        let Some((c, ox, _)) = self.active() else { return 0.0 };
        // A redraw can move it under the pointer.
        let r = c.regions.iter().find(|r| r.left == held.left).unwrap_or(held).rect;
        // The region is padded 6px either side so the thumb can reach both ends.
        ((self.pointer.0 - ox - r.0 - 6.0) / (r.2 - 12.0)).clamp(0.0, 1.0)
    }

    pub fn enter(&mut self, at: (f64, f64), ui: &crate::settings::Ui) {
        self.hovered = true;
        self.close_at = None;
        self.pointer = at;
        if ui.hover_open && !self.suppress && !self.open {
            self.open_at = Some(Instant::now() + Duration::from_millis(ui.hover_open_ms));
        }
        self.dirty = true;
    }

    pub fn leave(&mut self, ui: &crate::settings::Ui) {
        self.hovered = false;
        self.suppress = false;
        self.open_at = None;
        self.press = None;
        self.sliding = false;
        if self.open {
            self.close_at = Some(Instant::now() + Duration::from_millis(ui.hover_close_ms));
        }
        self.hover = None;
        self.dirty = true;
    }

    fn update_hover(&mut self) {
        let hover = self.hit(self.pointer);
        if hover != self.hover {
            self.hover = hover;
            self.dirty = true;
        }
    }

    pub fn dragging(&self) -> bool {
        self.dragging
    }

    pub fn hovering_target(&self) -> bool {
        self.hover.and_then(|i| self.active()?.0.regions.get(i)).is_some_and(|r| !r.inert())
    }

    pub fn motion(&mut self, at: (f64, f64)) -> Outcome {
        self.pointer = at;
        if self.sliding {
            let Some(held) = self.press.as_ref().and_then(|p| p.region.as_ref()) else { return Outcome::None };
            return Outcome::Act(held.left.clone(), self.slider_frac(held));
        }
        if let Some(p) = &self.press {
            let moved = (at.0 - p.at.0).hypot(at.1 - p.at.1);
            if p.button == BTN_LEFT && !self.dragging && moved > DRAG_START && self.view == View::Main && !self.style.attached() {
                self.start_drag();
            }
        }
        if !self.dragging {
            self.update_hover();
        }
        Outcome::None
    }

    fn start_drag(&mut self) {
        self.dragging = true;
        self.open_at = None;
        self.close_at = None;
        self.hover = None;
        let at = (self.place.0.x, self.place.1.x);
        self.pos = Some(at);
        self.dirty = true;
    }

    /// Relative deltas rather than surface coordinates: the surface moves
    /// underneath the pointer during a drag, so surface-local positions would
    /// race our own margin commits and jitter.
    pub fn relative(&mut self, (dx, dy): (f64, f64)) {
        if !self.dragging {
            return;
        }
        let x = (self.place.0.x + dx).clamp(0.0, self.out.0);
        let y = (self.place.1.x + dy).clamp(0.0, self.out.1 - PILL_H);
        self.place.0.snap(x);
        self.place.1.snap(y);
        self.pos = Some((x, y));
        self.dirty = true;
    }

    pub fn press(&mut self, button: u32) -> Outcome {
        self.open_at = None;
        let region = self.hit(self.pointer).and_then(|i| self.active()?.0.regions.get(i).cloned());
        let slider = region.clone().filter(|r| button == BTN_LEFT && is_slider(&r.left));
        self.press = Some(Press { button, at: self.pointer, region });
        if let Some(held) = slider {
            self.sliding = true;
            let frac = self.slider_frac(&held);
            return Outcome::Act(held.left, frac);
        }
        Outcome::None
    }

    pub fn release(&mut self) -> Outcome {
        let Some(p) = self.press.take() else { return Outcome::None };
        if self.sliding {
            self.sliding = false;
            return Outcome::None;
        }
        if self.dragging {
            self.finish_drag();
            return Outcome::Moved;
        }
        if let Some(pressed) = &p.region {
            let under = self.hit(self.pointer);
            // Let go off it, or it changed underneath: that's a click on nothing, not on the background.
            let Some(r) = self.active().and_then(|(c, ..)| c.same(under, pressed)) else { return Outcome::None };
            let action = match p.button {
                BTN_LEFT => &r.left,
                BTN_RIGHT => &r.right,
                BTN_MIDDLE => &r.middle,
                _ => &Action::None,
            };
            return Outcome::Act(action.clone(), 0.0);
        }
        if self.view != View::Main {
            return Outcome::None;
        }
        match p.button {
            BTN_LEFT => {
                // A pin waits out the double-click window, so a double click
                // doesn't flash the panel open and leave it pinned.
                if self.pin_at.take().is_some() {
                    return Outcome::Act(Action::Reset, 0.0);
                }
                self.pin_at = Some(Instant::now() + DOUBLE_CLICK);
                Outcome::None
            }
            BTN_MIDDLE => Outcome::Act(Action::Tuck, 0.0),
            _ => Outcome::None,
        }
    }

    fn finish_drag(&mut self) {
        self.dragging = false;
        // Somewhere else has different room.
        self.panel_dirty = true;
        self.suppress = true; // no hover expansion from the drop until the pointer leaves
        let (x, y) = (self.place.0.x, self.place.1.x);
        let home = self.home();
        if (x - home.0).hypot(y - home.1) < SNAP {
            self.reset();
        } else {
            let at = clamp_pos((x, y), self.out, self.cw.target);
            self.pos = Some(at);
            self.place.0.target = at.0;
            self.place.1.target = at.1;
        }
        self.dirty = true;
    }

    /// Back to top centre, animated. The anchor switches once it arrives.
    pub fn reset(&mut self) {
        self.pos = None;
        let home = self.home();
        self.place.0.target = home.0;
        self.place.1.target = home.1;
        self.panel_dirty = true;
        self.dirty = true;
    }

    pub fn scroll_kind(&self) -> Scroll {
        if self.view == View::Palette {
            return Scroll::Palette;
        }
        let on = self.hover.and_then(|i| self.active()?.0.regions.get(i)?.scroll);
        let rest = if self.t.target > 0.5 && self.overflows() { Scroll::Panel } else { Scroll::Tags };
        on.unwrap_or(rest)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        [self.open_at, self.close_at, self.flash_until, self.pin_at].into_iter().flatten().min()
    }

    /// True once when a background click turned out not to be a double click.
    pub fn take_pin(&mut self, now: Instant) -> bool {
        let due = self.pin_at.is_some_and(|t| t <= now);
        if due {
            self.pin_at = None;
        }
        due
    }

    pub fn tick(&mut self, now: Instant) {
        if self.open_at.is_some_and(|t| t <= now) {
            self.open_at = None;
            if self.hovered && self.press.is_none() {
                self.open = true;
                self.flash_until = None;
                self.dirty = true;
            }
        }
        if self.close_at.is_some_and(|t| t <= now) {
            self.close_at = None;
            if !self.hovered {
                self.open = false;
                self.dirty = true;
            }
        }
        if self.flash_until.is_some_and(|t| t <= now) {
            self.flash_until = None;
            self.invalidate(true, false);
        }
    }
}

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

fn tip_is_empty(t: Option<&str>) -> bool {
    t.is_none_or(str::is_empty)
}

pub(crate) fn blit(cr: &Context, c: &Content, x: f64, y: f64, w: f64, h: f64, a: f64) {
    let _ = cr.save();
    cr.rectangle(x, y, w, h);
    cr.clip();
    if cr.set_source_surface(&c.surf, x, y).is_ok() {
        let _ = cr.paint_with_alpha(a);
    }
    let _ = cr.restore();
}

/// What a frame that only moves visualizers has to repaint.
pub(crate) fn live_bounds(live: &[Live], origin: (f64, f64)) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for l in live {
        let (x, y, w, h) = l.rect;
        // A pixel over, for antialiasing.
        (x0, y0) = (x0.min(x - 1.0), y0.min(y - 1.0));
        (x1, y1) = (x1.max(x + w + 1.0), y1.max(y + h + 1.0));
    }
    (!live.is_empty()).then_some((x0 + origin.0, y0 + origin.1, x1 - x0, y1 - y0))
}

/// Fits the tallest panel, so opening never needs a layer resize.
fn surface_size(out: (f64, f64), style: Style) -> (f64, f64) {
    let compact = if style == Style::Split { MAX_COMPACT_W + modules::BUBBLE_W } else { MAX_COMPACT_W };
    let w = (PANEL_W.max(compact) + 2.0 * SHADOW).min(out.0);
    let h = (MAX_PANEL_H + 2.0 * SHADOW + SLACK).min(out.1);
    (w.floor(), h.floor())
}

/// The stretch of a bar that is the island's to paint: exactly its surface, in units.
pub(crate) fn middle(out: (f64, f64), style: Style, scale: Scale) -> (f64, f64) {
    let size = surface_size(out, style);
    let x = (home(out, style).0 - size.0 / 2.0).clamp(0.0, (out.0 - size.0).max(0.0)).round();
    (f64::from(scale.wl(x)) / scale.ui, f64::from(scale.size(size).0) / scale.ui)
}

/// Centred on the space the windows get, which the frame's rail pushes right.
fn home(out: (f64, f64), style: Style) -> (f64, f64) {
    let i = style.insets();
    (((i.left + out.0 - i.right) / 2.0).round(), style.dock_y())
}

fn clamp_pos((x, y): (f64, f64), out: (f64, f64), pill_w: f64) -> (f64, f64) {
    let half = pill_w / 2.0 + 4.0;
    (x.clamp(half.min(out.0 / 2.0), (out.0 - half).max(out.0 / 2.0)), y.clamp(4.0, (out.1 - PILL_H - 4.0).max(4.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_reaches_target_with_slight_overshoot_at_any_frame_rate() {
        for fps in [60.0, 144.0, 320.0] {
            let mut s = Spring::new(0.0, 0.42, 0.74, 0.002);
            s.target = 1.0;
            let (mut peak, mut frames) = (0.0f64, 0);
            while !s.settled() && frames < 2000 {
                s.step(1.0 / fps);
                peak = peak.max(s.x);
                frames += 1;
            }
            assert_eq!(s.x, 1.0, "settles exactly at {fps} fps, so idle stops rendering");
            assert!(peak > 1.0 && peak < 1.1, "gentle overshoot at {fps} fps: {peak}");
            assert!((frames as f64 / fps) < 1.0, "settles quickly at {fps} fps");
        }
    }

    #[test]
    fn positions_stay_on_screen() {
        assert_eq!(clamp_pos((-50.0, 5000.0), (1920.0, 1080.0), 200.0), (104.0, 1044.0));
        assert_eq!(clamp_pos((960.0, 500.0), (1920.0, 1080.0), 200.0), (960.0, 500.0));
    }
}
