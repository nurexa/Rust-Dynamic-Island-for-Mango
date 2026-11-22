//! Music visualizers. `parec` (it ships with the `pactl` the island already uses)
//! records the output's monitor only while music plays and a visualizer is on
//! screen; its thread turns that into levels. Drawing is an overlay on cached
//! content, so a frame repaints the visualizer's rectangle and nothing else.
use crate::{Msg, pen::Rect, theme::Rgb};
use cairo::Context;
use serde::Deserialize;
use std::{
    f32::consts::PI,
    io::Read,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    thread,
};

pub const BANDS: usize = 24;
pub type Levels = [f32; BANDS];

const RATE: usize = 24_000;
const WINDOW: usize = 1024;
/// 30 frames a second: plenty for bars, half the work of 60.
const HOP: usize = RATE / 30;
const LOW_HZ: f32 = 50.0;
const HIGH_HZ: f32 = 10_000.0;
/// Below this, a window is silence.
const SILENT: f32 = 1e-4;
/// How far under the recent loudest band still shows.
const RANGE_DB: f32 = 30.0;
/// The auto gain doesn't chase anything quieter than this.
const FLOOR_DB: f32 = -50.0;

#[derive(serde::Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Look {
    /// A few bars on the pill, by the album art.
    Bars,
    /// A filled wave behind the player.
    Wave,
    /// Spokes round circular album art.
    Ring,
}

/// Somewhere content leaves for levels to be drawn, in its own coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Live {
    pub look: Look,
    pub rect: Rect,
}

/// Bass to treble, spread out on a log scale so each band is about as busy.
fn band_edges() -> [usize; BANDS + 1] {
    let bin = RATE as f32 / WINDOW as f32;
    let mut edges = [0; BANDS + 1];
    for (i, e) in edges.iter_mut().enumerate() {
        *e = (LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f32 / BANDS as f32) / bin).round() as usize;
    }
    // The lowest bands are narrower than a bin: give each one of its own.
    for i in 1..=BANDS {
        edges[i] = edges[i].max(edges[i - 1] + 1).min(WINDOW / 2);
    }
    edges
}

/// In-place radix-2. `re` and `im` are a power of two long.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = -2.0 * PI / len as f32;
        let (wr, wi) = (step.cos(), step.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in start..start + len / 2 {
                let o = k + len / 2;
                let (tr, ti) = (re[o] * cr - im[o] * ci, re[o] * ci + im[o] * cr);
                (re[o], im[o]) = (re[k] - tr, im[k] - ti);
                (re[k], im[k]) = (re[k] + tr, im[k] + ti);
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len *= 2;
    }
}

struct Analyser {
    samples: [f32; WINDOW],
    hann: [f32; WINDOW],
    edges: [usize; BANDS + 1],
    levels: Levels,
    top_db: f32,
}

impl Analyser {
    fn new() -> Self {
        let hann = std::array::from_fn(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / WINDOW as f32).cos());
        Analyser { samples: [0.0; WINDOW], hann, edges: band_edges(), levels: [0.0; BANDS], top_db: FLOOR_DB }
    }

    /// Take in one hop. None while it's silent and has already fallen flat.
    fn push(&mut self, hop: &[f32]) -> Option<Levels> {
        let n = hop.len().min(WINDOW);
        self.samples.copy_within(n.., 0);
        for (s, v) in self.samples[WINDOW - n..].iter_mut().zip(&hop[hop.len() - n..]) {
            *s = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
        }
        let mut target = [0.0f32; BANDS];
        if self.samples.iter().any(|s| s.abs() > SILENT) {
            let mut re: [f32; WINDOW] = std::array::from_fn(|i| self.samples[i] * self.hann[i]);
            let mut im = [0.0; WINDOW];
            fft(&mut re, &mut im);
            let mut db = [0.0f32; BANDS];
            for (b, out) in db.iter_mut().enumerate() {
                let bins = self.edges[b]..self.edges[b + 1];
                let power = bins.map(|k| re[k] * re[k] + im[k] * im[k]).fold(0.0f32, f32::max);
                // Music falls off with pitch; tilt it back so the treble moves too.
                *out = 10.0 * (power.max(1e-12) / (WINDOW * WINDOW) as f32).log10() + b as f32 * 0.6;
            }
            let loudest = db.iter().copied().fold(f32::MIN, f32::max);
            self.top_db = loudest.max(self.top_db - 0.15).max(FLOOR_DB);
            for (t, d) in target.iter_mut().zip(db) {
                *t = ((d - (self.top_db - RANGE_DB)) / RANGE_DB).clamp(0.0, 1.0);
            }
        } else if self.levels.iter().all(|l| *l == 0.0) {
            return None;
        }
        for (l, t) in self.levels.iter_mut().zip(target) {
            *l += (t - *l) * if t > *l { 0.65 } else { 0.28 };
            if *l < 0.01 && t == 0.0 {
                *l = 0.0;
            }
        }
        Some(self.levels)
    }
}

/// A running capture. Dropping it ends the child, and with it the thread.
pub struct Capture {
    child: Child,
}

impl Capture {
    pub fn start(tx: crate::Tx) -> Option<Self> {
        let mut cmd = Command::new("parec");
        cmd.args(["--device=@DEFAULT_MONITOR@", "--raw", "--format=float32le", "--channels=1", "--latency-msec=33",
                  "--client-name=Island visualizer"])
            .arg(format!("--rate={RATE}"))
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        // Die with the island, however it goes.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = cmd.spawn().ok()?;
        let mut out = child.stdout.take()?;
        thread::spawn(move || {
            let mut analyser = Analyser::new();
            let mut raw = [0u8; HOP * 4];
            let mut hop = [0.0f32; HOP];
            while out.read_exact(&mut raw).is_ok() {
                for (s, b) in hop.iter_mut().zip(raw.chunks_exact(4)) {
                    *s = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                }
                if let Some(levels) = analyser.push(&hop) {
                    if tx.send(Msg::Levels(levels)).is_err() {
                        break;
                    }
                }
            }
        });
        Some(Capture { child })
    }

    /// The audio server went, or wasn't there.
    pub fn ended(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The mean of one of `n` equal groups of bands.
fn group(levels: &Levels, i: usize, n: usize) -> f64 {
    let (a, b) = (i * BANDS / n, ((i + 1) * BANDS / n).max(i * BANDS / n + 1));
    levels[a..b].iter().sum::<f32>() as f64 / (b - a) as f64
}

impl Look {
    /// The wave goes behind the title and the art; the others sit in their own space.
    pub fn under(self) -> bool {
        self == Look::Wave
    }
}

/// Neighbours averaged in, so a lone note is a swell rather than a spike.
fn smoothed(levels: &Levels) -> Levels {
    let mut out = *levels;
    for _ in 0..3 {
        let from = out;
        for (i, v) in out.iter_mut().enumerate() {
            let (a, b) = (from[i.saturating_sub(1)], from[(i + 1).min(BANDS - 1)]);
            *v = 0.3 * a + 0.4 * from[i] + 0.3 * b;
        }
    }
    out
}

pub fn paint(cr: &Context, live: &Live, origin: (f64, f64), levels: &Levels, color: Rgb, alpha: f64) {
    let (x, y, w, h) = live.rect;
    let (x, y) = (x + origin.0, y + origin.1);
    let (r, g, b) = color;
    match live.look {
        Look::Bars => {
            // Bass in the middle, the way it reads as one thing pulsing.
            let order = [3, 1, 0, 2, 4];
            let bar = w / (order.len() as f64 * 1.6 - 0.6);
            cr.set_line_width(bar);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_source_rgba(r, g, b, alpha);
            for (i, &band) in order.iter().enumerate() {
                let cx = x + bar / 2.0 + i as f64 * bar * 1.6;
                let half = ((h - bar) * group(levels, band, order.len()) / 2.0).max(0.01);
                cr.move_to(cx, y + h / 2.0 - half);
                cr.line_to(cx, y + h / 2.0 + half);
            }
            let _ = cr.stroke();
        }
        Look::Wave => {
            let _ = cr.save();
            crate::pen::rounded_rect(cr, x, y, w, h, 16.0);
            cr.clip();
            let levels = smoothed(levels);
            let point = |i: usize| (x + w * i as f64 / (BANDS - 1) as f64, y + h - h * 0.92 * levels[i] as f64);
            cr.move_to(x, y + h);
            let first = point(0);
            cr.line_to(first.0, first.1);
            let mut at = first;
            for i in 1..BANDS {
                let to = point(i);
                let mid = (at.0 + to.0) / 2.0;
                cr.curve_to(mid, at.1, mid, to.1, to.0, to.1);
                at = to;
            }
            cr.line_to(x + w, y + h);
            cr.close_path();
            // Faint, because the title sits on it.
            cr.set_source_rgba(r, g, b, 0.15 * alpha);
            let _ = cr.fill();
            let _ = cr.restore();
        }
        Look::Ring => {
            let (cx, cy, outer) = (x + w / 2.0, y + h / 2.0, w.min(h) / 2.0);
            let inner = outer * RING_INNER;
            let spokes = BANDS * 2;
            let width = (inner * std::f64::consts::TAU / spokes as f64 * 0.5).max(1.5);
            cr.set_line_width(width);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_source_rgba(r, g, b, alpha);
            for i in 0..spokes {
                // The right half is the left one mirrored, bass at the bottom.
                let band = if i < BANDS { i } else { spokes - 1 - i };
                let a = std::f64::consts::FRAC_PI_2 + (i as f64 + 0.5) / spokes as f64 * std::f64::consts::TAU;
                let start = inner + width / 2.0;
                let end = start + (outer - start - width / 2.0).max(0.0) * levels[band] as f64 + 0.01;
                cr.move_to(cx + a.cos() * start, cy + a.sin() * start);
                cr.line_to(cx + a.cos() * end, cy + a.sin() * end);
            }
            let _ = cr.stroke();
        }
    }
}

/// How much of a ring's box is left for the art.
pub const RING_INNER: f64 = 0.8;

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, from: usize) -> Vec<f32> {
        (from..from + HOP).map(|i| 0.3 * (2.0 * PI * hz * i as f32 / RATE as f32).sin()).collect()
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        for hz in [80.0, 440.0, 1000.0, 6000.0] {
            let mut a = Analyser::new();
            let mut levels = [0.0; BANDS];
            for i in 0..12 {
                levels = a.push(&tone(hz, i * HOP)).expect("not silent");
            }
            let peak = (0..BANDS).max_by(|&x, &y| levels[x].total_cmp(&levels[y])).unwrap();
            let want = ((hz / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln() * BANDS as f32) as usize;
            assert!(peak.abs_diff(want) <= 3, "{hz} Hz lit band {peak}, not {want}");
            assert!(levels[peak] > 0.6, "{hz} Hz only reached {}", levels[peak]);
        }
    }

    #[test]
    fn silence_falls_flat_then_stops_sending() {
        let mut a = Analyser::new();
        a.push(&tone(440.0, 0));
        let mut sent = 0;
        while a.push(&[0.0; HOP]).is_some() {
            sent += 1;
            assert!(sent < 60, "has to reach zero, or it redraws forever");
        }
        assert!(a.levels.iter().all(|l| *l == 0.0));
        assert_eq!(Analyser::new().push(&[0.0; HOP]), None, "silence from the start sends nothing");
        Analyser::new().push(&[f32::NAN; HOP]); // junk from the pipe doesn't panic
    }

    #[test]
    fn bands_are_distinct_and_in_range() {
        let e = band_edges();
        assert!(e.windows(2).all(|w| w[0] < w[1]) && e[0] > 0 && e[BANDS] <= WINDOW / 2, "{e:?}");
    }
}
