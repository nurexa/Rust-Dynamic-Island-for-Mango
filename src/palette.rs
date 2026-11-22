//! The search palette: one text field over one ranked list, shared by every tool and
//! by `island dmenu`. This is the logic only; `modules/palette.rs` draws it.
use crate::tools::{Act, Ctx, Entry, Icon, Item, Then, quote};
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
    /// Web search URL with `{}` for the query; empty hides the row.
    pub web: String,
    /// Answer sums and conversions in the main search.
    pub calc: bool,
    /// Rows worked out from the query (a sum, a web search), kept at the end of `items`.
    instant: usize,
    /// The row that opened each list in `back`: its title, its place, the first in view.
    pub trail: Vec<(String, usize, usize)>,
    /// The list as it was before a setting's field took its place.
    typing_over: Option<(String, usize, usize)>,
    /// Where to come back to once the list has been built again.
    pub replay: Option<Place>,
    /// How the last change went, and whether it did.
    pub notice: Option<(String, bool)>,
}

/// Somewhere in nested lists, found again by name when they're rebuilt.
pub struct Place {
    trail: Vec<(String, usize, usize)>,
    query: String,
    selected: usize,
    top: usize,
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
    /// Build this list on a worker; it replaces the current one when it arrives.
    Load(fn(&Ctx) -> Vec<Item>),
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
                  generation, loading: true, ask: None, confirming: None, dmenu: None, back: Vec::new(),
                  web: String::new(), calc: true, instant: 0, trail: Vec::new(), typing_over: None, replay: None, notice: None }
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
            self.instant = 0;
            self.loading = false;
            if let Some(place) = self.replay.take() {
                return self.go_to(place);
            }
            self.refilter();
        }
    }

    /// Where to come back to after a change that steps out of `up` pages.
    pub fn place(&self, up: usize) -> Place {
        let (query, selected, top) = self.typing_over.clone().unwrap_or((self.query.clone(), self.selected, self.top));
        let mut place = Place { trail: self.trail.clone(), query, selected, top };
        for _ in 0..up {
            let Some((_, selected, top)) = place.trail.pop() else { break };
            (place.selected, place.top) = (selected, top);
            place.query.clear();
        }
        place
    }

    fn go_to(&mut self, place: Place) {
        self.back.clear();
        self.trail.clear();
        self.ask = None;
        self.typing_over = None;
        let mut arrived = true;
        for step in place.trail {
            let found = self.items.iter().find_map(|i| match &i.act {
                Act::Open(inner) if i.title == step.0 => Some(inner.clone()),
                _ => None,
            });
            let Some(inner) = found else {
                // That page is gone with the change: stay on the one it was in.
                (self.selected, self.top) = (step.1, step.2);
                arrived = false;
                break;
            };
            self.back.push(std::mem::replace(&mut self.items, inner));
            self.trail.push(step);
        }
        let (selected, top) = if arrived { (place.selected, place.top) } else { (self.selected, self.top) };
        self.query = if arrived { place.query } else { String::new() };
        self.refilter();
        self.land(selected, top);
    }

    fn land(&mut self, selected: usize, top: usize) {
        self.selected = selected.min(self.shown.len().saturating_sub(1));
        self.top = top.min(self.selected);
        if self.selected >= self.top + VISIBLE_ROWS {
            self.top = self.selected + 1 - VISIBLE_ROWS;
        }
    }

    /// Esc in a setting's field puts the list back as it was.
    fn stop_typing(&mut self) -> Outcome {
        let Some((query, selected, top)) = self.typing_over.take() else { return Outcome::Nothing };
        self.ask = None;
        self.notice = None;
        self.query = query;
        self.refilter();
        self.land(selected, top);
        Outcome::Redraw
    }

    fn refilter(&mut self) {
        self.items.truncate(self.items.len() - self.instant);
        self.instant = 0;
        let mut hits: Vec<(i32, usize)> = self.items.iter().enumerate().filter_map(|(i, it)| {
            if it.deep && self.query.is_empty() {
                return None;
            }
            let title = score(&self.query, &it.title);
            // The subtitle counts for less, so "code" finds Code before a mention of it.
            let sub = (!self.query.is_empty()).then(|| score_sub(&self.query, &it.sub)).flatten();
            title.into_iter().chain(sub).max().map(|s| (s + it.boost, i))
        }).collect();
        if !self.query.is_empty() {
            hits.sort_by_key(|h| -h.0);
        }
        self.shown = hits.into_iter().map(|h| h.1).collect();
        if self.entries[self.current].live() {
            // The tool already searched for exactly this; its order is the ranking.
            self.shown = (0..self.items.len()).collect();
        }
        self.add_instant();
        self.selected = 0;
        self.top = 0;
        self.confirming = None;
    }

    /// A sum's answer goes first (Enter copies it), a web search last.
    fn add_instant(&mut self) {
        let q = self.query.trim();
        if q.is_empty() || self.ask.is_some() || self.dmenu.is_some() || !self.back.is_empty() || !self.entries[self.current].instant() {
            return;
        }
        #[cfg(feature = "calc")]
        if let Some((shown, copy)) = crate::tools::calc::answer(q).filter(|_| self.calc) {
            let act = Act::Sh(format!("printf %s {} | wl-copy", quote(&copy)));
            self.items.push(Item::new(shown, Icon::Glyph("calculate".into()), act).sub(format!("{q} · Enter copies it")));
            self.shown.insert(0, self.items.len() - 1);
            self.instant += 1;
        }
        if q.eq_ignore_ascii_case("meow") {
            let act = Act::Sh("printf %s '=^･ω･^=' | wl-copy".into());
            self.items.push(Item::new("=^･ω･^=  mrrp!", Icon::Glyph("star_shine".into()), act).sub("There's a cat in here. Enter copies it"));
            self.shown.insert(0, self.items.len() - 1);
            self.instant += 1;
        }
        if !self.web.is_empty() {
            let url = self.web.replace("{}", &url_encode(q));
            let act = Act::Sh(format!("xdg-open {}", quote(&url)));
            self.items.push(Item::new(format!("Search the web for “{q}”"), Icon::Glyph("language".into()), act));
            self.shown.push(self.items.len() - 1);
            self.instant += 1;
        }
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
            Act::Type { label, value, field } => {
                self.typing_over = Some((std::mem::replace(&mut self.query, value), self.selected, self.top));
                self.ask = Some(Ask { label, password: false, then: Some(Then::Setting(field)) });
                self.notice = None;
                Outcome::Redraw
            }
            Act::Reply(text) => self.reply(text),
            Act::Open(items) => {
                self.trail.push((self.items[index].title.clone(), self.selected, self.top));
                self.notice = None;
                self.back.push(std::mem::replace(&mut self.items, items));
                self.query.clear();
                self.refilter();
                Outcome::Redraw
            }
            Act::Back => self.pop(),
            Act::Load(list) => {
                self.trail.push((self.items[index].title.clone(), self.selected, self.top));
                self.back.push(std::mem::take(&mut self.items));
                self.query.clear();
                self.refilter();
                self.loading = true;
                self.generation += 1;
                Outcome::Load(list)
            }
            act => Outcome::Run(act),
        }
    }

    /// One level out of a nested list. False when already at the top.
    fn pop(&mut self) -> Outcome {
        let Some(items) = self.back.pop() else { return Outcome::Nothing };
        self.items = items;
        // A list still loading must not land on the one we went back to.
        self.generation += 1;
        self.loading = false;
        self.query.clear();
        self.notice = None;
        self.refilter();
        // Back on the row that led in.
        if let Some((_, selected, top)) = self.trail.pop() {
            self.land(selected, top);
        }
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
        self.typing_over = None;
        self.replay = None;
        self.notice = None;
        self.items.clear();
        self.back.clear();
        self.trail.clear();
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
            if p.entries[p.current].live() && p.back.is_empty() && p.ask.is_none() {
                // Ask the tool again; a slower answer to an older query is dropped by generation.
                p.items.clear();
                p.instant = 0;
                p.generation += 1;
                p.loading = true;
                p.refilter();
                return Outcome::Reload;
            }
            p.refilter();
            Outcome::Redraw
        };
        let word = |p: &mut Self| {
            let keep = p.query.trim_end().rfind(char::is_whitespace).map_or(0, |i| i + 1);
            p.query.truncate(keep);
        };
        match keys.action(sym, mods.ctrl, mods.alt, mods.shift) {
            Some(KeyAct::Close) if self.typing_over.is_some() && self.ask.is_some() => return self.stop_typing(),
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

/// Percent-encodes everything but the URL-safe characters, so a query can't break out of its parameter.
fn url_encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "+".into(),
        _ => format!("%{b:02X}"),
    }).collect()
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
    fn the_main_search_answers_sums_and_offers_the_web() {
        let mut p = Palette::new(vec![Entry::Builtin(crate::tools::TOOLS.iter().position(|t| t.name == "apps").unwrap())], 0, 1);
        p.web = "https://example.com/?q={}".into();
        p.set_items(1, vec![Item::new("Firefox", Icon::None, Act::Sh("firefox".into()))]);
        for c in "2*21".chars() {
            p.key(Keysym::a, Some(&c.to_string()), Modifiers::default(), &Keys::default());
        }
        let titles: Vec<&str> = p.shown.iter().map(|&i| p.items[i].title.as_str()).collect();
        assert_eq!(titles, ["= 42", "Search the web for “2*21”"]);
        assert!(matches!(p.choose(), Outcome::Run(Act::Sh(s)) if s.contains("'42'") && s.contains("wl-copy")));
        p.query = "a&b c".into();
        p.refilter();
        assert!(matches!(&p.items[p.shown[0]].act, Act::Sh(s) if s.ends_with("?q=a%26b+c'")));
        assert_eq!(p.items.len(), 2, "old instant rows are replaced, not piled up");
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

    #[test]
    fn a_slow_list_loads_in_place_and_escape_beats_it_back() {
        fn slow(_: &Ctx) -> Vec<Item> {
            Vec::new()
        }
        let mut p = palette(&[]);
        p.items = vec![Item::new("Scan", Icon::None, Act::Load(slow))];
        p.refilter();
        assert!(matches!(p.choose(), Outcome::Load(_)) && p.loading);
        p.set_items(p.generation, vec![Item::new("found", Icon::None, Act::Quit)]);
        assert_eq!((p.items[0].title.as_str(), p.loading), ("found", false));
        assert!(matches!(p.key(Keysym::Escape, None, Modifiers::default(), &Keys::default()), Outcome::Redraw));
        assert_eq!(p.items[0].title, "Scan", "Escape goes back to where the scan started");
        let late = p.generation;
        p.choose();
        p.key(Keysym::Escape, None, Modifiers::default(), &Keys::default());
        p.set_items(late + 1, vec![]);
        assert_eq!(p.items[0].title, "Scan", "a scan that finishes after Escape is dropped");
    }
}
