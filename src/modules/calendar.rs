//! A month, with a dot where there are events and what's on the chosen day.
use super::{Ctx, PAD};
use crate::{
    calendar::{self, CELLS, Occurrence},
    pen::{Action, Pen, Region},
};
use pango::Weight;

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September",
                            "October", "November", "December"];
const ROW: f64 = 30.0;
const AGENDA: usize = 4;

fn time_of(minute: Option<i32>, twelve: bool) -> String {
    match minute {
        None => "All day".into(),
        Some(m) if twelve => format!("{}:{:02} {}", (m / 60 + 11) % 12 + 1, m % 60, if m < 720 { "AM" } else { "PM" }),
        Some(m) => format!("{:02}:{:02}", m / 60, m % 60),
    }
}

/// A chosen day's events, or with none chosen, what's left of today and then what's next.
fn agenda(events: &[Occurrence], selected: Option<i64>, (today, minute): (i64, i32)) -> Vec<&Occurrence> {
    events.iter().filter(|e| match selected {
        Some(day) => e.day == day,
        None => e.day > today || (e.day == today && e.minute.is_none_or(|m| m >= minute)),
    }).take(AGENDA).collect()
}

pub fn panel(p: &mut Pen, c: &Ctx, mut y: f64) -> f64 {
    let pal = p.pal;
    let cal = &c.m.calendar;
    let first = c.cfg.calendar.first_weekday();
    let now @ (today, _) = calendar::now();
    let (year, month) = calendar::month(today, cal.offset);
    let (left, right) = (PAD, p.w - PAD);

    let cy = y + 15.0;
    let title = format!("{} {year}", MONTHS[month as usize - 1]);
    let w = p.text(&title, 14.0, Weight::Semibold, pal.fg, left, cy, -1.0);
    p.regions.push(Region::new((left - 6.0, y, w + 12.0, 30.0), 10.0, Action::CalMonth(0)).tip("This month"));
    for (i, (icon, by)) in [("chevron_right", 1), ("chevron_left", -1)].into_iter().enumerate() {
        let r = (right - 26.0 - i as f64 * 30.0, y + 1.0, 28.0, 28.0);
        p.button(icon, r, false, Region::new(r, 14.0, Action::CalMonth(by)));
    }
    y += 36.0;

    let cell = (right - left) / 7.0;
    for i in 0..7 {
        let name = DAYS[((first + i) % 7) as usize];
        let w = p.width(name, 11.0, Weight::Semibold);
        p.text(name, 11.0, Weight::Semibold, pal.muted, left + (i as f64 + 0.5) * cell - w / 2.0, y + 8.0, -1.0);
    }
    y += 22.0;

    let start = calendar::grid_start((year, month), first);
    for i in 0..CELLS {
        let day = start + i;
        let (_, m, d) = calendar::civil_from_days(day);
        let (cx, cy) = (left + ((i % 7) as f64 + 0.5) * cell, y + ((i / 7) as f64 + 0.5) * ROW);
        let circle = (cx - 13.0, cy - 13.0, 26.0, 26.0);
        let is_today = day == today;
        if is_today {
            p.fill(circle, 13.0, pal.accent, 1.0);
        } else if cal.selected == Some(day) {
            p.fill(circle, 13.0, pal.accent, 0.24);
        }
        let (color, weight) = match () {
            _ if is_today => (pal.on_accent, Weight::Bold),
            _ if m == month => (pal.fg, Weight::Medium),
            _ => (pal.muted, Weight::Normal),
        };
        let label = d.to_string();
        let w = p.width(&label, 12.0, weight);
        p.text(&label, 12.0, weight, color, cx - w / 2.0, cy, -1.0);
        if cal.events.iter().any(|e| e.day == day) {
            p.fill((cx - 2.0, cy + 7.5, 4.0, 4.0), 2.0, if is_today { pal.on_accent } else { pal.accent }, 1.0);
        }
        p.regions.push(Region::new(circle, 13.0, Action::CalDay(day)));
    }
    y += (CELLS / 7) as f64 * ROW + 6.0;

    if !c.cfg.calendar.dir.is_empty() {
        let twelve = crate::modules::clock::twelve_hour(&c.cfg.clock);
        let shown = agenda(&cal.events, cal.selected, now);
        if shown.is_empty() {
            let text = if cal.selected.is_some() { "Nothing that day" } else { "Nothing coming up" };
            p.text(text, 12.0, Weight::Normal, pal.muted, left, y + 12.0, -1.0);
            y += 26.0;
        }
        for e in shown {
            let cy = y + 13.0;
            let mut when = time_of(e.minute, twelve);
            if cal.selected.is_none() && e.day != today {
                let (_, _, d) = calendar::civil_from_days(e.day);
                when = format!("{} {d}, {when}", DAYS[calendar::weekday(e.day) as usize]);
            }
            p.fill((left, cy - 7.0, 3.0, 14.0), 1.5, pal.accent, 1.0);
            let w = p.right(&when, 11.5, Weight::Medium, pal.muted, right, cy);
            p.text(&e.title, 12.0, Weight::Medium, pal.fg, left + 11.0, cy, right - w - 8.0 - left - 11.0);
            y += 26.0;
        }
    }
    y + 12.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_whats_next() {
        assert_eq!([time_of(Some(0), true), time_of(Some(785), true), time_of(Some(785), false)], ["12:00 AM", "1:05 PM", "13:05"]);
        let e = |day, minute| Occurrence { day, minute, title: "x".into() };
        let all = [e(9, None), e(10, None), e(10, Some(60)), e(10, Some(600)), e(11, None)];
        assert_eq!(agenda(&all, None, (10, 120)), [&all[1], &all[3], &all[4]], "what's over is left out");
        assert_eq!(agenda(&all, Some(9), (10, 120)), [&all[0]]);
    }
}
