//! What the frame and bar styles put round the screen: thin strips rather than one
//! screen-sized surface (a megabyte, not sixteen), and the space they take from the
//! windows. Strips redraw on events only; nothing here animates.
use crate::{
    Model,
    canvas::Canvas,
    icons::Icons,
    modules,
    pen::{Action, Content, Rect, Scroll},
    settings::Config,
    style::{self, Insets, Style},
    theme::Rgb,
};
use smithay_client_toolkit::{
    compositor::{CompositorState, FrameCallbackData, Region},
    reexports::client::{QueueHandle, protocol::{wl_output, wl_surface::WlSurface}},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::Shm,
};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// Where each strip sits on the output.
fn strips(out: (f64, f64), style: Style) -> Vec<(Side, Rect)> {
    let i = style.insets();
    let r = style::FRAME_R;
    let (w, h) = out;
    match style {
        Style::Frame => {
            let (top, bottom) = (i.top + r, i.bottom + r);
            let middle = h - top - bottom;
            vec![
                // Top and bottom carry the corners, so the sides are only as wide as the border.
                (Side::Top, (0.0, 0.0, w, top)),
                (Side::Bottom, (0.0, h - bottom, w, bottom)),
                (Side::Left, (0.0, top, i.left, middle)),
                (Side::Right, (w - i.right, top, i.right, middle)),
            ]
        }
        Style::Bar => vec![(Side::Top, (0.0, 0.0, w, i.top + r))],
        _ => Vec::new(),
    }
}

/// The part of a strip that's solid, in its own coordinates.
fn solid(side: Side, (_, _, w, h): Rect, i: Insets) -> Rect {
    match side {
        Side::Top => (0.0, 0.0, w, i.top),
        Side::Bottom => (0.0, h - i.bottom, w, i.bottom),
        Side::Left | Side::Right => (0.0, 0.0, w, h),
    }
}

struct Strip {
    side: Side,
    rect: Rect,
    layer: LayerSurface,
    canvas: Canvas,
    configured: bool,
    waiting: bool,
    dirty: bool,
    content: Option<Content>,
    hover: Option<usize>,
    bg: Option<Rgb>,
}

/// An invisible pixel that holds one edge's space: a drawn strip is bigger than
/// what it reserves, and an exclusive zone can't say that.
struct Reserve {
    layer: LayerSurface,
    canvas: Canvas,
}

pub struct Edges {
    pub output: wl_output::WlOutput,
    pub name: String,
    out: (f64, f64),
    style: Style,
    strips: Vec<Strip>,
    reserves: Vec<Reserve>,
    pressed: Option<usize>,
}

/// Where an edge event landed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct At {
    pub edges: usize,
    strip: usize,
}

impl Edges {
    /// Create these before the island: layers stack in creation order, and the
    /// island has to be over the band it grows out of.
    #[allow(clippy::too_many_arguments)]
    pub fn new(output: &wl_output::WlOutput, name: &str, out: (f64, f64), scale: i32, style: Style, shm: &Shm,
               shell: &LayerShell, compositor: &CompositorState, qh: &QueueHandle<crate::App>) -> Option<Self> {
        if style.insets() == Insets::default() {
            return None;
        }
        let create = |anchor: Anchor, zone: i32, (w, h): (u32, u32), (top, left): (i32, i32), input: Option<Rect>| {
            let surface = compositor.create_surface(qh);
            let layer = shell.create_layer_surface(qh, surface, Layer::Top, Some(crate::NAMESPACE), Some(output));
            layer.set_anchor(anchor);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.set_exclusive_zone(zone);
            layer.set_size(w, h);
            layer.set_margin(top, 0, 0, left);
            if let Ok(region) = Region::new(compositor) {
                if let Some((x, y, w, h)) = input {
                    region.add(x as i32, y as i32, w.ceil() as i32, h.ceil() as i32);
                }
                layer.wl_surface().set_input_region(Some(region.wl_region()));
            }
            layer.commit();
            layer
        };
        let i = style.insets();
        let mut reserves = Vec::new();
        for (anchor, zone) in [(Anchor::LEFT, i.left), (Anchor::RIGHT, i.right), (Anchor::TOP, i.top), (Anchor::BOTTOM, i.bottom)] {
            if zone > 0.0 {
                let layer = create(anchor, zone as i32, (1, 1), (0, 0), None);
                reserves.push(Reserve { layer, canvas: Canvas::new(shm, (1.0, 1.0), 1)? });
            }
        }
        let mut drawn = Vec::new();
        for (side, rect) in strips(out, style) {
            let size = (rect.2.round(), rect.3.round());
            let at = (rect.1 as i32, rect.0 as i32);
            let layer = create(Anchor::TOP | Anchor::LEFT, -1, (size.0 as u32, size.1 as u32), at, Some(solid(side, rect, i)));
            let canvas = Canvas::new(shm, size, scale)?;
            drawn.push(Strip { side, rect, layer, canvas, configured: false, waiting: false, dirty: true, content: None, hover: None, bg: None });
        }
        Some(Edges { output: output.clone(), name: name.to_owned(), out, style, strips: drawn, reserves, pressed: None })
    }

    pub fn configured(&mut self, layer: &LayerSurface) {
        if let Some(r) = self.reserves.iter_mut().find(|r| &r.layer == layer) {
            // Mapped, it starts holding its space. It never draws again.
            if let Some(frame) = r.canvas.begin((0.0, 0.0, 1.0, 1.0)) {
                r.canvas.present(frame, r.layer.wl_surface());
                r.layer.commit();
            }
        }
        if let Some(s) = self.strips.iter_mut().find(|s| &s.layer == layer) {
            s.configured = true;
        }
    }

    pub fn frame_done(&mut self, surface: &WlSurface) {
        if let Some(s) = self.strips.iter_mut().find(|s| s.layer.wl_surface() == surface) {
            s.waiting = false;
        }
    }

    pub fn find(&self, surface: &WlSurface, edges: usize) -> Option<At> {
        self.strips.iter().position(|s| s.layer.wl_surface() == surface).map(|strip| At { edges, strip })
    }

    fn carries_content(&self, side: Side) -> bool {
        matches!((self.style, side), (Style::Frame, Side::Left) | (Style::Bar, Side::Top))
    }

    /// Something the rail or the bar's ends show has changed.
    pub fn invalidate(&mut self) {
        for i in 0..self.strips.len() {
            if self.carries_content(self.strips[i].side) {
                self.strips[i].content = None;
                self.strips[i].dirty = true;
            }
        }
    }

    pub fn flush(&mut self, m: &Model, icons: &mut Icons, cfg: &Config, qh: &QueueHandle<crate::App>) {
        let insets = self.style.insets();
        for i in 0..self.strips.len() {
            let carries = self.carries_content(self.strips[i].side);
            let s = &mut self.strips[i];
            s.dirty |= s.bg != Some(m.pal.bg);
            if !(s.configured && s.dirty && !s.waiting) {
                continue;
            }
            let (x, y, w, h) = s.rect;
            if carries && s.content.is_none() {
                let scale = s.canvas.scale();
                s.content = match self.style {
                    Style::Frame => modules::rail(m, icons, cfg, &self.name, (w, h), scale),
                    _ => modules::bar(m, icons, cfg, &self.name, (w, insets.top), scale),
                };
            }
            let Some(frame) = s.canvas.begin((0.0, 0.0, w, h)) else { continue };
            let cr = &frame.cr;
            let _ = cr.save();
            cr.translate(-x, -y);
            style::frame(cr, self.out, insets, style::FRAME_R);
            let (r, g, b) = m.pal.bg;
            cr.set_source_rgba(r, g, b, 1.0);
            let _ = cr.fill();
            let _ = cr.restore();
            if let Some(c) = &s.content {
                if cr.set_source_surface(&c.surf, 0.0, 0.0).is_ok() {
                    let _ = cr.paint();
                }
                if let Some(reg) = s.hover.and_then(|i| c.regions.get(i)) {
                    let (rx, ry, rw, rh) = reg.rect;
                    crate::pen::rounded_rect(cr, rx, ry, rw, rh, reg.radius);
                    let (r, g, b) = m.pal.fg;
                    cr.set_source_rgba(r, g, b, 0.1);
                    let _ = cr.fill();
                }
            }
            let surface = s.layer.wl_surface();
            s.canvas.present(frame, surface);
            surface.frame(qh, FrameCallbackData(surface.clone()));
            s.layer.commit();
            s.waiting = true;
            s.dirty = false;
            s.bg = Some(m.pal.bg);
        }
    }

    pub fn motion(&mut self, at: At, (x, y): (f64, f64)) {
        let s = &mut self.strips[at.strip];
        let hover = s.content.as_ref().and_then(|c| {
            c.regions.iter().rposition(|r| x >= r.rect.0 && x < r.rect.0 + r.rect.2 && y >= r.rect.1 && y < r.rect.1 + r.rect.3)
        });
        if hover != s.hover {
            s.hover = hover;
            s.dirty = true;
        }
    }

    pub fn leave(&mut self, at: At) {
        self.pressed = None;
        let s = &mut self.strips[at.strip];
        if s.hover.take().is_some() {
            s.dirty = true;
        }
    }

    pub fn press(&mut self, at: At) {
        self.pressed = self.strips[at.strip].hover;
    }

    pub fn release(&mut self, at: At, button: u32) -> Action {
        let s = &self.strips[at.strip];
        let same = self.pressed.take().filter(|p| Some(*p) == s.hover);
        let Some(r) = same.and_then(|i| s.content.as_ref()?.regions.get(i)) else { return Action::None };
        match button {
            crate::island::BTN_LEFT => r.left.clone(),
            crate::island::BTN_RIGHT => r.right.clone(),
            crate::island::BTN_MIDDLE => r.middle.clone(),
            _ => Action::None,
        }
    }

    /// The middle of the frame's right border is the way into the sidebar.
    pub fn at_sidebar(&self, at: At, (_, y): (f64, f64)) -> bool {
        let s = &self.strips[at.strip];
        s.side == Side::Right && (y - s.rect.3 / 2.0).abs() < s.rect.3 * 0.3
    }

    pub fn hovering_target(&self, at: At) -> bool {
        self.strips[at.strip].hover.is_some()
    }

    pub fn scroll_kind(&self, at: At) -> Scroll {
        let s = &self.strips[at.strip];
        s.hover.and_then(|i| s.content.as_ref()?.regions.get(i)?.scroll).unwrap_or(Scroll::Tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_cover_the_border_and_its_corners_without_overlapping() {
        let out = (1920.0, 1080.0);
        let s = strips(out, Style::Frame);
        let hit = |x: f64, y: f64| s.iter().filter(|(_, r)| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3).count();
        for (x, y) in [(0.0, 0.0), (20.0, 540.0), (39.9, 27.9), (41.0, 10.0), (1919.0, 540.0), (1910.0, 1070.0), (960.0, 1079.0)] {
            assert_eq!(hit(x, y), 1, "{x},{y}");
        }
        assert_eq!(hit(960.0, 540.0), 0, "nothing over the windows");
        let bytes: f64 = s.iter().map(|(_, r)| r.2 * r.3 * 4.0).sum();
        assert!(bytes < 1024.0 * 1024.0, "a megabyte at 1080p at most: {bytes}");
        assert!(strips(out, Style::Notch).is_empty());
    }

    #[test]
    fn only_the_band_takes_clicks() {
        let i = Style::Frame.insets();
        for (side, rect) in strips((1920.0, 1080.0), Style::Frame) {
            let (_, y, _, h) = solid(side, rect, i);
            match side {
                Side::Top => assert_eq!((y, h), (0.0, style::BORDER)),
                Side::Bottom => assert_eq!((y + h, h), (rect.3, style::BORDER)),
                _ => assert_eq!(h, rect.3),
            }
        }
    }
}
