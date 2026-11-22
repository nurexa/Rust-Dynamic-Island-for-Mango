//! Time and date.
use super::{Ctx, PILL_H};
use crate::pen::Pen;
use pango::Weight;

/// strftime in local time, without the leading zero of a 12-hour clock.
pub fn format(fmt: &str) -> String {
    let s = pango::glib::DateTime::now_local().and_then(|d| d.format(fmt)).map(|s| s.to_string());
    s.unwrap_or_default().trim_start_matches('0').to_owned()
}

pub fn twelve_hour(fmt: &str) -> bool {
    ["%I", "%l", "%r"].iter().any(|f| fmt.contains(f))
}

/// Hours and minutes for the rail, 12 or 24 hour as the clock's own format is.
pub fn stacked(fmt: &str) -> (String, String) {
    let twelve = twelve_hour(fmt);
    let Ok(now) = pango::glib::DateTime::now_local() else { return Default::default() };
    let part = |f: &str| now.format(f).map(|s| s.to_string()).unwrap_or_default();
    (part(if twelve { "%I" } else { "%H" }), part("%M"))
}

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let w = p.text(&format(&c.cfg.clock), 13.0, Weight::Semibold, p.pal.fg, x, PILL_H / 2.0, -1.0);
    x + w
}
