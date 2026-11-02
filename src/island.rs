//! One island per output: the hover, drag, pin and tuck state machine, the
//! spring animation, and the frame loop. A single large surface stays put
//! while the pill morphs inside it, because resizing a layer surface takes a
//! configure round trip per frame. The input region tracks the visible shape,
//! so the transparent rest of the surface never eats clicks.
use crate::{
    Model,
    modules::{self, MAX_COMPACT_W, MAX_PANEL_H, PANEL_W, PILL_H},
    pen::{Action, Content, Scroll, View},
    icons::Icons,
    settings::Config,
};
use cairo::{Context, Format, ImageSurface};
use smithay_client_toolkit::{
    compositor::{CompositorState, FrameCallbackData, Region},
    reexports::client::{QueueHandle, protocol::{wl_output, wl_surface::WlSurface}},
    shell::{WaylandSurface, wlr_layer::{KeyboardInteractivity, Layer, LayerSurface}},
    shm::{Shm, slot::{Buffer, SlotPool}},
};
use std::time::{Duration, Instant};

pub const DOCK_Y: f64 = 6.0;
const SHADOW: f64 = 26.0;
const PANEL_R: f64 = 30.0;
const SNAP: f64 = 56.0; // release this close to home and the pill docks
const DRAG_START: f64 = 5.0;
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
    fn new(x: f64, period: f64, zeta: f64, eps: f64) -> Self {
        let w = std::f64::consts::TAU / period;
        Spring { x, v: 0.0, target: x, k: w * w, c: 2.0 * zeta * w, eps }
    }

    fn tune(&mut self, period: f64, zeta: f64) {
        let w = std::f64::consts::TAU / period;
        self.k = w * w;
        self.c = 2.0 * zeta * w;
    }

    fn step(&mut self, dt: f64) {
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

    fn settled(&self) -> bool {
        (self.x - self.target).abs() < self.eps && self.v.abs() < self.eps * 10.0
    }

    fn snap(&mut self, x: f64) {
        self.x = x;
        self.target = x;
        self.v = 0.0;
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smooth(t: f64) -> f64 {
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
    region: Option<usize>,
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
    pub scale: i32,
    pub layer: LayerSurface,
    configured: bool,
    pool: SlotPool,
    buffers: Vec<Buffer>,
    size: (f64, f64),

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
    compact_dirty: bool,
    panel_dirty: bool,

    frame_pending: bool,
    dirty: bool,
    prev_bounds: (f64, f64, f64, f64),
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
    pub fn new(output: wl_output::WlOutput, name: String, out: (f64, f64), scale: i32, layer: LayerSurface,
               shm: &Shm, state_pos: Option<(f64, f64)>, pinned: bool, tucked: bool) -> Option<Self> {
        let size = surface_size(out);
        let pool = SlotPool::new((size.0 * size.1) as usize * 4 * scale.pow(2) as usize, shm).ok()?;
        let home = (out.0 / 2.0, DOCK_Y);
        let pos = state_pos.map(|p| clamp_pos(p, out, 200.0));
        let at = pos.unwrap_or(home);
        Some(Island {
            output, name, out, scale, layer, configured: false, pool, buffers: Vec::new(), size, pos,
            place: (Spring::new(at.0, 0.34, 0.82, 0.3), Spring::new(at.1, 0.34, 0.82, 0.3)),
            placed: None, input: None,
            t: Spring::new(if pinned { 1.0 } else { 0.0 }, 0.42, 0.78, 0.002),
            cw: Spring::new(200.0, 0.36, 0.86, 0.3),
            ph: Spring::new(320.0, 0.36, 0.86, 0.3),
            pw: Spring::new(PANEL_W, 0.36, 0.86, 0.3),
            k: Spring::new(if tucked { 1.0 } else { 0.0 }, 0.34, 0.86, 0.002),
            last: None, time_scale: std::env::var("ISLAND_TIME_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            pinned, tucked, toast: false, raised: false, hovered: false, open: false, suppress: false, press: None,
            dragging: false, sliding: false, pointer: (0.0, 0.0), hover: None, pin_at: None,
            open_at: None, close_at: None, flash_until: None, view: View::Main,
            compact: None, panel: None, compact_dirty: true, panel_dirty: true,
            frame_pending: false, dirty: true, prev_bounds: (0.0, 0.0, 0.0, 0.0),
        })
    }

    pub fn surface_size(&self) -> (f64, f64) {
        self.size
    }

    pub fn surface(&self) -> &WlSurface {
        self.layer.wl_surface()
    }

    pub fn configured(&mut self) {
        self.configured = true;
        self.dirty = true;
    }

    pub fn wants_panel(&self) -> bool {
        (self.pinned || self.open || self.toast || self.view == View::Palette) && !self.dragging
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
        self.view = if self.pinned || self.open { View::Main } else { View::Toast };
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

    pub fn set_output(&mut self, out: (f64, f64), scale: i32) {
        if scale != self.scale {
            self.scale = scale;
            self.buffers.clear();
            self.compact_dirty = true;
            self.panel_dirty = true;
        }
        let size = surface_size(out);
        if size != self.size {
            self.size = size;
            self.buffers.clear();
            self.layer.set_size(size.0 as u32, size.1 as u32);
        }
        self.out = out;
        self.pos = self.pos.map(|p| clamp_pos(p, out, self.cw.target));
        let at = self.pos.unwrap_or(self.home());
        self.place.0.snap(at.0);
        self.place.1.snap(at.1);
        self.placed = None;
        self.dirty = true;
    }

    fn home(&self) -> (f64, f64) {
        (self.out.0 / 2.0, DOCK_Y)
    }

    fn opens_down(&self) -> bool {
        self.place.1.x + PILL_H / 2.0 < self.out.1 / 2.0
    }

    /// Surface top-left on the output, keeping the whole panel on screen.
    fn surface_pos(&self) -> (f64, f64) {
        let (px, py) = (self.place.0.x, self.place.1.x);
        let x = (px - self.size.0 / 2.0).clamp(0.0, (self.out.0 - self.size.0).max(0.0));
        let y = if self.opens_down() { (py - SHADOW).max(0.0) } else { py + PILL_H + SHADOW - self.size.1 };
        (x.round(), y.clamp(0.0, (self.out.1 - self.size.1).max(0.0)).round())
    }

    fn shape(&self) -> Shape {
        let (sx, sy) = self.surface_pos();
        let (px, py) = (self.place.0.x - sx, self.place.1.x - sy);
        let t = self.t.x;
        let k = self.k.x;
        // The panel slides sideways as it opens if the pill sits near an edge.
        let panel_cx = self.panel_cx(self.place.0.x, self.pw.x) - sx;
        let cx = lerp(px, panel_cx, t.clamp(0.0, 1.0));
        let w = lerp(lerp(self.cw.x, TUCK_W, k), self.pw.x, t);
        let h = lerp(lerp(PILL_H, TUCK_H, k), self.ph.x, t);
        let top = lerp(py, if self.pos.is_none() { 2.0 } else { py }, k);
        let y = if self.opens_down() { top } else { py + PILL_H - h };
        let r = lerp(PILL_H / 2.0, PANEL_R, t.clamp(0.0, 1.0)).min(h / 2.0);
        Shape { x: cx - w / 2.0, y, w, h, r, cx }
    }

    /// Where the input region should sit at rest for the current target.
    fn target_rect(&self) -> (f64, f64, f64, f64) {
        let (sx, sy) = self.surface_pos();
        let (px, py) = (self.place.0.target - sx, self.place.1.target - sy);
        if self.t.target > 0.5 {
            let w = self.pw.target;
            let cx = self.panel_cx(self.place.0.target, w) - sx;
            let y = if self.opens_down() { py } else { py + PILL_H - self.ph.target };
            (cx - w / 2.0, y, w, self.ph.target)
        } else if self.k.target > 0.5 {
            // A thin sliver is a hard target: keep a generous hot zone around it.
            let y = if self.pos.is_none() { 0.0 } else { py };
            (px - 70.0, y, 140.0, 12.0)
        } else {
            (px - self.cw.target / 2.0, py, self.cw.target, PILL_H)
        }
    }

    fn apply_input(&mut self, compositor: &CompositorState) {
        let r = self.target_rect();
        let rect = (r.0.floor() as i32, r.1.floor() as i32, r.2.ceil() as i32, r.3.ceil() as i32);
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
        let want = (x as i32, y as i32);
        if self.placed != Some(want) {
            self.placed = Some(want);
            self.layer.set_margin(want.1, 0, 0, want.0);
        }
    }

    fn update_targets(&mut self) {
        let opening = self.wants_panel();
        if opening && self.t.target < 0.5 {
            self.t.tune(0.42, 0.74);
        } else if !opening && self.t.target > 0.5 {
            self.t.tune(0.34, 0.92);
        }
        self.t.target = if opening { 1.0 } else { 0.0 };
        self.k.target = if self.tucked && !opening && !self.dragging { 1.0 } else { 0.0 };
    }

    fn springs_settled(&self) -> bool {
        [&self.t, &self.cw, &self.ph, &self.pw, &self.k, &self.place.0, &self.place.1].iter().all(|s| s.settled())
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
                self.compact = Some(c);
            }
        }
        // A hover that outlives a toast carries on as the normal panel.
        if self.view == View::Toast && !self.toast && (self.pinned || self.open) {
            self.view = View::Main;
            self.panel_dirty = true;
        }
        if self.panel_visible() {
            // While closing, keep showing what was open rather than swapping it out.
            if (self.panel_dirty && self.wants_panel()) || self.panel.is_none() {
                self.panel_dirty = false;
                let first = self.panel.is_none();
                if let Some(c) = modules::panel(m, icons, cfg, &self.name, &self.view, self.scale, self.pinned, self.tucked) {
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
        if self.configured && self.dirty && !self.frame_pending {
            self.render(m, icons, cfg, compositor, qh);
        }
    }

    pub fn frame_done(&mut self) {
        self.frame_pending = false;
    }

    fn render(&mut self, m: &Model, icons: &mut Icons, cfg: &Config, compositor: &CompositorState, qh: &QueueHandle<crate::App>) {
        self.update_targets();
        self.rebuild(m, icons, cfg);

        let now = Instant::now();
        // The first frame after idle has no meaningful gap to integrate over.
        let dt = self.last.map_or(1.0 / 240.0, |l| (now - l).as_secs_f64()).min(1.0 / 30.0) * self.time_scale;
        for s in [&mut self.t, &mut self.cw, &mut self.ph, &mut self.pw, &mut self.k, &mut self.place.0, &mut self.place.1] {
            s.step(dt);
        }
        let animating = !self.springs_settled();
        self.last = animating.then_some(now);

        self.apply_place();
        self.apply_input(compositor);
        // Regions change with the state and the content; indices don't carry over.
        if self.hovered && !self.dragging {
            self.hover = self.hit(self.pointer);
        }

        let s = self.scale as f64;
        let (bw, bh) = ((self.size.0 * s) as i32, (self.size.1 * s) as i32);
        let stride = bw * 4;
        let free = self.buffers.iter().position(|b| b.canvas(&mut self.pool).is_some());
        let index = match free {
            Some(i) => i,
            None => {
                let format = smithay_client_toolkit::reexports::client::protocol::wl_shm::Format::Argb8888;
                let Ok((buffer, _)) = self.pool.create_buffer(bw, bh, stride, format) else { return };
                self.buffers.push(buffer);
                self.buffers.len() - 1
            }
        };
        let shape = self.shape();
        let Some(canvas) = self.buffers[index].canvas(&mut self.pool) else { return };
        let (ptr, len) = (canvas.as_mut_ptr(), canvas.len());
        debug_assert!(len >= (stride * bh) as usize);
        // Safety: `canvas` is an exclusive view of the shm slot, valid until the
        // buffer is attached below, and the cairo surface is finished before that.
        let target = unsafe { ImageSurface::create_for_data_unsafe(ptr, Format::ARgb32, bw, bh, stride) };
        let Ok(target) = target else { return };
        target.set_device_scale(s, s);
        if let Ok(cr) = Context::new(&target) {
            self.paint(&cr, shape, &m.pal, animating, cfg);
        }
        target.finish();

        let b = (shape.x - SHADOW, shape.y - SHADOW, shape.w + 2.0 * SHADOW, shape.h + 2.0 * SHADOW);
        let (x0, y0) = (b.0.min(self.prev_bounds.0), b.1.min(self.prev_bounds.1));
        let x1 = (b.0 + b.2).max(self.prev_bounds.0 + self.prev_bounds.2);
        let y1 = (b.1 + b.3).max(self.prev_bounds.1 + self.prev_bounds.3);
        self.prev_bounds = b;

        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(self.scale);
        let _ = self.buffers[index].attach_to(surface);
        let surface = self.layer.wl_surface();
        surface.damage_buffer((x0 * s).floor() as i32 - 2, (y0 * s).floor() as i32 - 2,
                              ((x1 - x0) * s).ceil() as i32 + 4, ((y1 - y0) * s).ceil() as i32 + 4);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.layer.commit();
        self.frame_pending = true;
        self.dirty = animating;
    }

    fn paint(&self, cr: &Context, sh: Shape, pal: &crate::theme::Palette, animating: bool, cfg: &Config) {
        let t = self.t.x.clamp(0.0, 1.0);
        let k = self.k.x.clamp(0.0, 1.0);
        cr.set_operator(cairo::Operator::Clear);
        let _ = cr.paint();
        cr.set_operator(cairo::Operator::Over);

        // Soft shadow as nested strokes: only the rim is touched, so a big panel
        // costs what a small one does, and no blur pass is needed.
        let lift = 1.0 - k;
        if lift > 0.0 {
            let depth = lerp(12.0, 24.0, t);
            for i in (1..=6).rev() {
                let f = i as f64 / 6.0;
                squircle(cr, sh.x, sh.y + 2.0, sh.w, sh.h, sh.r, t);
                cr.set_line_width(2.0 * depth * f);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.055 * lift * (1.0 - f * 0.7));
                let _ = cr.stroke();
            }
        }

        squircle(cr, sh.x, sh.y, sh.w, sh.h, sh.r, t);
        let (r, g, b) = pal.bg;
        cr.set_source_rgba(r, g, b, lerp(0.97, 0.72, k));
        let _ = cr.fill();

        let a_compact = (1.0 - smooth(t / 0.45)) * (1.0 - k);
        let a_panel = smooth((t - 0.32) / 0.68);
        let down = self.opens_down();
        let morphing = animating && (t > 0.0 && t < 1.0 || k > 0.0 && k < 1.0);
        if morphing {
            let _ = cr.save();
            squircle(cr, sh.x, sh.y, sh.w, sh.h, sh.r, t);
            cr.clip();
        }
        if let (true, Some(c)) = (a_compact > 0.003, &self.compact) {
            let (x, y) = (sh.cx - sh.w.min(c.w) / 2.0, if down { sh.y } else { sh.y + sh.h - PILL_H });
            let x = if sh.w > c.w { sh.cx - c.w / 2.0 } else { x };
            blit(cr, c, x, y, sh.w.min(c.w), PILL_H, a_compact);
        }
        if let (true, Some(c)) = (a_panel > 0.003, &self.panel) {
            let y = if down { sh.y } else { sh.y + sh.h - c.h };
            blit(cr, c, sh.cx - c.w / 2.0, y, c.w, c.h.min(sh.h + 8.0), a_panel);
        }
        if morphing {
            let _ = cr.restore();
        }

        // Hover feedback sits over the cached content, so moving the pointer
        // never re-lays anything out.
        if let (false, Some(i)) = (self.dragging, self.hover) {
            if let Some((c, ox, oy)) = self.active() {
                if let Some(reg) = c.regions.get(i) {
                    let (x, y, w, h) = reg.rect;
                    squircle(cr, ox + x, oy + y, w, h, reg.radius, 0.0);
                    let (fr, fg, fb) = pal.fg;
                    cr.set_source_rgba(fr, fg, fb, 0.1);
                    let _ = cr.fill();
                    if let (Some(tip), true) = (&reg.tip, self.t.target > 0.5 && !tip_is_empty(reg.tip.as_deref())) {
                        self.paint_tip(cr, tip, sh, pal, cfg);
                    }
                }
            }
        }
    }

    /// Window titles used to live in Waybar's tooltips. Here the panel's top
    /// right corner of the monitor heading carries them.
    fn paint_tip(&self, cr: &Context, tip: &str, sh: Shape, pal: &crate::theme::Palette, cfg: &Config) {
        let layout = pangocairo::functions::create_layout(cr);
        let mut d = pango::FontDescription::new();
        d.set_family(&cfg.font);
        d.set_weight(pango::Weight::Medium);
        d.set_absolute_size(11.0 * pango::SCALE as f64);
        layout.set_font_description(Some(&d));
        layout.set_width((300.0 * pango::SCALE as f64) as i32);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        layout.set_alignment(pango::Alignment::Right);
        layout.set_text(tip);
        cr.move_to(sh.x + sh.w - 20.0 - 300.0, sh.y + 55.0);
        let (r, g, b) = pal.fg;
        cr.set_source_rgba(r, g, b, 0.75);
        pangocairo::functions::show_layout(cr, &layout);
    }

    /// The content the pointer can currently reach, and its origin in surface coordinates.
    fn active(&self) -> Option<(&Content, f64, f64)> {
        let (sx, sy) = self.surface_pos();
        let (px, py) = (self.place.0.target - sx, self.place.1.target - sy);
        if self.t.target > 0.5 {
            let c = self.panel.as_ref()?;
            let cx = self.panel_cx(self.place.0.target, c.w) - sx;
            let y = if self.opens_down() { py } else { py + PILL_H - c.h };
            Some((c, cx - c.w / 2.0, y))
        } else if self.k.target > 0.5 {
            None
        } else {
            let c = self.compact.as_ref()?;
            Some((c, px - c.w / 2.0, py))
        }
    }

    fn hit(&self, (x, y): (f64, f64)) -> Option<usize> {
        let (c, ox, oy) = self.active()?;
        let (x, y) = (x - ox, y - oy);
        // Icons are pushed after their chip, so search top-most first.
        c.regions.iter().rposition(|r| x >= r.rect.0 && x < r.rect.0 + r.rect.2 && y >= r.rect.1 && y < r.rect.1 + r.rect.3)
    }

    fn slider_frac(&self, region: usize) -> f64 {
        let Some((c, ox, _)) = self.active() else { return 0.0 };
        let r = c.regions[region].rect;
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
        self.hover.is_some()
    }

    pub fn motion(&mut self, at: (f64, f64)) -> Outcome {
        self.pointer = at;
        if self.sliding {
            let region = self.press.as_ref().and_then(|p| p.region).unwrap_or(0);
            return Outcome::Act(Action::Volume, self.slider_frac(region));
        }
        if let Some(p) = &self.press {
            let moved = (at.0 - p.at.0).hypot(at.1 - p.at.1);
            if p.button == BTN_LEFT && !self.dragging && moved > DRAG_START && self.view == View::Main {
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
        let region = self.hit(self.pointer);
        self.press = Some(Press { button, at: self.pointer, region });
        if button == BTN_LEFT {
            if let Some(i) = region {
                let is_slider = self.active().is_some_and(|(c, ..)| c.regions[i].left == Action::Volume);
                if is_slider {
                    self.sliding = true;
                    return Outcome::Act(Action::Volume, self.slider_frac(i));
                }
            }
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
        let same = self.hit(self.pointer) == p.region;
        if let (true, Some(i)) = (same, p.region) {
            let Some((c, ..)) = self.active() else { return Outcome::None };
            let r = &c.regions[i];
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
        self.dirty = true;
    }

    pub fn scroll_kind(&self) -> Scroll {
        if self.view == View::Palette {
            return Scroll::Palette;
        }
        self.hover.and_then(|i| self.active()?.0.regions.get(i)?.scroll).unwrap_or(Scroll::Tags)
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

fn blit(cr: &Context, c: &Content, x: f64, y: f64, w: f64, h: f64, a: f64) {
    let _ = cr.save();
    cr.rectangle(x, y, w, h);
    cr.clip();
    if cr.set_source_surface(&c.surf, x, y).is_ok() {
        let _ = cr.paint_with_alpha(a);
    }
    let _ = cr.restore();
}

/// Fits the tallest panel, so opening never needs a layer resize.
fn surface_size(out: (f64, f64)) -> (f64, f64) {
    let w = (PANEL_W.max(MAX_COMPACT_W) + 2.0 * SHADOW).min(out.0);
    let h = (MAX_PANEL_H + SHADOW + 12.0).min(out.1);
    (w.floor(), h.floor())
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
