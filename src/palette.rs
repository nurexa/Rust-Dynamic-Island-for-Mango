//! The search palette: one text field over one ranked list, shared by every tool and
//! by `island dmenu`. This is the logic only; `modules/palette.rs` draws it.
use crate::tools::{Act, Entry, Item, Then};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};
use crate::settings::{KeyAct, Keys};
use std::sync::mpsc;

pub const VISIBLE_ROWS: usize = 8;

/// Who's waiting on a `dmenu` answer. Dropping it unanswered reads as a cancel.
pub struct Dmenu {
    pub reply: mpsc::Sender<String>,
    pub index: bool,
}

pub struct Ask {
    pub label: String,
    pub password: bool,
    pub then: Option<Then>,
}

pub struct Palette {
    pub entries: Vec<Entry>,
    pub current: usize,
    pub query: String,
    pub items: Vec<Item>,
    /// Indices into `items`, best match first.
    pub shown: Vec<usize>,
    pub selected: usize,
    pub top: usize,
    pub generation: u64,
    pub loading: bool,
    pub ask: Option<Ask>,
    /// Row waiting for its second Enter.
    pub confirming: Option<usize>,
    pub dmenu: Option<Dmenu>,
    /// Lists left behind by `Act::Open`, innermost last.
    pub back: Vec<Vec<Item>>,
}

pub enum Outcome {
    Nothing,
    Redraw,
    Close,
    /// Close and do this.
    Run(Act),
    /// Close and hand this to the text field's continuation.
    Answer(Then, String),
    Reload,
}

/// Higher is better; None is no match. Whole-prefix beats word-prefix beats
/// substring beats letters in order, and tighter wins within each.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let (q, t) = (query.to_lowercase(), text.to_lowercase());
    if t.starts_with(&q) {
        return Some(1000 - t.len().min(200) as i32);
    }
    if let Some(at) = t.find(&q) {
        let word_start = t[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        return Some(if word_start { 800 } else { 600 } - at.min(200) as i32);
    }
    // Scattered letters: "ffx" finds Firefox. Only when they're fairly close, or
    // any long string matches any short query.
    let mut needle = q.chars().peekable();
    let (mut last, mut gaps) = (None, 0usize);
    for (i, c) in t.chars().enumerate() {
        if needle.peek() == Some(&c) {
            needle.next();
            if let Some(l) = last {
                gaps += i - l - 1;
            }
            last = Some(i);
        }
    }
    (needle.peek().is_none() && gaps <= q.chars().count() * 2).then(|| 300 - gaps as i32)
}

/// Descriptions only match on whole words of the query, never scattered letters.
fn score_sub(query: &str, text: &str) -> Option<i32> {
    let at = text.to_lowercase().find(&query.to_lowercase())?;
    Some(200 - at.min(150) as i32)
}

impl Palette {
    pub fn new(entries: Vec<Entry>, current: usize, generation: u64) -> Self {
        Palette { entries, current, query: String::new(), items: Vec::new(), shown: Vec::new(), selected: 0, top: 0,
                  generation, loading: true, ask: None, confirming: None, dmenu: None, back: Vec::new() }
    }

    pub fn dmenu(prompt: String, password: bool, items: Vec<Item>, dmenu: Dmenu) -> Self {
        let mut p = Palette::new(vec![Entry::Dmenu(prompt.clone())], 0, 0);
        p.loading = false;
        p.items = items;
        p.dmenu = Some(dmenu);
        if password {
            p.ask = Some(Ask { label: prompt, password: true, then: None });
        }
        p.refilter();
        p
    }

    pub fn set_items(&mut self, generation: u64, items: Vec<Item>) {
        if generation == self.generation {
            self.items = items;
            self.back.clear();
            self.loading = false;
            self.refilter();
        }
    }

    fn refilter(&mut self) {
        let mut hits: Vec<(i32, usize)> = self.items.iter().enumerate().filter_map(|(i, it)| {
            let title = score(&self.query, &it.title);
            // The subtitle counts for less, so "code" finds Code before a mention of it.
            let sub = (!self.query.is_empty()).then(|| score_sub(&self.query, &it.sub)).flatten();
            title.into_iter().chain(sub).max().map(|s| (s + it.boost, i))
        }).collect();
        if !self.query.is_empty() {
            hits.sort_by_key(|h| -h.0);
        }
        self.shown = hits.into_iter().map(|h| h.1).collect();
        self.selected = 0;
        self.top = 0;
        self.confirming = None;
    }

    pub fn move_selection(&mut self, by: isize) {
        let Some(last) = self.shown.len().checked_sub(1) else { return };
        self.selected = self.selected.saturating_add_signed(by).min(last);
        self.confirming = None;
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + VISIBLE_ROWS {
            self.top = self.selected + 1 - VISIBLE_ROWS;
        }
    }

    pub fn scroll(&mut self, by: isize) {
        let max = self.shown.len().saturating_sub(VISIBLE_ROWS);
        self.top = self.top.saturating_add_signed(by).min(max);
        self.selected = self.selected.clamp(self.top, (self.top + VISIBLE_ROWS - 1).min(self.shown.len().saturating_sub(1)));
    }

    /// Enter, or a click on a row.
    pub fn choose(&mut self) -> Outcome {
        if let Some(ask) = self.ask.take() {
            return match ask.then {
                Some(then) => Outcome::Answer(then, std::mem::take(&mut self.query)),
                None => self.reply(self.query.clone()),
            };
        }
        let Some(&index) = self.shown.get(self.selected) else {
            let typed = self.entries[self.current].typed().filter(|_| !self.query.is_empty());
            if self.dmenu.is_some() && !self.query.is_empty() {
                return self.reply(self.query.clone());
            }
            return typed.map_or(Outcome::Nothing, |f| Outcome::Run(f(&self.query)));
        };
        if self.dmenu.as_ref().is_some_and(|d| d.index) {
            return self.reply(index.to_string());
        }
        match self.items[index].act.clone() {
            Act::Confirm(text, then) => {
                if self.confirming == Some(index) {
                    Outcome::Run(*then)
                } else {
                    self.confirming = Some(index);
                    self.items[index].sub = text;
                    Outcome::Redraw
                }
            }
            Act::Ask { label, password, then } => {
                self.query.clear();
                self.ask = Some(Ask { label, password, then: Some(then) });
                Outcome::Redraw
            }
            Act::Reply(text) => self.reply(text),
            Act::Open(items) => {
                self.back.push(std::mem::replace(&mut self.items, items));
                self.query.clear();
                self.refilter();
                Outcome::Redraw
            }
            Act::Back => self.pop(),
            act => Outcome::Run(act),
        }
    }

    /// One level out of a nested list. False when already at the top.
    fn pop(&mut self) -> Outcome {
        let Some(items) = self.back.pop() else { return Outcome::Nothing };
        self.items = items;
        self.query.clear();
        self.refilter();
        Outcome::Redraw
    }

    fn reply(&mut self, text: String) -> Outcome {
        if let Some(d) = self.dmenu.take() {
            let _ = d.reply.send(text);
        }
        Outcome::Close
    }

    pub fn switch(&mut self, to: usize) -> Outcome {
        if self.dmenu.is_some() || to >= self.entries.len() || to == self.current {
            return Outcome::Nothing;
        }
        self.current = to;
        self.query.clear();
        self.ask = None;
        self.items.clear();
        self.back.clear();
        self.refilter();
        self.loading = true;
        self.generation += 1;
        Outcome::Reload
    }

    fn step_tool(&mut self, by: isize) -> Outcome {
        let n = self.entries.len() as isize;
        self.switch((self.current as isize + by).rem_euclid(n) as usize)
    }

    pub fn key(&mut self, sym: Keysym, text: Option<&str>, mods: Modifiers, keys: &Keys) -> Outcome {
        let edited = |p: &mut Self| {
            p.refilter();
            Outcome::Redraw
        };
        let word = |p: &mut Self| {
            let keep = p.query.trim_end().rfind(char::is_whitespace).map_or(0, |i| i + 1);
            p.query.truncate(keep);
        };
        match keys.action(sym, mods.ctrl, mods.alt, mods.shift) {
            Some(KeyAct::Close) if !self.back.is_empty() => return self.pop(),
            Some(KeyAct::Close) => return Outcome::Close,
            Some(KeyAct::Accept) => return self.choose(),
            Some(KeyAct::Up) => self.move_selection(-1),
            Some(KeyAct::Down) => self.move_selection(1),
            Some(KeyAct::PageUp) => self.move_selection(-(VISIBLE_ROWS as isize)),
            Some(KeyAct::PageDown) => self.move_selection(VISIBLE_ROWS as isize),
            Some(KeyAct::NextTool) => return self.step_tool(1),
            Some(KeyAct::PrevTool) => return self.step_tool(-1),
            Some(KeyAct::Clear) => {
                self.query.clear();
                return edited(self);
            }
            Some(KeyAct::DeleteWord) => {
                word(self);
                return edited(self);
            }
            None if sym == Keysym::BackSpace => {
                if mods.ctrl {
                    word(self);
                } else {
                    self.query.pop();
                }
                return edited(self);
            }
            None if mods.ctrl => return Outcome::Nothing,
            None => {
                let Some(t) = text.filter(|t| !t.chars().any(char::is_control)) else { return Outcome::Nothing };
                self.query.push_str(t);
                return edited(self);
            }
        }
        Outcome::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Icon;

    fn palette(titles: &[&str]) -> Palette {
        let mut p = Palette::new(vec![Entry::Dmenu("x".into())], 0, 7);
        let items = titles.iter().map(|t| Item::new(*t, Icon::None, Act::Sh(t.to_string()))).collect();
        p.set_items(7, items);
        p
    }

    #[test]
    fn ranks_prefix_then_word_then_substring_then_letters() {
        let s = |q, t| score(q, t).unwrap();
        assert!(s("fi", "Firefox") > s("fi", "Web Files"));
        assert!(s("fi", "Web Files") > s("fi", "Wifi"));
        assert!(s("fi", "Wifi") > s("ffx", "Firefox"));
        assert!(s("ffx", "Firefox") > 0);
        assert_eq!(score("zz", "Firefox"), None);
        assert_eq!(score("abc", "a..........b..........c"), None, "scattered letters stay close");
        assert_eq!(score_sub("fire", "Configure things"), None);
        assert!(score_sub("archive", "Work with file archives").is_some());
    }

    #[test]
    fn typing_filters_and_the_best_match_is_selected() {
        let mut p = palette(&["Discord", "Firefox", "Files"]);
        let m = Modifiers::default();
        p.key(Keysym::f, Some("f"), m, &Keys::default());
        p.key(Keysym::i, Some("i"), m, &Keys::default());
        let names: Vec<&str> = p.shown.iter().map(|&i| p.items[i].title.as_str()).collect();
        assert_eq!(names, ["Files", "Firefox"]);
        assert!(matches!(p.key(Keysym::Return, None, m, &Keys::default()), Outcome::Run(Act::Sh(s)) if s == "Files"));
    }

    #[test]
    fn stale_lists_from_a_previous_tool_are_ignored() {
        let mut p = palette(&["a"]);
        p.set_items(6, vec![]);
        assert_eq!(p.items.len(), 1);
    }

    #[test]
    fn destructive_rows_need_a_second_enter_and_moving_cancels_it() {
        let mut p = palette(&["keep"]);
        p.items.push(Item::new("off", Icon::None, Act::Confirm("sure?".into(), Box::new(Act::Quit))));
        p.refilter();
        let m = Modifiers::default();
        p.move_selection(1);
        assert!(matches!(p.choose(), Outcome::Redraw));
        assert_eq!(p.items[1].sub, "sure?");
        p.move_selection(-1);
        p.move_selection(1);
        assert!(matches!(p.choose(), Outcome::Redraw), "moving away reset it");
        assert!(matches!(p.key(Keysym::Return, None, m, &Keys::default()), Outcome::Run(Act::Quit)));
    }

    #[test]
    fn selection_scrolls_the_window_and_stays_in_bounds() {
        let titles: Vec<String> = (0..20).map(|i| format!("row {i}")).collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let mut p = palette(&refs);
        p.move_selection(9);
        assert_eq!((p.selected, p.top), (9, 2));
        p.move_selection(100);
        assert_eq!(p.selected, 19);
        p.move_selection(-100);
        assert_eq!((p.selected, p.top), (0, 0));
    }

    #[test]
    fn dmenu_replies_with_the_row_or_its_index_and_cancels_on_drop() {
        let (tx, rx) = mpsc::channel();
        let items = vec![Item::new("a", Icon::None, Act::Reply("a".into())), Item::new("b", Icon::None, Act::Reply("b".into()))];
        let mut p = Palette::dmenu("pick".into(), false, items.clone(), Dmenu { reply: tx, index: false });
        p.move_selection(1);
        assert!(matches!(p.choose(), Outcome::Close));
        assert_eq!(rx.recv().unwrap(), "b");

        let (tx, rx) = mpsc::channel();
        let mut p = Palette::dmenu("pick".into(), false, items, Dmenu { reply: tx, index: true });
        p.move_selection(1);
        p.choose();
        assert_eq!(rx.recv().unwrap(), "1");

        let (tx, rx) = mpsc::channel::<String>();
        drop(Palette::dmenu("pick".into(), false, Vec::new(), Dmenu { reply: tx, index: false }));
        assert!(rx.recv().is_err());
    }

    #[test]
    fn password_prompts_answer_with_what_was_typed() {
        let (tx, rx) = mpsc::channel();
        let mut p = Palette::dmenu("pw".into(), true, Vec::new(), Dmenu { reply: tx, index: false });
        p.key(Keysym::a, Some("a"), Modifiers::default(), &Keys::default());
        p.key(Keysym::Return, None, Modifiers::default(), &Keys::default());
        assert_eq!(rx.recv().unwrap(), "a");
    }

    #[test]
    fn open_nests_a_list_and_escape_or_back_returns_to_it() {
        let mut p = palette(&[]);
        let leaf = Item::new("fill", Icon::None, Act::Quit);
        p.items = vec![Item::new("Settings", Icon::None, Act::Open(vec![Item::new("up", Icon::None, Act::Back), leaf.clone()]))];
        p.refilter();
        assert!(matches!(p.choose(), Outcome::Redraw));
        assert_eq!(p.items.len(), 2);
        assert!(matches!(p.key(Keysym::Escape, None, Modifiers::default(), &Keys::default()), Outcome::Redraw));
        assert_eq!(p.items[0].title, "Settings");
        // At the top Escape closes, as before.
        assert!(matches!(p.key(Keysym::Escape, None, Modifiers::default(), &Keys::default()), Outcome::Close));
        p.choose();
        p.selected = 0;
        assert!(matches!(p.choose(), Outcome::Redraw) && p.items[0].title == "Settings");
    }
}
