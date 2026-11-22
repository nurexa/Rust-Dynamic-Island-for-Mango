//! The sidebar. It swells out of the right edge, or the frame's border, with the
//! island's springs and corners. Its surface exists only from opening to closed,
//! so a closed sidebar costs nothing at all.
use crate::{
    Model,
    canvas::Canvas,
    icons::Icons,
    island::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, Outcome, Spring, blit, lerp, live_bounds, smooth},
    modules,
    pen::{Action, Content, Rect, Scroll, is_slider},
    settings::Config,
    style,
};
use cairo::Context;
use smithay_client_toolkit::{
    compositor::{CompositorState, FrameCallbackData, Region},
    reexports::client::{QueueHandle, protocol::{wl_output, wl_surface::WlSurface}},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::Shm,
};
use std::time::{Duration, Instant};

pub const W: f64 = 380.0;
const R: f64 = 30.0;
/// The spring runs a little past open; that needs somewhere to go.
const SLACK: f64 = 28.0;
const SCROLL_STEP: f64 = 48.0;
/// Frames stop coming when nobody can see it.
const STALLED: Duration = Duration::from_secs(1);

pub struct Sidebar {
    pub output: wl_output::WlOutput,
    pub name: String,
    pub layer: LayerSurface,
    canvas: Canvas,
    out: (f64, f64),
    size: (f64, f64),
    scale: i32,
    band: f64,
    /// The middle of the windows' space, and the most it can take of it.
    middle: f64,
    room: f64,
    t: Spring,
    h: Spring,
    last: Option<Instant>,
    time_scale: f64,
    content: Option<Content>,
    total: f64,
    scroll: f64,
    content_dirty: bool,
    dirty: bool,
    live_dirty: bool,
    configured: bool,
    waiting: Option<Instant>,
    input: Option<(i32, i32, i32, i32)>,
    /// Hover opened it, so leaving closes it.
    by_hover: bool,
    typing: bool,
    hovered: bool,
    hover: Option<usize>,
    pointer: (f64, f64),
    press: Option<(u32, Option<usize>)>,
    sliding: bool,
    close_at: Option<Instant>,
}

impl Sidebar {
    #[allow(clippy::too_many_arguments)]
    pub fn open(output: &wl_output::WlOutput, name: &str, out: (f64, f64), scale: i32, style: style::Style, by_hover: bool,
                shm: &Shm, shell: &LayerShell, compositor: &CompositorState, qh: &QueueHandle<crate::App>) -> Option<Self> {
        let i = style.insets();
        let size = ((W + i.right + SLACK).min(out.0).floor(), out.1.floor());
        let canvas = Canvas::new(shm, size, scale)?;
        let surface = compositor.create_surface(qh);
        let layer = shell.create_layer_surface(qh, surface, Layer::Top, Some(crate::NAMESPACE), Some(output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(size.0 as u32, size.1 as u32);
        layer.set_margin(0, 0, 0, (out.0 - size.0) as i32);
        layer.commit();
        let mut t = Spring::new(0.0, 0.42, 0.78, 0.002);
        t.target = 1.0;
        Some(Sidebar {
            output: output.clone(), name: name.to_owned(), layer, canvas, out, size, scale, band: i.right,
            middle: ((i.top + out.1 - i.bottom) / 2.0).round(),
            // Room for the corners where it meets the edge, and the frame's own past those.
            room: (out.1 - i.top - i.bottom - 2.0 * (style::FILLET + 4.0)).max(120.0).floor(),
            t, h: Spring::new(0.0, 0.36, 0.86, 0.3),
            last: None, time_scale: std::env::var("ISLAND_TIME_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            content: None, total: 0.0, scroll: 0.0, content_dirty: true, dirty: true, live_dirty: false,
            configured: false, waiting: None, input: None, by_hover, typing: false, hovered: false, hover: None,
            pointer: (0.0, 0.0), press: None, sliding: false, close_at: None,
        })
    }

    pub fn fits(&self, out: (f64, f64), scale: i32) -> bool {
        self.out == out && self.scale == scale
    }

    pub fn surface(&self) -> &WlSurface {
        self.layer.wl_surface()
    }

    pub fn configured(&mut self) {
        self.configured = true;
        self.dirty = true;
    }

    pub fn frame_done(&mut self) {
        self.waiting = None;
    }

    pub fn is_open(&self) -> bool {
        self.t.target > 0.5
    }

    /// Closed all the way: time to drop it.
    pub fn finished(&self) -> bool {
        !self.is_open() && (self.t.x == 0.0 || !self.configured || self.stalled())
    }

    fn stalled(&self) -> bool {
        self.waiting.is_some_and(|t| t.elapsed() > STALLED)
    }

    pub fn close(&mut self) {
        self.t.tune(0.34, 0.92);
        self.t.target = 0.0;
        self.set_typing(false);
        self.close_at = None;
        self.hover = None;
        self.dirty = true;
    }

    pub fn reopen(&mut self) {
        self.t.tune(0.42, 0.78);
        self.t.target = 1.0;
        self.by_hover = false;
        self.content_dirty = true;
        self.dirty = true;
    }

    /// Opened by hovering the edge, then the edge was left without coming in.
    pub fn edge_left(&mut self, ms: u64) {
        if self.by_hover && !self.hovered && self.is_open() {
            self.close_at = Some(Instant::now() + Duration::from_millis(ms));
        }
    }

    pub fn set_typing(&mut self, on: bool) {
        if self.typing != on {
            self.typing = on;
            self.layer.set_keyboard_interactivity(if on { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
            self.layer.commit();
            self.invalidate();
        }
    }

    pub fn invalidate(&mut self) {
        if self.is_open() {
            self.content_dirty = true;
            self.dirty = true;
        }
    }

    pub fn levels_changed(&mut self) {
        self.live_dirty |= self.content.as_ref().is_some_and(|c| !c.live.is_empty());
    }

    pub fn wants_levels(&self) -> bool {
        self.is_open() && !self.stalled() && self.content.as_ref().is_some_and(|c| !c.live.is_empty())
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.close_at
    }

    pub fn tick(&mut self, now: Instant) {
        if self.close_at.is_some_and(|t| t <= now) {
            self.close_at = None;
            if !self.hovered && !self.typing {
                self.close();
            }
        }
    }

    fn rebuild(&mut self, m: &Model, icons: &mut Icons, cfg: &Config) {
        if !self.content_dirty || !self.is_open() {
            return;
        }
        self.content_dirty = false;
        let Some((mut c, mut total)) = modules::sidebar(m, icons, cfg, &self.name, (W, self.room), self.scroll, self.scale) else { return };
        // What's shown got shorter under the scroll: come back up to its end.
        let most = (total - self.room).max(0.0);
        if self.scroll > most {
            self.scroll = most;
            if let Some(again) = modules::sidebar(m, icons, cfg, &self.name, (W, self.room), most, self.scale) {
                (c, total) = again;
            }
        }
        self.total = total;
        if self.content.is_none() {
            self.h.snap(c.h);
        }
        self.h.target = c.h;
        self.content = Some(c);
    }

    /// What's drawn now: how far out it reaches, its top, and its height.
    fn shape(&self) -> (f64, f64, f64) {
        let t = self.t.x.max(0.0);
        let h = self.h.x * lerp(0.7, 1.0, t.min(1.0));
        ((W * t).min(self.size.0 - self.band), self.middle - h / 2.0, h)
    }

    /// The content's top left once open.
    fn origin(&self) -> (f64, f64) {
        (self.size.0 - self.band - W, self.middle - self.h.target / 2.0)
    }

    pub fn flush(&mut self, m: &Model, icons: &mut Icons, cfg: &Config, compositor: &CompositorState, qh: &QueueHandle<crate::App>) {
        if !self.configured || self.waiting.is_some() || !(self.dirty || self.live_dirty) {
            return;
        }
        let only_live = !self.dirty;
        self.live_dirty = false;
        self.rebuild(m, icons, cfg);
        let now = Instant::now();
        let dt = self.last.map_or(1.0 / 240.0, |l| (now - l).as_secs_f64()).min(1.0 / 30.0) * self.time_scale;
        self.t.step(dt);
        self.h.step(dt);
        let animating = !(self.t.settled() && self.h.settled());
        self.last = animating.then_some(now);
        self.apply_input(compositor);
        if self.hovered {
            self.hover = self.hit();
        }

        let (out, y, h) = self.shape();
        let edge = self.size.0 - self.band;
        let top = y - style::FILLET;
        let bounds = (edge - out, top, out + self.band, h + 2.0 * style::FILLET);
        let origin = (edge - out, y);
        let frame = if only_live && !animating {
            let area = self.content.as_ref().and_then(|c| live_bounds(&c.live, origin));
            match area {
                Some(area) => self.canvas.begin_live(bounds, area),
                None => return,
            }
        } else {
            self.canvas.begin(bounds)
        };
        let Some(frame) = frame else { return };
        self.paint(&frame.cr, frame.patch, (out, y, h), m);

        let surface = self.layer.wl_surface();
        self.canvas.present(frame, surface);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.layer.commit();
        self.waiting = Some(now);
        self.dirty = animating;
    }

    fn paint(&self, cr: &Context, patch: Option<Rect>, (out, y, h): (f64, f64, f64), m: &Model) {
        let edge = self.size.0 - self.band;
        match patch {
            // A visualizer's patch is all inside, so there's no outline to work out.
            Some((px, py, pw, ph)) => cr.rectangle(px, py, pw, ph),
            None => {
                let _ = cr.save();
                // The island's tab, turned to hang from the right.
                cr.translate(self.size.0, 0.0);
                cr.rotate(std::f64::consts::FRAC_PI_2);
                style::tab(cr, (y, y + h), self.band, self.band + out, style::FILLET, R);
                let _ = cr.restore();
            }
        }
        let (r, g, b) = m.pal.bg;
        cr.set_source_rgba(r, g, b, 1.0);
        let _ = cr.fill_preserve();
        let Some(c) = &self.content else {
            cr.new_path();
            return;
        };
        let alpha = smooth((self.t.x - 0.25) / 0.75);
        let _ = cr.save();
        cr.clip();
        let x = edge - out;
        let visualize = |under: bool| {
            for live in c.live.iter().filter(|l| l.look.under() == under) {
                crate::viz::paint(cr, live, (x, y), &m.levels, m.pal.accent, alpha);
            }
        };
        visualize(true);
        blit(cr, c, x, y, out.min(W), h.min(c.h), alpha);
        visualize(false);
        let _ = cr.restore();
        if let Some(reg) = self.hover.and_then(|i| c.regions.get(i)) {
            let (rx, ry, rw, rh) = reg.rect;
            crate::pen::rounded_rect(cr, x + rx, y + ry, rw, rh, reg.radius);
            let (r, g, b) = m.pal.fg;
            cr.set_source_rgba(r, g, b, 0.1);
            let _ = cr.fill();
        }
    }

    fn apply_input(&mut self, compositor: &CompositorState) {
        let r: Rect = match (&self.content, self.is_open()) {
            // The band is in it, so coming in from the border doesn't cross a gap.
            (Some(c), true) => (self.size.0 - self.band - W, self.middle - c.h / 2.0, W + self.band, c.h),
            _ => (0.0, 0.0, 0.0, 0.0),
        };
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

    fn hit(&self) -> Option<usize> {
        if !self.is_open() {
            return None;
        }
        let c = self.content.as_ref()?;
        let (ox, oy) = self.origin();
        let (x, y) = (self.pointer.0 - ox, self.pointer.1 - oy);
        // Scrolled past the top or bottom is out of reach.
        if y < 0.0 || y >= c.h {
            return None;
        }
        c.regions.iter().rposition(|r| x >= r.rect.0 && x < r.rect.0 + r.rect.2 && y >= r.rect.1 && y < r.rect.1 + r.rect.3)
    }

    fn update_hover(&mut self) {
        let hover = self.hit();
        if hover != self.hover {
            self.hover = hover;
            self.dirty = true;
        }
    }

    fn slide(&self, i: usize) -> Option<(Action, f64)> {
        let r = self.content.as_ref()?.regions.get(i)?;
        let x = self.pointer.0 - self.origin().0;
        Some((r.left.clone(), ((x - r.rect.0 - 6.0) / (r.rect.2 - 12.0)).clamp(0.0, 1.0)))
    }

    pub fn enter(&mut self, at: (f64, f64)) {
        self.hovered = true;
        self.close_at = None;
        self.pointer = at;
        self.update_hover();
    }

    pub fn leave(&mut self, ms: u64) {
        self.hovered = false;
        self.press = None;
        self.sliding = false;
        if self.hover.take().is_some() {
            self.dirty = true;
        }
        if self.by_hover && self.is_open() {
            self.close_at = Some(Instant::now() + Duration::from_millis(ms));
        }
    }

    pub fn motion(&mut self, at: (f64, f64)) -> Outcome {
        self.pointer = at;
        if self.sliding {
            if let Some((action, frac)) = self.press.and_then(|p| p.1).and_then(|i| self.slide(i)) {
                return Outcome::Act(action, frac);
            }
        }
        self.update_hover();
        Outcome::None
    }

    pub fn press(&mut self, button: u32) -> Outcome {
        let region = self.hit();
        self.press = Some((button, region));
        if let (BTN_LEFT, Some(i)) = (button, region) {
            if let Some((action, frac)) = self.slide(i).filter(|(a, _)| is_slider(a)) {
                self.sliding = true;
                return Outcome::Act(action, frac);
            }
        }
        Outcome::None
    }

    pub fn release(&mut self) -> Outcome {
        let Some((button, region)) = self.press.take() else { return Outcome::None };
        if std::mem::take(&mut self.sliding) || self.hit() != region {
            return Outcome::None;
        }
        let Some(r) = region.and_then(|i| self.content.as_ref()?.regions.get(i)) else { return Outcome::None };
        let action = match button {
            BTN_LEFT => &r.left,
            BTN_RIGHT => &r.right,
            BTN_MIDDLE => &r.middle,
            _ => &Action::None,
        };
        Outcome::Act(action.clone(), 0.0)
    }

    pub fn hovering_target(&self) -> bool {
        self.hover.is_some()
    }

    pub fn over_volume(&self) -> bool {
        self.hover.and_then(|i| self.content.as_ref()?.regions.get(i)?.scroll) == Some(Scroll::Volume)
    }

    pub fn scroll_by(&mut self, notches: f64) {
        let to = (self.scroll + notches * SCROLL_STEP).clamp(0.0, (self.total - self.room).max(0.0));
        if to != self.scroll {
            self.scroll = to;
            self.invalidate();
        }
    }
}
