//! Two scales meet at a surface: the monitor's, and the island's own. Everything
//! lays out and hit-tests in the island's units. Only what talks to Wayland converts.

pub const MIN: f64 = 0.75;
pub const MAX: f64 = 2.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scale {
    /// Buffer pixels per Wayland pixel: the monitor's.
    pub buffer: i32,
    /// Wayland pixels per island unit: the user's.
    pub ui: f64,
}

impl Scale {
    /// Buffer pixels per island unit: what drawing happens at.
    pub fn px(self) -> f64 {
        f64::from(self.buffer) * self.ui
    }

    /// Wayland pixels.
    pub fn wl(self, v: f64) -> i32 {
        (v * self.ui).round() as i32
    }

    /// A surface's size in Wayland pixels. Zero would mean "you choose".
    pub fn size(self, (w, h): (f64, f64)) -> (i32, i32) {
        (self.wl(w).max(1), self.wl(h).max(1))
    }

    pub fn units(self, (x, y): (f64, f64)) -> (f64, f64) {
        (x / self.ui, y / self.ui)
    }

    /// A rectangle on whole Wayland pixels, so neighbouring surfaces share their seams.
    pub fn snap(self, (x, y, w, h): crate::pen::Rect) -> crate::pen::Rect {
        let on = |v: f64| f64::from(self.wl(v)) / self.ui;
        (on(x), on(y), on(x + w) - on(x), on(y + h) - on(y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_survives_the_trip_through_units() {
        for ui in [MIN, 0.9, 1.0, 1.1, 1.25, 1.5, MAX] {
            let s = Scale { buffer: 2, ui };
            assert_eq!(s.size(s.units((1920.0, 1080.0))), (1920, 1080), "{ui}");
            let out = s.units((1920.0, 1080.0));
            let top = s.snap((0.0, 0.0, out.0, 28.0));
            let side = s.snap((0.0, 28.0, 40.0, out.1 - 56.0));
            assert_eq!(s.wl(top.1 + top.3), s.wl(side.1), "no seam at {ui}");
        }
        assert_eq!(Scale { buffer: 2, ui: 1.25 }.px(), 2.5);
    }
}
