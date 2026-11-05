//! The pill's privacy dots: orange while an app records a microphone, green
//! while one has the camera. Nothing shows otherwise.
use super::{Ctx, PILL_H};
use crate::pen::{Action, Pen, Region};

const MIC: (f64, f64, f64) = (1.0, 0.62, 0.04);
const CAM: (f64, f64, f64) = (0.2, 0.82, 0.35);

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let mic = c.m.audio.as_ref().map_or(&[][..], |a| &a.recording[..]);
    let cam = &c.m.camera[..];
    if mic.is_empty() && cam.is_empty() {
        return x;
    }
    let cy = PILL_H / 2.0;
    let mut dx = x;
    for (on, color) in [(!cam.is_empty(), CAM), (!mic.is_empty(), MIC)] {
        if on {
            p.fill((dx, cy - 4.0, 8.0, 8.0), 4.0, color, 1.0);
            dx += 12.0;
        }
    }
    let w = dx - 4.0 - x;
    p.regions.push(Region::new((x - 4.0, 6.0, w + 8.0, PILL_H - 12.0), 8.0, Action::None).tip(crate::privacy::describe(mic, cam)));
    x + w
}
