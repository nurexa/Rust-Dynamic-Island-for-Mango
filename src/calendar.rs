//! The optional calendar: date arithmetic, and events from a folder of .ics files,
//! which is what vdirsyncer, khal and every "export calendar" button write. Files
//! are read when the calendar is shown and never watched; nothing here touches
//! the network.
//!
//! ponytail: the recurrences calendars really write (daily to yearly, INTERVAL,
//! COUNT, UNTIL, BYDAY, BYMONTHDAY, BYMONTH, BYSETPOS, EXDATE, moved instances).
//! No hourly rules, RDATE or BYWEEKNO; take an RRULE crate if someone needs those.
use glib::{DateTime, TimeZone};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 5000;
const MAX_FILE: u64 = 16 << 20;
const MAX_DEPTH: u32 = 4;
const MAX_STEPS: i64 = 20_000;
const MAX_SHOWN: usize = 4000;
const MAX_SPAN: i64 = 62;
/// A month is shown as six weeks, so it never changes height.
pub const CELLS: i64 = 42;

/// One thing on one day, in local time.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Occurrence {
    /// Days since 1970-01-01.
    pub day: i64,
    /// Minutes past midnight. None is all day.
    pub minute: Option<i32>,
    pub title: String,
}

/// What the calendar is showing.
#[derive(Debug, Default)]
pub struct View {
    /// Months away from this one.
    pub offset: i32,
    pub selected: Option<i64>,
    pub events: Vec<Occurrence>,
}

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

pub fn civil_from_days(day: i64) -> (i64, i64, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, doy - (153 * mp + 2) / 5 + 1)
}

/// Monday is 0.
pub fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

fn month_len(y: i64, m: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)
}

fn shift_month(y: i64, m: i64, by: i64) -> (i64, i64) {
    let n = y * 12 + m - 1 + by;
    (n.div_euclid(12), n.rem_euclid(12) + 1)
}

fn split(t: &DateTime) -> (i64, i32) {
    (days_from_civil(t.year() as i64, t.month() as i64, t.day_of_month() as i64), t.hour() * 60 + t.minute())
}

/// Today, and the minute it is.
pub fn now() -> (i64, i32) {
    DateTime::now_local().as_ref().map(split).unwrap_or_default()
}

/// The month `offset` months from today's.
pub fn month(today: i64, offset: i32) -> (i64, i64) {
    let (y, m, _) = civil_from_days(today);
    shift_month(y, m, offset as i64)
}

/// The first day in the six-week grid. `first` is the weekday weeks start on.
pub fn grid_start((y, m): (i64, i64), first: i64) -> i64 {
    let day = days_from_civil(y, m, 1);
    day - (weekday(day) - first).rem_euclid(7)
}

/// What's worth loading: the month on show, and two weeks from today for what's next.
pub fn range(today: i64, offset: i32, first: i64) -> (i64, i64) {
    let start = grid_start(month(today, offset), first);
    (start.min(today), (start + CELLS).max(today + 14))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug)]
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<i64>,
    until: Option<Stamp>,
    /// Which of them in the month (0 is every), and the weekday.
    by_day: Vec<(i64, i64)>,
    by_month_day: Vec<i64>,
    by_month: Vec<i64>,
    set_pos: Vec<i64>,
    week_start: i64,
}

/// A wall-clock moment, and whose wall. No zone is this machine's.
#[derive(Clone, Debug)]
struct Stamp {
    day: i64,
    minute: Option<i32>,
    zone: Option<TimeZone>,
}

impl Stamp {
    fn time(&self, local: &TimeZone) -> Option<DateTime> {
        let (y, m, d) = civil_from_days(self.day);
        let min = self.minute.unwrap_or(0);
        DateTime::new(self.zone.as_ref().unwrap_or(local), y as i32, m as i32, d as i32, min / 60, min % 60, 0.0).ok()
    }

    /// Where it falls here.
    fn localise(&self, local: &TimeZone) -> (i64, Option<i32>) {
        if self.minute.is_none() || self.zone.is_none() {
            return (self.day, self.minute); // dates, and floating times, are the same everywhere
        }
        match self.time(local).and_then(|t| t.to_timezone(local).ok()) {
            Some(t) => {
                let (day, minute) = split(&t);
                (day, Some(minute))
            }
            None => (self.day, self.minute),
        }
    }

    /// The same moment, or, if either side is only a date, the same day.
    fn same(&self, other: &Stamp, local: &TimeZone) -> bool {
        if self.minute.is_none() || other.minute.is_none() {
            return self.day == other.day;
        }
        // Two days out, they can't be the same moment in any zone.
        (self.day - other.day).abs() <= 2 && self.time(local).map(|t| t.to_unix()) == other.time(local).map(|t| t.to_unix())
    }
}

#[derive(Debug)]
struct Event {
    uid: String,
    title: String,
    start: Stamp,
    /// Days it goes on past the first.
    span: i64,
    rule: Option<Rule>,
    except: Vec<Stamp>,
    /// The instance of a recurring event this one stands in for.
    replaces: Option<Stamp>,
    cancelled: bool,
}

/// `20261121`, `20261121T090000`, or the same with a Z for UTC.
fn stamp(value: &str, tzid: Option<&str>) -> Option<Stamp> {
    let (value, utc) = value.trim().strip_suffix('Z').map_or((value.trim(), false), |v| (v, true));
    let (date, time) = value.split_once('T').map_or((value, None), |(d, t)| (d, Some(t)));
    let num = |s: &str, r: std::ops::Range<usize>| -> Option<i64> {
        s.get(r).filter(|p| p.bytes().all(|b| b.is_ascii_digit()))?.parse().ok()
    };
    if date.len() != 8 {
        return None;
    }
    let (y, m, d) = (num(date, 0..4)?, num(date, 4..6)?, num(date, 6..8)?);
    if !(1900..=2200).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > month_len(y, m) {
        return None;
    }
    let minute = match time {
        Some(t) => {
            let (h, min) = (num(t, 0..2)?, num(t, 2..4)?);
            if h > 23 || min > 59 {
                return None;
            }
            Some((h * 60 + min) as i32)
        }
        None => None,
    };
    let zone = match (utc, tzid) {
        (true, _) => Some(TimeZone::utc()),
        (_, Some(id)) => zone(id),
        _ => None,
    };
    Some(Stamp { day: days_from_civil(y, m, d), minute, zone })
}

/// An IANA zone, also when it has a vendor's prefix in front. Others count as local.
fn zone(id: &str) -> Option<TimeZone> {
    let id = id.trim_matches('"');
    // GLib reads absolute paths as files, and an .ics doesn't get to pick files.
    if id.contains("..") || id.len() > 128 {
        return None;
    }
    let id = if id.starts_with('/') {
        let parts: Vec<&str> = id.rsplitn(3, '/').collect();
        format!("{}/{}", parts.get(1)?, parts.first()?)
    } else {
        id.to_owned()
    };
    TimeZone::from_identifier(Some(&id))
}

fn day_code(s: &str) -> Option<i64> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|d| *d == s).map(|i| i as i64)
}

fn rule(value: &str) -> Option<Rule> {
    let mut freq = None;
    let mut r = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, by_day: Vec::new(),
                       by_month_day: Vec::new(), by_month: Vec::new(), set_pos: Vec::new(), week_start: 0 };
    let numbers = |v: &str, max: i64| -> Vec<i64> {
        v.split(',').filter_map(|n| n.trim_start_matches('+').parse().ok()).filter(|n: &i64| *n != 0 && (-max..=max).contains(n)).collect()
    };
    for part in value.split(';') {
        let Some((key, v)) = part.split_once('=') else { continue };
        match key {
            "FREQ" => {
                freq = match v {
                    "DAILY" => Some(Freq::Daily),
                    "WEEKLY" => Some(Freq::Weekly),
                    "MONTHLY" => Some(Freq::Monthly),
                    "YEARLY" => Some(Freq::Yearly),
                    _ => return None,
                }
            }
            "INTERVAL" => r.interval = v.parse::<i64>().unwrap_or(1).clamp(1, 1000),
            "COUNT" => r.count = v.parse().ok(),
            "UNTIL" => r.until = stamp(v, None),
            "BYMONTHDAY" => r.by_month_day = numbers(v, 31),
            "BYMONTH" => r.by_month = numbers(v, 12).into_iter().filter(|m| *m > 0).collect(),
            "BYSETPOS" => r.set_pos = numbers(v, 366),
            "WKST" => r.week_start = day_code(v).unwrap_or(0),
            "BYDAY" => {
                r.by_day = v.split(',').filter_map(|d| {
                    let at = d.len().checked_sub(2)?;
                    let (nth, code) = (d.get(..at)?, d.get(at..)?);
                    let nth = if nth.is_empty() { 0 } else { nth.trim_start_matches('+').parse().ok().filter(|n: &i64| n.abs() <= 53)? };
                    Some((nth, day_code(code)?))
                }).collect();
            }
            _ => {}
        }
    }
    r.freq = freq?;
    Some(r)
}

impl Rule {
    /// The days this rule picks in one month.
    fn in_month(&self, y: i64, m: i64, first_day: i64, out: &mut Vec<i64>) {
        let (start, len) = (days_from_civil(y, m, 1), month_len(y, m));
        if !self.by_month_day.is_empty() {
            for &n in &self.by_month_day {
                let d = if n < 0 { len + n + 1 } else { n };
                let day = start + d - 1;
                if (1..=len).contains(&d) && (self.by_day.is_empty() || self.by_day.iter().any(|b| b.1 == weekday(day))) {
                    out.push(day);
                }
            }
        } else if !self.by_day.is_empty() {
            for &(nth, wd) in &self.by_day {
                let all: Vec<i64> = (start..start + len).filter(|d| weekday(*d) == wd).collect();
                let count = all.len() as i64;
                match nth {
                    0 => out.extend(&all),
                    n if n > 0 && n <= count => out.push(all[(n - 1) as usize]),
                    n if n < 0 && -n <= count => out.push(all[(count + n) as usize]),
                    _ => {}
                }
            }
        } else if first_day <= len {
            out.push(start + first_day - 1); // no 31st this month is no event this month
        }
    }

    /// The k-th period's days, in order.
    fn period(&self, start: i64, k: i64) -> Vec<i64> {
        let mut out = Vec::new();
        let n = k * self.interval;
        let (y, m, d) = civil_from_days(start);
        let month_wanted = |m: i64| self.by_month.is_empty() || self.by_month.contains(&m);
        match self.freq {
            Freq::Daily => {
                let day = start + n;
                if (self.by_day.is_empty() || self.by_day.iter().any(|b| b.1 == weekday(day))) && month_wanted(civil_from_days(day).1) {
                    out.push(day);
                }
            }
            Freq::Weekly => {
                let week = start - (weekday(start) - self.week_start).rem_euclid(7) + n * 7;
                let days = if self.by_day.is_empty() { vec![weekday(start)] } else { self.by_day.iter().map(|b| b.1).collect() };
                out.extend(days.iter().map(|wd| week + (wd - self.week_start).rem_euclid(7)).filter(|d| month_wanted(civil_from_days(*d).1)));
            }
            Freq::Monthly => {
                let (y, m) = shift_month(y, m, n);
                if month_wanted(m) {
                    self.in_month(y, m, d, &mut out);
                }
            }
            Freq::Yearly => {
                let months = if self.by_month.is_empty() { vec![m] } else { self.by_month.clone() };
                for month in months {
                    self.in_month(y + n, month, d, &mut out);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        if !self.set_pos.is_empty() {
            let len = out.len() as i64;
            let picked: Vec<i64> = self.set_pos.iter().map(|&p| if p < 0 { len + p } else { p - 1 })
                .filter(|i| (0..len).contains(i)).map(|i| out[i as usize]).collect();
            out = picked;
            out.sort_unstable();
        }
        out
    }

    /// Start days from `from` up to `to`, as the event's own zone counts them.
    fn days(&self, start: &Stamp, (from, to): (i64, i64), local: &TimeZone) -> Vec<i64> {
        let longest = match self.freq {
            Freq::Daily => 1,
            Freq::Weekly => 7,
            Freq::Monthly => 31,
            Freq::Yearly => 366,
        };
        // A count has to be counted from the start; anything else can skip ahead.
        let mut k = if self.count.is_some() { 0 } else { ((from - start.day) / (longest * self.interval) - 1).max(0) };
        let last = k + MAX_STEPS;
        let until = self.until.as_ref().map(|u| match u.minute {
            Some(_) => (u.day + 2, u.time(local).map(|t| t.to_unix())),
            None => (u.day, None),
        });
        let (mut out, mut seen) = (Vec::new(), 0);
        if k == 0 {
            // DTSTART is always the first, whatever the rule says.
            seen = 1;
            if start.day >= from && start.day < to {
                out.push(start.day);
            }
        }
        while k < last {
            let days = self.period(start.day, k);
            let begins = start.day + k * self.interval * match self.freq {
                Freq::Daily => 1,
                Freq::Weekly => 7,
                Freq::Monthly => 28,
                Freq::Yearly => 365,
            };
            for day in days {
                if day <= start.day {
                    continue;
                }
                if day >= to || self.count.is_some_and(|c| seen >= c) {
                    return out;
                }
                if let Some((day_limit, moment)) = until {
                    if day > day_limit {
                        return out;
                    }
                    if let Some(limit) = moment {
                        let at = Stamp { day, ..start.clone() };
                        if day + 4 >= day_limit && at.time(local).is_some_and(|t| t.to_unix() > limit) {
                            return out;
                        }
                    }
                }
                seen += 1;
                if day >= from {
                    out.push(day);
                }
            }
            // An empty period mustn't hide that the range is behind us.
            if begins > to + 366 {
                break;
            }
            k += 1;
        }
        out
    }
}

fn unfold(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in src.lines() {
        let line = line.trim_end_matches('\r');
        match (line.strip_prefix([' ', '\t']), out.last_mut()) {
            (Some(more), Some(last)) => last.push_str(more),
            _ => out.push(line.to_owned()),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        let c = if c == '\\' {
            match chars.next() {
                Some('n' | 'N') | None => ' ',
                Some(other) => other,
            }
        } else {
            c
        };
        out.push(if c.is_control() { ' ' } else { c });
        if out.len() >= 200 {
            break; // it's one line in a panel
        }
    }
    out.trim().to_owned()
}

/// The name, its parameters and its value. A colon inside quotes isn't the separator.
fn property(line: &str) -> Option<(&str, &str, &str)> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| {
        quoted ^= c == '"';
        (c == ':' && !quoted).then_some(i)
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let (name, params) = head.split_once(';').unwrap_or((head, ""));
    Some((name, params, value))
}

fn parse(src: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut lines: Option<Vec<String>> = None;
    let mut nested = 0;
    for line in unfold(src) {
        match line.as_str() {
            "BEGIN:VEVENT" => (lines, nested) = (Some(Vec::new()), 0),
            "END:VEVENT" => events.extend(lines.take().and_then(|l| event(&l))),
            // An alarm inside has its own SUMMARY and times.
            l if l.starts_with("BEGIN:") => nested += 1,
            l if l.starts_with("END:") => nested = (nested - 1).max(0),
            _ if nested == 0 => {
                if let Some(l) = &mut lines {
                    l.push(line);
                }
            }
            _ => {}
        }
    }
    events
}

fn event(lines: &[String]) -> Option<Event> {
    let (mut uid, mut title, mut start, mut end, mut length) = (String::new(), String::new(), None, None, None);
    let (mut repeat, mut except, mut replaces, mut cancelled) = (None, Vec::new(), None, false);
    for line in lines {
        let Some((name, params, value)) = property(line) else { continue };
        let tzid = params.split(';').find_map(|p| p.strip_prefix("TZID="));
        match name {
            "UID" => uid = value.to_owned(),
            "SUMMARY" => title = unescape(value),
            "DTSTART" => start = stamp(value, tzid),
            "DTEND" => end = stamp(value, tzid),
            "DURATION" => length = duration_days(value),
            "RRULE" => repeat = rule(value),
            "EXDATE" => except.extend(value.split(',').filter_map(|v| stamp(v, tzid))),
            "RECURRENCE-ID" => replaces = stamp(value, tzid),
            "STATUS" => cancelled = value == "CANCELLED",
            _ => {}
        }
    }
    let start: Stamp = start?;
    let span = match (&end, length) {
        (Some(e), _) => {
            // An end at midnight, or an all-day end, is the day after the last one.
            let before = e.minute.unwrap_or(0) == 0 && e.day > start.day;
            e.day - start.day - before as i64
        }
        (None, Some(days)) => days - 1,
        (None, None) => 0,
    };
    if title.is_empty() {
        title = "Untitled".into();
    }
    Some(Event { uid, title, start, span: span.clamp(0, MAX_SPAN), rule: repeat, except, replaces, cancelled })
}

/// Whole days of a DURATION; hours don't matter to a month view.
fn duration_days(v: &str) -> Option<i64> {
    let body = v.trim_start_matches('+').strip_prefix('P')?;
    let (n, unit) = body.split_at(body.find(['W', 'D'])?);
    Some(n.parse::<i64>().ok()?.min(MAX_SPAN) * if unit.starts_with('W') { 7 } else { 1 })
}

fn expand(events: &[Event], (from, to): (i64, i64), local: &TimeZone) -> Vec<Occurrence> {
    let moved: Vec<(&str, &Stamp)> = events.iter().filter_map(|e| Some((e.uid.as_str(), e.replaces.as_ref()?))).collect();
    let mut out = Vec::new();
    for e in events.iter().filter(|e| !e.cancelled) {
        // Two days of slack either side: a zone can move an event a day, and spans reach in.
        let window = (from - e.span - 2, to + 2);
        let days = match (&e.rule, &e.replaces) {
            (Some(rule), None) => rule.days(&e.start, window, local),
            _ if e.start.day >= window.0 && e.start.day < window.1 => vec![e.start.day],
            _ => continue,
        };
        for day in days {
            let at = Stamp { day, ..e.start.clone() };
            if e.rule.is_some() && e.replaces.is_none() {
                let excluded = e.except.iter().any(|x| at.same(x, local))
                    || moved.iter().any(|(uid, id)| *uid == e.uid && at.same(id, local));
                if excluded {
                    continue;
                }
            }
            let (first, minute) = at.localise(local);
            for i in 0..=e.span {
                if (from..to).contains(&(first + i)) {
                    out.push(Occurrence { day: first + i, minute: minute.filter(|_| i == 0), title: e.title.clone() });
                }
            }
            if out.len() >= MAX_SHOWN {
                break;
            }
        }
    }
    out.sort();
    out.dedup();
    out.truncate(MAX_SHOWN);
    out
}

fn ics_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else { continue };
        if meta.is_dir() && depth < MAX_DEPTH {
            ics_files(&path, depth + 1, out);
        } else if meta.is_file() && meta.len() <= MAX_FILE && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ics")) {
            out.push(path);
        }
    }
}

/// Every event in the folder that falls in the range. Slow, so off the UI thread.
pub fn load(dir: &Path, range: (i64, i64)) -> Vec<Occurrence> {
    let mut files = Vec::new();
    ics_files(dir, 0, &mut files);
    let mut events = Vec::new();
    for path in files {
        let mut bytes = Vec::new();
        if fs::File::open(&path).is_ok_and(|f| f.take(MAX_FILE).read_to_end(&mut bytes).is_ok()) {
            events.extend(parse(&String::from_utf8_lossy(&bytes)));
        }
    }
    expand(&events, range, &TimeZone::local())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i64, m: i64, d: i64) -> i64 {
        days_from_civil(y, m, d)
    }

    fn found(body: &str, from: i64, to: i64) -> Vec<(i64, Option<i32>)> {
        let src = format!("BEGIN:VCALENDAR\r\n{body}END:VCALENDAR\r\n");
        let local = TimeZone::from_identifier(Some("America/New_York")).expect("tzdata");
        expand(&parse(&src), (from, to), &local).into_iter().map(|o| (o.day, o.minute)).collect()
    }

    fn event(lines: &str) -> String {
        format!("BEGIN:VEVENT\r\nSUMMARY:x\r\n{lines}\r\nEND:VEVENT\r\n")
    }

    fn days(v: &[(i64, Option<i32>)]) -> Vec<i64> {
        v.iter().map(|(d, _)| civil_from_days(*d).2).collect()
    }

    #[test]
    fn dates_round_trip_and_the_grid_lines_up() {
        for n in (-40_000..90_000).step_by(17) {
            let (y, m, d) = civil_from_days(n);
            assert_eq!(days_from_civil(y, m, d), n);
        }
        assert_eq!((day(1970, 1, 1), weekday(0)), (0, 3), "a Thursday");
        assert_eq!(grid_start((2026, 11), 6), day(2026, 11, 1), "November 2026 starts on a Sunday");
        assert_eq!(grid_start((2026, 11), 0), day(2026, 10, 26));
        assert_eq!(month(day(2026, 11, 21), 2), (2027, 1));
        assert_eq!(month(day(2026, 1, 31), -1), (2025, 12));
        assert_eq!((month_len(2028, 2), month_len(2100, 2)), (29, 28));
    }

    #[test]
    fn single_all_day_and_several_day_events() {
        let (a, b) = (day(2026, 11, 1), day(2026, 12, 1));
        assert_eq!(found(&event("DTSTART:20261121T093000"), a, b), [(day(2026, 11, 21), Some(570))]);
        assert_eq!(days(&found(&event("DTSTART;VALUE=DATE:20261110\r\nDTEND;VALUE=DATE:20261113"), a, b)), [10, 11, 12]);
        assert_eq!(days(&found(&event("DTSTART;VALUE=DATE:20261110\r\nDURATION:P2D"), a, b)), [10, 11]);
        assert_eq!(days(&found(&event("DTSTART;VALUE=DATE:20261029\r\nDTEND;VALUE=DATE:20261103"), a, b)), [1, 2], "reaches into the month");
        assert!(found(&event("DTSTART:20261121T090000\r\nSTATUS:CANCELLED"), a, b).is_empty());
    }

    #[test]
    fn zones_land_on_the_local_day_across_dst() {
        let (a, b) = (day(2026, 10, 1), day(2026, 12, 31));
        assert_eq!(found(&event("DTSTART:20261121T023000Z"), a, b), [(day(2026, 11, 20), Some(21 * 60 + 30))]);
        // 9 in New York stays 9 when the clocks change on November 1st.
        let weekly = found(&event("DTSTART;TZID=America/New_York:20261028T090000\r\nRRULE:FREQ=WEEKLY;COUNT=3"), a, b);
        assert_eq!(weekly.iter().map(|o| o.1).collect::<Vec<_>>(), [Some(540); 3]);
        // 15:00 in Berlin is 10:00 here in the week only Europe has changed, 9:00 after.
        let berlin = found(&event("DTSTART;TZID=Europe/Berlin:20261028T150000\r\nRRULE:FREQ=WEEKLY;COUNT=2"), a, b);
        assert_eq!(berlin.iter().map(|o| o.1).collect::<Vec<_>>(), [Some(600), Some(540)]);
    }

    #[test]
    fn recurrences() {
        let (a, b) = (day(2026, 11, 1), day(2026, 12, 1));
        let r = |start: &str, rule: &str| days(&found(&event(&format!("DTSTART:{start}\r\nRRULE:{rule}")), a, b));
        assert_eq!(r("20261102T100000", "FREQ=DAILY;INTERVAL=3;COUNT=4"), [2, 5, 8, 11]);
        assert_eq!(r("20261102T100000", "FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261111"), [2, 4, 9, 11]);
        assert_eq!(r("20261103T100000", "FREQ=WEEKLY;INTERVAL=2"), [3, 17]);
        assert_eq!(r("20200114T100000", "FREQ=MONTHLY;BYDAY=2TU"), [10], "second Tuesday, years on");
        assert_eq!(r("20200131T100000", "FREQ=MONTHLY;BYDAY=-1FR"), [27], "last Friday");
        assert_eq!(r("20200131T100000", "FREQ=MONTHLY"), Vec::<i64>::new(), "November has no 31st");
        assert_eq!(r("20260130T100000", "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1"), [30], "last weekday");
        assert_eq!(r("19991126T000000", "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH"), [26], "Thanksgiving");
        assert_eq!(r("20200101T100000", "FREQ=DAILY").len(), 30, "fast-forwards");
        assert_eq!(r("20200101T100000", "FREQ=DAILY;COUNT=10"), Vec::<i64>::new(), "a count that ran out");
    }

    #[test]
    fn exceptions_and_moved_instances() {
        let body = event("UID:a\r\nDTSTART:20261102T100000\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE:20261103T100000")
            + &event("UID:a\r\nRECURRENCE-ID:20261104T100000\r\nDTSTART:20261106T120000");
        assert_eq!(days(&found(&body, day(2026, 11, 1), day(2026, 12, 1))), [2, 5, 6]);
    }

    #[test]
    fn hostile_files_dont_panic_hang_or_choose_files() {
        let (a, b) = (day(2026, 11, 1), day(2026, 12, 1));
        for body in ["DTSTART:99999999T999999", "DTSTART:2026\u{e9}121", "DTSTART:20261131", "DTSTART:20261101\r\nRRULE:FREQ=DAILY;INTERVAL=99999999999999999999",
                     "DTSTART:20261101\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=31;BYMONTH=2;COUNT=999999999", "DTSTART:20261101\r\nRRULE:FREQ=WEEKLY;BYDAY=,\u{e9},-9223372036854775808MO",
                     "DTSTART:20261101\r\nDURATION:P99999999999999W", "DTSTART;TZID=\"x:y\":20261101T100000"] {
            found(&event(body), a, b);
        }
        assert!(zone("/../../etc/passwd").is_none() && zone("../../../etc/passwd").is_none());
        assert_eq!(unescape("Tea\\, then\\ncake\u{7}"), "Tea, then cake");
        assert_eq!(unfold("SUMMARY:Lo\r\n ng\r\nX:1"), ["SUMMARY:Long", "X:1"]);
    }
}
