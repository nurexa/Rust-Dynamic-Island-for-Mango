//! The island's styles, and the outlines they're made of. Only geometry lives
//! here: no state, no surfaces. An attached style is one shape with the edge it
//! hangs from, so its sides flow out of that edge in reversed corners.
use crate::pen::Rect;
use cairo::Context;
use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// A floating pill you can drag anywhere.
    #[default]
    Pill,
    /// A border round the screen with a rail down the left. The island, the palette
    /// and the sidebar all grow out of it.
    Frame,
    /// Hangs from the top edge of the screen.
    Notch,
    /// A full-width bar that swells into the panel.
    Bar,
    /// The pill, with what's live budding off into a bubble of its own.
    Split,
    /// The time, and whatever needs you.
    Minimal,
}

/// Space a style takes from the windows, per screen edge.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Insets {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

pub const RAIL: f64 = 40.0;
pub const BORDER: f64 = 8.0;
pub const BAR: f64 = 32.0;
/// The frame's inner corners.
pub const FRAME_R: f64 = 20.0;
/// The reversed corners where a shape leaves its edge.
pub const FILLET: f64 = 18.0;
/// Between the pill and its bubble, once they've parted.
pub const SPLIT_GAP: f64 = 7.0;

impl Style {
    pub const ALL: [Style; 6] = [Style::Pill, Style::Frame, Style::Notch, Style::Bar, Style::Split, Style::Minimal];

    pub fn name(self) -> &'static str {
        match self {
            Style::Pill => "pill",
            Style::Frame => "frame",
            Style::Notch => "notch",
            Style::Bar => "bar",
            Style::Split => "split",
            Style::Minimal => "minimal",
        }
    }

    pub fn parse(name: &str) -> Option<Style> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    pub fn next(self) -> Style {
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Part of the screen's edge, so it doesn't float, drag or cast a shadow.
    pub fn attached(self) -> bool {
        matches!(self, Style::Frame | Style::Notch | Style::Bar)
    }

    pub fn insets(self) -> Insets {
        match self {
            Style::Frame => Insets { left: RAIL, top: BORDER, right: BORDER, bottom: BORDER },
            Style::Bar => Insets { top: BAR, ..Insets::default() },
            _ => Insets::default(),
        }
    }

    /// The island's top when it's home: under the frame's border, inside the bar.
    pub fn dock_y(self) -> f64 {
        match self {
            Style::Frame => BORDER,
            Style::Notch | Style::Bar => 0.0,
            _ => 6.0,
        }
    }

    /// Whether a pill module shows on the island itself. The frame's rail and the
    /// bar's ends already carry some, and minimal keeps only what matters now.
    pub fn on_island(self, module: &str) -> bool {
        match self {
            Style::Frame => !matches!(module, "workspaces" | "clock" | "network" | "battery"),
            Style::Bar => !matches!(module, "workspaces" | "network" | "battery"),
            Style::Minimal => matches!(module, "claude" | "timer" | "privacy" | "clock"),
            _ => true,
        }
    }

    /// Whether a module buds off into the split style's bubble.
    pub fn in_bubble(self, module: &str) -> bool {
        self == Style::Split && matches!(module, "media" | "timer" | "claude")
    }
}

/// How far a shape has to reach out of its edge for full corners.
fn corners(out: f64, w: f64, fillet: f64, r: f64) -> (f64, f64) {
    // Short of room, both shrink together, so it swells out of the edge as one curve.
    let k = (out / (fillet + r).max(1e-6)).clamp(0.0, 1.0);
    (fillet * k, (r * k).min(w / 2.0).max(0.0))
}

/// A shape hanging from a band along y = 0 that's `band` thick: the piece of band
/// above it, reversed corners where it leaves the band, round ones at its far end.
/// `span` is its two sides and `end` how far down it reaches.
/// Other edges are this same path under a rotated or mirrored matrix.
pub fn tab(cr: &Context, span: (f64, f64), band: f64, end: f64, fillet: f64, r: f64) {
    let (x0, x1) = span;
    let out = (end - band).max(0.0);
    let (f, r) = corners(out, x1 - x0, fillet, r);
    let end = band + out;
    let (c, d) = (f * 0.36, r * 0.36);
    cr.new_path();
    cr.move_to(x0 - f, 0.0);
    cr.line_to(x1 + f, 0.0);
    cr.line_to(x1 + f, band);
    cr.curve_to(x1 + c, band, x1, band + c, x1, band + f);
    cr.line_to(x1, end - r);
    cr.curve_to(x1, end - d, x1 - d, end, x1 - r, end);
    cr.line_to(x0 + r, end);
    cr.curve_to(x0 + d, end, x0, end - d, x0, end - r);
    cr.line_to(x0, band + f);
    cr.curve_to(x0, band + c, x0 - c, band, x0 - f, band);
    cr.close_path();
}

/// The border: the whole screen, less a rounded hole for the windows.
pub fn frame(cr: &Context, (w, h): (f64, f64), i: Insets, r: f64) {
    cr.new_path();
    cr.rectangle(0.0, 0.0, w, h);
    let (x0, y0, x1, y1) = (i.left, i.top, w - i.right, h - i.bottom);
    let c = r * 0.36;
    cr.move_to(x0 + r, y0);
    cr.line_to(x1 - r, y0);
    cr.curve_to(x1 - c, y0, x1, y0 + c, x1, y0 + r);
    cr.line_to(x1, y1 - r);
    cr.curve_to(x1, y1 - c, x1 - c, y1, x1 - r, y1);
    cr.line_to(x0 + r, y1);
    cr.curve_to(x0 + c, y1, x0, y1 - c, x0, y1 - r);
    cr.line_to(x0, y0 + r);
    cr.curve_to(x0, y0 + c, x0 + c, y0, x0 + r, y0);
    cr.close_path();
    cr.set_fill_rule(cairo::FillRule::EvenOdd);
}

/// The drop's waist between the pill and its bubble while they're still parting.
/// False, with no path, once they've come apart.
pub fn neck(cr: &Context, main: Rect, bubble_x: f64) -> bool {
    let (x, y, w, h) = main;
    let gap = bubble_x - (x + w);
    if gap >= SPLIT_GAP * 0.85 {
        return false;
    }
    let pinch = (gap.max(0.0) / (SPLIT_GAP * 0.85)) * h / 2.0;
    let (a, b) = (x + w - h / 2.0, bubble_x + h / 2.0);
    let mid = (a + b) / 2.0;
    cr.new_path();
    cr.move_to(a, y);
    cr.curve_to(mid, y + pinch, mid, y + pinch, b, y);
    cr.line_to(b, y + h);
    cr.curve_to(mid, y + h - pinch, mid, y + h - pinch, a, y + h);
    cr.close_path();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairo::{Format, ImageSurface};

    fn canvas() -> Context {
        Context::new(&ImageSurface::create(Format::ARgb32, 8, 8).unwrap()).unwrap()
    }

    fn inside(cr: &Context, x: f64, y: f64) -> bool {
        cr.in_fill(x, y).unwrap()
    }

    #[test]
    fn a_tab_flows_out_of_its_band_without_a_step() {
        let cr = canvas();
        tab(&cr, (100.0, 300.0), 8.0, 120.0, 18.0, 24.0);
        assert!(inside(&cr, 200.0, 4.0) && inside(&cr, 200.0, 110.0), "band and body are one shape");
        assert!(inside(&cr, 98.0, 9.0) && inside(&cr, 302.0, 9.0), "reversed corners fill outside the sides");
        assert!(!inside(&cr, 88.0, 22.0) && !inside(&cr, 312.0, 22.0), "and curve away");
        assert!(!inside(&cr, 101.0, 119.0), "far corners are round");
    }

    #[test]
    fn a_tab_thins_away_to_nothing_but_its_band() {
        let cr = canvas();
        for end in [8.0, 8.4, 12.0, 30.0, 400.0] {
            tab(&cr, (100.0, 300.0), 8.0, end, 18.0, 24.0);
            let (x0, y0, x1, y1) = cr.path_extents().unwrap();
            assert!(y0 == 0.0 && (y1 - end).abs() < 0.01, "reaches exactly {end}: {y0} {y1}"); // cairo counts in 256ths
            assert!(x0 >= 100.0 - 18.0 && x1 <= 300.0 + 18.0, "never wider than its fillets at {end}");
        }
    }

    #[test]
    fn corners_never_overlap_when_short_of_room() {
        for out in [0.0, 1.0, 10.0, 41.9, 42.0, 500.0] {
            let (f, r) = corners(out, 200.0, 18.0, 24.0);
            assert!(f + r <= out + 1e-9, "{out}");
        }
        assert_eq!(corners(500.0, 20.0, 18.0, 24.0), (18.0, 10.0), "a narrow one keeps half its width");
    }

    #[test]
    fn the_frame_is_a_border_with_rounded_inner_corners() {
        let cr = canvas();
        let i = Style::Frame.insets();
        frame(&cr, (1920.0, 1080.0), i, FRAME_R);
        assert!(inside(&cr, 20.0, 500.0) && inside(&cr, 900.0, 4.0) && inside(&cr, 1916.0, 500.0) && inside(&cr, 900.0, 1076.0));
        assert!(!inside(&cr, 900.0, 500.0), "windows show through");
        assert!(inside(&cr, i.left + 1.0, i.top + 1.0), "the corner is filled in");
        assert!(!inside(&cr, i.left + FRAME_R, i.top + 1.0));
    }

    #[test]
    fn a_drop_pinches_then_parts() {
        let cr = canvas();
        assert!(neck(&cr, (0.0, 0.0, 100.0, 32.0), 98.0) && inside(&cr, 99.0, 2.0), "still one drop");
        assert!(neck(&cr, (0.0, 0.0, 100.0, 32.0), 104.0) && !inside(&cr, 102.0, 1.0), "pinched");
        assert!(!neck(&cr, (0.0, 0.0, 100.0, 32.0), 100.0 + SPLIT_GAP), "apart");
    }

    #[test]
    fn styles_round_trip_and_only_two_reserve_space() {
        for s in Style::ALL {
            assert_eq!(Style::parse(s.name()), Some(s));
            assert_eq!(s.insets() != Insets::default(), matches!(s, Style::Frame | Style::Bar), "{s:?}");
        }
        assert_eq!(Style::parse("blob"), None);
        assert_eq!(Style::ALL.iter().fold(Style::Pill, |s, _| s.next()), Style::Pill);
    }
}
