//! Notifications: the queue behind the toasts and the history in the panel. The
//! D-Bus side lives in bus.rs. Any app can send anything here, so text is flattened
//! to plain characters and images are size-checked before they get near cairo.
#![cfg_attr(not(feature = "notifications"), allow(dead_code))]
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

const HISTORY: usize = 30;
/// Notes the island raises itself live up here, clear of ids given to apps.
const LOCAL_IDS: u32 = 0xfff0_0000;
const MAX_IMAGE_SIDE: i32 = 2048;

#[derive(Clone, Debug, PartialEq)]
pub enum NoteImage {
    None,
    Path(PathBuf),
    /// A name from an icon theme.
    Named(String),
    /// Premultiplied RGBA.
    Pixels { w: i32, h: i32, rgba: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub title: String,
    pub body: String,
    /// 0 low, 1 normal, 2 critical.
    pub urgency: u8,
    /// None until shown: use the configured default for the urgency.
    pub timeout: Option<Duration>,
    pub sticky: bool,
    /// Key, label. "default" is the toast's own click.
    pub actions: Vec<(String, String)>,
    pub image: NoteImage,
}

impl Note {
    pub fn local(app: &str, title: &str, body: &str, image: NoteImage) -> Self {
        Note { id: 0, app: app.into(), title: title.into(), body: body.into(), urgency: 1, timeout: None,
               sticky: false, actions: Vec::new(), image }
    }

    pub fn is_local(&self) -> bool {
        self.id >= LOCAL_IDS
    }
}

/// Notification bodies may carry a little HTML. It's flattened, not rendered:
/// pango markup from a stranger is a parser to attack and a way to fake the UI.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
        .replace("&#39;", "'").replace("&amp;", "&");
    out.chars().take(500).collect()
}

/// The image-data hint: raw rows in RGB or RGBA, straight (not premultiplied) alpha.
pub fn pixels(w: i32, h: i32, rowstride: i32, alpha: bool, data: &[u8]) -> NoteImage {
    let channels = if alpha { 4 } else { 3 };
    let valid = (1..=MAX_IMAGE_SIDE).contains(&w) && (1..=MAX_IMAGE_SIDE).contains(&h) && rowstride >= w * channels;
    if !valid || data.len() < (rowstride * (h - 1) + w * channels) as usize {
        return NoteImage::None;
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in data.chunks(rowstride as usize).take(h as usize) {
        for p in row[..(w * channels) as usize].chunks_exact(channels as usize) {
            let a = if alpha { p[3] as u32 } else { 255 };
            let pm = |c: u8| (c as u32 * a / 255) as u8;
            rgba.extend_from_slice(&[pm(p[0]), pm(p[1]), pm(p[2]), a as u8]);
        }
    }
    NoteImage::Pixels { w, h, rgba }
}

/// What's waiting, what's showing, and what's already been.
#[derive(Default)]
pub struct Center {
    pub queue: VecDeque<Note>,
    pub showing: Option<Note>,
    pub until: Option<Instant>,
    pub history: VecDeque<Note>,
    pub dnd: bool,
    next_local: u32,
}

impl Center {
    pub fn new(dnd: bool) -> Self {
        Center { dnd, ..Default::default() }
    }

    pub fn add(&mut self, mut note: Note) {
        if note.id == 0 {
            self.next_local = self.next_local.wrapping_add(1) & 0xf_ffff;
            note.id = LOCAL_IDS + self.next_local;
        }
        self.history.retain(|n| n.id != note.id);
        self.queue.retain(|n| n.id != note.id);
        self.history.push_front(note.clone());
        self.history.truncate(HISTORY);
        // An update to the one on screen replaces it where it stands.
        if let Some(showing) = self.showing.as_mut().filter(|s| s.id == note.id) {
            // A new timer, or an update could vanish on the old one's schedule.
            if let Some(wait) = note.timeout {
                self.until = Some(Instant::now() + wait);
            }
            if note.sticky {
                self.until = None;
            }
            *showing = note;
        } else if !self.dnd || note.urgency == 2 {
            self.queue.push_back(note);
        }
    }

    pub fn next(&mut self, low: Duration, normal: Duration, now: Instant) -> Option<&Note> {
        let note = self.queue.pop_front()?;
        let wait = note.timeout.unwrap_or(if note.urgency == 0 { low } else { normal });
        self.until = (!note.sticky).then_some(now + wait);
        self.showing = Some(note);
        self.showing.as_ref()
    }

    /// Takes it off the screen; history keeps it.
    pub fn finish(&mut self) -> Option<Note> {
        self.until = None;
        self.showing.take()
    }

    /// Whichever list it's in. True when it was on screen.
    pub fn remove(&mut self, id: u32) -> bool {
        self.queue.retain(|n| n.id != id);
        self.history.retain(|n| n.id != id);
        let shown = self.showing.as_ref().is_some_and(|n| n.id == id);
        if shown {
            self.finish();
        }
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, urgency: u8) -> Note {
        Note { id, urgency, ..Note::local("a", "t", "b", NoteImage::None) }
    }

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn markup_is_flattened_and_never_passed_through() {
        assert_eq!(plain("<b>hi</b> &amp; <a href=\"x\">bye</a>"), "hi & bye");
        assert_eq!(plain("1 &lt; 2"), "1 < 2");
        assert_eq!(plain(&"x".repeat(900)).len(), 500);
    }

    #[test]
    fn image_data_is_checked_before_use() {
        assert_eq!(pixels(1, 1, 3, false, &[10, 20, 30]), NoteImage::Pixels { w: 1, h: 1, rgba: vec![10, 20, 30, 255] });
        assert_eq!(pixels(1, 1, 4, true, &[200, 0, 0, 128]), NoteImage::Pixels { w: 1, h: 1, rgba: vec![100, 0, 0, 128] });
        assert_eq!(pixels(2, 2, 6, false, &[0; 3]), NoteImage::None, "too little data");
        assert_eq!(pixels(-1, 2, 6, false, &[0; 30]), NoteImage::None);
        assert_eq!(pixels(100_000, 2, 300_000, false, &[0; 30]), NoteImage::None);
    }

    #[test]
    fn updates_replace_in_place_and_expire_on_the_right_clock() {
        let mut c = Center::default();
        let now = Instant::now();
        c.add(note(5, 1));
        assert!(c.next(S, 5 * S, now).is_some());
        assert_eq!(c.until, Some(now + 5 * S));
        c.add(Note { title: "new".into(), ..note(5, 1) });
        assert_eq!(c.showing.as_ref().unwrap().title, "new");
        assert!(c.queue.is_empty(), "no second toast for an update");
        c.add(Note { timeout: Some(60 * S), ..note(5, 1) });
        assert!(c.until.unwrap() > now + 30 * S, "an update restarts the timer");
        c.add(Note { sticky: true, ..note(5, 1) });
        assert_eq!(c.until, None, "and can turn it off");
        assert_eq!(c.history.len(), 1);
    }

    #[test]
    fn do_not_disturb_keeps_history_but_lets_critical_through() {
        let mut c = Center { dnd: true, ..Default::default() };
        c.add(note(1, 1));
        assert!(c.queue.is_empty());
        assert_eq!(c.history.len(), 1);
        c.add(note(2, 2));
        assert_eq!(c.queue.len(), 1);
    }

    #[test]
    fn local_notes_get_ids_apps_can_never_collide_with() {
        let mut c = Center::default();
        c.add(Note::local("s", "t", "", NoteImage::None));
        assert!(c.queue[0].is_local());
        assert!(c.queue[0].id != 0);
    }
}
