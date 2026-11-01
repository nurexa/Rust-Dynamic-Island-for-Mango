//! Time and date.
use super::{Ctx, PILL_H};
use crate::pen::Pen;
use pango::Weight;

/// strftime in local time, without the leading zero of a 12-hour clock.
pub fn format(fmt: &str) -> String {
    let s = pango::glib::DateTime::now_local().and_then(|d| d.format(fmt)).map(|s| s.to_string());
    s.unwrap_or_default().trim_start_matches('0').to_owned()
}

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let w = p.text(&format(&c.cfg.clock), 13.0, Weight::Semibold, p.pal.fg, x, PILL_H / 2.0, -1.0);
    x + w
}
