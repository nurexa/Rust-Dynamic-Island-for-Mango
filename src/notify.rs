//! Notifications: the queue behind the toasts and the history in the panel. The
//! D-Bus side lives in bus.rs. Any app can send anything here, so text is flattened
//! to plain characters and images are size-checked before they get near cairo.
#![cfg_attr(not(feature = "notifications"), allow(dead_code))]
use crate::settings::{KeyAct, Keys};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

const HISTORY: usize = 30;
/// Notes from one app closer together than this share a toast instead of queueing.
const BURST: Duration = Duration::from_secs(4);
/// The action key KDE, GNOME and swaync use for a reply typed into the notification.
pub const INLINE_REPLY: &str = "inline-reply";
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
    /// Hint text for the inline reply field (x-kde-reply-placeholder-text).
    pub placeholder: String,
}

impl Note {
    pub fn local(app: &str, title: &str, body: &str, image: NoteImage) -> Self {
        Note { id: 0, app: app.into(), title: title.into(), body: body.into(), urgency: 1, timeout: None,
               sticky: false, actions: Vec::new(), image, placeholder: String::new() }
    }

    /// Made by the island, whether or not it has its id yet. The bus never hands out 0.
    pub fn is_ours(&self) -> bool {
        self.id == 0 || self.is_local()
    }

    pub fn is_local(&self) -> bool {
        self.id >= LOCAL_IDS
    }

    /// The Reply button's label, when the app takes inline replies.
    pub fn reply_label(&self) -> Option<&str> {
        let (_, label) = self.actions.iter().find(|(k, _)| k == INLINE_REPLY)?;
        Some(if label.is_empty() { "Reply" } else { label })
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

/// A toast: the newest note, and how many earlier ones from the same app it stands for.
#[derive(Clone, Debug)]
pub struct Toast {
    pub note: Note,
    pub more: u32,
    /// When the newest note folded into it arrived.
    at: Instant,
}

/// A reply being typed into a toast or a history entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub id: u32,
    pub text: String,
}

pub enum Typed {
    Nothing,
    Redraw,
    Send(u32, String),
}

/// What's waiting, what's showing, and what's already been.
#[derive(Default)]
pub struct Center {
    pub queue: VecDeque<Toast>,
    pub showing: Option<Toast>,
    pub until: Option<Instant>,
    pub history: VecDeque<Note>,
    pub dnd: bool,
    pub replying: Option<Reply>,
    /// Apps whose history group is open. Not saved: a restart starts collapsed.
    expanded: Vec<String>,
    /// The showing toast's own timeout, for restarting it.
    wait: Duration,
    next_local: u32,
}

impl Center {
    pub fn new(dnd: bool) -> Self {
        Center { dnd, ..Default::default() }
    }

    /// An id for a note of our own, known before it's added, so its buttons can be matched.
    pub fn next_id(&mut self) -> u32 {
        self.next_local = self.next_local.wrapping_add(1) & 0xf_ffff;
        LOCAL_IDS + self.next_local
    }

    pub fn add(&mut self, note: Note) -> Option<u32> {
        self.add_at(note, Instant::now())
    }

    /// Returns the id of a note whose toast this one took over: it has left the screen.
    pub fn add_at(&mut self, mut note: Note, now: Instant) -> Option<u32> {
        if note.id == 0 {
            self.next_local = self.next_local.wrapping_add(1) & 0xf_ffff;
            note.id = LOCAL_IDS + self.next_local;
        }
        self.history.retain(|n| n.id != note.id);
        self.history.push_front(note.clone());
        self.history.truncate(HISTORY);
        let replying = self.replying.as_ref().map(|r| r.id);
        // Critical ones each matter on their own, and a toast being replied to stays put.
        let burst = |t: &Toast| t.note.app == note.app && t.note.urgency < 2 && note.urgency < 2
            && now < t.at + BURST && replying != Some(t.note.id);
        // An update replaces its own toast where it stands; a burst replaces its app's.
        if let Some(t) = self.showing.as_mut().filter(|t| t.note.id == note.id || burst(t)) {
            // A new timer, or the newest could vanish on the old one's schedule.
            self.until = (!note.sticky && replying != Some(note.id)).then(|| now + note.timeout.unwrap_or(self.wait));
            return fold(t, note, now);
        }
        if let Some(t) = self.queue.iter_mut().find(|t| t.note.id == note.id || burst(t)) {
            return fold(t, note, now);
        }
        if !self.dnd || note.urgency == 2 {
            self.queue.push_back(Toast { note, more: 0, at: now });
        }
        None
    }

    pub fn next(&mut self, low: Duration, normal: Duration, now: Instant) -> Option<&Note> {
        let toast = self.queue.pop_front()?;
        self.wait = toast.note.timeout.unwrap_or(if toast.note.urgency == 0 { low } else { normal });
        self.until = (!toast.note.sticky).then_some(now + self.wait);
        self.showing = Some(toast);
        self.showing.as_ref().map(|t| &t.note)
    }

    /// Takes it off the screen; history keeps it.
    pub fn finish(&mut self) -> Option<Note> {
        self.until = None;
        self.showing.take().map(|t| t.note)
    }

    /// Whichever list it's in. True when it was on screen.
    pub fn remove(&mut self, id: u32) -> bool {
        self.queue.retain(|t| t.note.id != id);
        self.history.retain(|n| n.id != id);
        let history = &self.history;
        self.expanded.retain(|app| history.iter().any(|n| &n.app == app));
        if self.replying.as_ref().is_some_and(|r| r.id == id) {
            self.replying = None;
        }
        let shown = self.showing.as_ref().is_some_and(|t| t.note.id == id);
        if shown {
            self.finish();
        }
        shown
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.expanded.clear();
        self.cancel_reply(Instant::now());
    }

    /// History by app: the app with the newest note first, and each app's newest first.
    pub fn groups(&self) -> Vec<(&str, Vec<&Note>)> {
        let mut out: Vec<(&str, Vec<&Note>)> = Vec::new();
        for n in &self.history {
            match out.iter_mut().find(|g| g.0 == n.app) {
                Some(g) => g.1.push(n),
                None => out.push((&n.app, vec![n])),
            }
        }
        out
    }

    pub fn expanded(&self, app: &str) -> bool {
        self.expanded.iter().any(|a| a == app)
    }

    pub fn toggle(&mut self, app: &str) {
        match self.expanded.iter().position(|a| a == app) {
            Some(i) => drop(self.expanded.swap_remove(i)),
            None => self.expanded.push(app.to_owned()),
        }
    }

    /// Every note in history from one app, for clearing its group.
    pub fn ids_from(&self, app: &str) -> Vec<u32> {
        self.history.iter().filter(|n| n.app == app).map(|n| n.id).collect()
    }

    /// Opens the reply field. A toast holds still while it's typed into.
    pub fn start_reply(&mut self, id: u32) {
        if !self.history.iter().any(|n| n.id == id && n.reply_label().is_some()) {
            return;
        }
        if self.showing.as_ref().is_some_and(|t| t.note.id == id) {
            self.until = None;
        }
        self.replying = Some(Reply { id, text: String::new() });
    }

    /// Closes the field unsent; a toast picks its timer up again.
    pub fn cancel_reply(&mut self, now: Instant) {
        let Some(r) = self.replying.take() else { return };
        if self.showing.as_ref().is_some_and(|t| t.note.id == r.id && !t.note.sticky) {
            self.until = Some(now + self.wait);
        }
    }

    /// A key while the reply field is open, with the palette's text-field bindings.
    pub fn reply_key(&mut self, sym: Keysym, text: Option<&str>, mods: Modifiers, keys: &Keys) -> Typed {
        let Some(r) = &mut self.replying else { return Typed::Nothing };
        let word = |s: &mut String| s.truncate(s.trim_end().rfind(char::is_whitespace).map_or(0, |i| i + 1));
        match keys.action(sym, mods.ctrl, mods.alt, mods.shift) {
            Some(KeyAct::Accept) if r.text.trim().is_empty() => return Typed::Nothing,
            Some(KeyAct::Accept) => return self.replying.take().map_or(Typed::Nothing, |r| Typed::Send(r.id, r.text)),
            Some(KeyAct::Close) => self.cancel_reply(Instant::now()),
            Some(KeyAct::Clear) => r.text.clear(),
            Some(KeyAct::DeleteWord) => word(&mut r.text),
            None if sym == Keysym::BackSpace && mods.ctrl => word(&mut r.text),
            None if sym == Keysym::BackSpace => drop(r.text.pop()),
            None if mods.ctrl => return Typed::Nothing,
            None => match text.filter(|t| !t.chars().any(char::is_control)) {
                Some(t) => r.text.push_str(t),
                None => return Typed::Nothing,
            },
            _ => return Typed::Nothing,
        }
        Typed::Redraw
    }
}

/// The newest note takes the toast's place. Returns the id it pushed off the screen.
fn fold(t: &mut Toast, note: Note, now: Instant) -> Option<u32> {
    let gone = (t.note.id != note.id).then_some(t.note.id);
    t.more += gone.is_some() as u32;
    t.at = now;
    t.note = note;
    gone
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
        assert_eq!(c.showing.as_ref().unwrap().note.title, "new");
        assert_eq!(c.showing.as_ref().unwrap().more, 0, "an update isn't a second note");
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
        assert!(c.queue[0].note.is_local());
        assert!(Note::local("s", "t", "", NoteImage::None).is_ours() && c.queue[0].note.is_ours());
        assert!(!note(7, 1).is_ours(), "one from the bus is never relayed");
        assert!(c.queue[0].note.id != 0);
    }

    fn from(app: &str, id: u32) -> Note {
        Note { id, app: app.into(), ..note(id, 1) }
    }

    #[test]
    fn history_groups_by_app_newest_app_first() {
        let mut c = Center::default();
        for (app, id) in [("chat", 1), ("mail", 2), ("chat", 3), ("build", 4)] {
            c.add(from(app, id));
        }
        let g: Vec<(&str, Vec<u32>)> = c.groups().into_iter().map(|(a, n)| (a, n.iter().map(|n| n.id).collect())).collect();
        assert_eq!(g, [("build", vec![4]), ("chat", vec![3, 1]), ("mail", vec![2])]);
        assert_eq!(c.ids_from("chat"), [3, 1]);
    }

    #[test]
    fn groups_start_collapsed_and_forget_apps_that_leave() {
        let mut c = Center::default();
        c.add(from("chat", 1));
        c.add(from("chat", 2));
        assert!(!c.expanded("chat"));
        c.toggle("chat");
        assert!(c.expanded("chat"));
        c.toggle("chat");
        assert!(!c.expanded("chat"));
        c.toggle("chat");
        for id in c.ids_from("chat") {
            c.remove(id);
        }
        assert!(c.groups().is_empty());
        assert!(c.expanded.is_empty(), "no state left behind for an app that's gone");
    }

    #[test]
    fn a_burst_from_one_app_shares_a_toast() {
        let mut c = Center::default();
        let now = Instant::now();
        c.add_at(from("chat", 1), now);
        c.next(S, 5 * S, now);
        assert_eq!(c.add_at(from("chat", 2), now + S), Some(1), "the first one leaves the screen");
        assert_eq!(c.add_at(from("chat", 3), now + 2 * S), Some(2));
        let t = c.showing.as_ref().unwrap();
        assert_eq!((t.note.id, t.more), (3, 2));
        assert_eq!(c.until, Some(now + 7 * S), "the newest gets the full time");
        assert!(c.queue.is_empty());
        assert_eq!(c.history.len(), 3, "history still has every one");

        // Waiting behind another app's toast, they fold up the same way.
        c.add_at(from("mail", 4), now + 2 * S);
        c.add_at(from("mail", 5), now + 3 * S);
        assert_eq!(c.queue.len(), 1);
        assert_eq!((c.queue[0].note.id, c.queue[0].more), (5, 1));
    }

    #[test]
    fn slow_notes_and_critical_ones_still_queue() {
        let mut c = Center::default();
        let now = Instant::now();
        c.add_at(from("chat", 1), now);
        c.next(S, 60 * S, now);
        assert_eq!(c.add_at(from("chat", 2), now + 10 * S), None, "outside the window");
        assert_eq!(c.add_at(Note { urgency: 2, ..from("chat", 3) }, now + 10 * S), None);
        assert_eq!(c.queue.len(), 2);
        assert_eq!(c.showing.as_ref().unwrap().note.id, 1);
    }

    fn replyable(id: u32) -> Note {
        Note { actions: vec![(INLINE_REPLY.into(), "Answer".into())], ..from("chat", id) }
    }

    fn typed(c: &mut Center, keys: &[(Keysym, Option<&str>)]) -> Typed {
        let mut last = Typed::Nothing;
        for (sym, text) in keys {
            last = c.reply_key(*sym, *text, Modifiers::default(), &Keys::default());
        }
        last
    }

    #[test]
    fn inline_reply_holds_the_toast_and_sends_what_was_typed() {
        let mut c = Center::default();
        let now = Instant::now();
        c.add_at(from("chat", 9), now);
        c.start_reply(9);
        assert_eq!(c.replying, None, "only notes that asked for replies get a field");
        assert_eq!(replyable(1).reply_label(), Some("Answer"));

        c.add_at(replyable(1), now);
        c.next(S, 5 * S, now);
        c.start_reply(1);
        assert_eq!(c.until, None, "a toast being typed into doesn't time out");
        c.add_at(from("chat", 2), now + S);
        assert_eq!(c.showing.as_ref().unwrap().note.id, 1, "nor gets swapped out by a burst");
        let sent = typed(&mut c, &[(Keysym::h, Some("h")), (Keysym::i, Some("i")), (Keysym::x, Some("x")),
                                   (Keysym::BackSpace, None), (Keysym::Return, None)]);
        assert!(matches!(sent, Typed::Send(1, ref t) if t == "hi"));
        assert_eq!(c.replying, None);
    }

    #[test]
    fn escape_cancels_and_the_toast_times_out_again() {
        let mut c = Center::default();
        let now = Instant::now();
        c.add_at(replyable(1), now);
        c.next(S, 5 * S, now);
        c.start_reply(1);
        assert!(matches!(typed(&mut c, &[(Keysym::Return, None)]), Typed::Nothing), "nothing to send yet");
        typed(&mut c, &[(Keysym::a, Some("a")), (Keysym::Escape, None)]);
        assert_eq!(c.replying, None);
        assert!(c.until.is_some());
        c.start_reply(1);
        c.remove(1);
        assert_eq!(c.replying, None, "the field goes with its note");
    }
}
