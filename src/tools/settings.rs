//! Settings: all of config.toml as pages of switches, choices and fields. What's
//! changed here is written there (src/config_edit.rs). Typing finds any setting
//! from the first page. A test fails if a config key has no control.
use super::{Act, Ctx, Edit, Field, Icon, Input, Item, Tail, quote};
use crate::{
    config_edit::{Change, path},
    prefs,
    settings::{Config, State},
    style::Style,
};
use serde_json::{Value as Json, json};

#[derive(Clone, Copy)]
enum Kind {
    Switch,
    /// One of these words.
    Pick(&'static [&'static str]),
    Style,
    /// One of these numbers, or one typed within the limits. `whole` takes no fraction.
    Num { steps: &'static [f64], unit: &'static str, min: f64, max: f64, whole: bool },
    /// A number, or left to the style.
    Opacity,
    Text,
    Palette,
    /// An ordered list drawn from what's available.
    Order(Options),
    Chords,
}

#[derive(Clone, Copy)]
enum Options {
    Pill,
    Panel,
    Sidebar,
    Tools,
    Visualizer,
}

struct Setting {
    key: &'static str,
    title: &'static str,
    help: &'static str,
    kind: Kind,
}

const fn s(key: &'static str, title: &'static str, help: &'static str, kind: Kind) -> Setting {
    Setting { key, title, help, kind }
}

const fn num(steps: &'static [f64], unit: &'static str, min: f64, max: f64, whole: bool) -> Kind {
    Kind::Num { steps, unit, min, max, whole }
}

struct Page {
    title: &'static str,
    icon: &'static str,
    about: &'static str,
    settings: &'static [Setting],
}

const PAGES: &[Page] = &[
    Page { title: "Appearance", icon: "colorize", about: "Style, colors, icons, size, blur, font", settings: &[
        s("style", "Style", "The island's shape", Kind::Style),
        s("ui.scale", "Size", "Everything, bigger or smaller", num(&[0.75, 0.85, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0], "×", 0.75, 2.0, false)),
        s("ui.blur", "Background blur", "Auto is glass only", Kind::Pick(&["auto", "on", "off"])),
        s("ui.opacity", "Opacity", "How solid backgrounds are", Kind::Opacity),
        s("ui.solid_icons", "Filled icons", "Off draws thin outlines", Kind::Switch),
        s("icon_style", "App icons", "Glyph follows the colors, color is the apps' own", Kind::Pick(&["glyph", "color"])),
        s("palette", "Color palette file", "Re-read whenever it changes", Kind::Palette),
        s("font", "Font", "Any installed family", Kind::Text),
        s("icon_font", "Icon font", "A Nerd Font, for app logos", Kind::Text),
    ] },
    Page { title: "Layout", icon: "apps", about: "What's on the pill, panel and sidebar, in what order", settings: &[
        s("pill", "Pill", "Left to right", Kind::Order(Options::Pill)),
        s("panel", "Panel", "Top to bottom", Kind::Order(Options::Panel)),
        s("sidebar", "Sidebar", "Top to bottom", Kind::Order(Options::Sidebar)),
        s("tools", "Search tabs", "Left to right", Kind::Order(Options::Tools)),
        s("visualizer", "Music visualizers", "Where the music shows", Kind::Order(Options::Visualizer)),
    ] },
    Page { title: "Behavior", icon: "mouse", about: "Hover and timing", settings: &[
        s("ui.hover_open", "Open on hover", "Off waits for a click", Kind::Switch),
        s("ui.hover_sidebar", "Sidebar on hover", "Rest on the frame's right border", Kind::Switch),
        s("ui.hover_open_ms", "Delay before opening", "How long a hover takes", num(&[0.0, 80.0, 130.0, 200.0, 300.0, 500.0], " ms", 0.0, 5000.0, true)),
        s("ui.hover_close_ms", "Delay before closing", "Grace when the pointer leaves", num(&[100.0, 200.0, 280.0, 400.0, 700.0, 1000.0], " ms", 0.0, 10000.0, true)),
    ] },
    Page { title: "Notifications", icon: "notifications", about: "On or off, time on screen", settings: &[
        s("notifications", "Show notifications", "Off leaves them to another daemon", Kind::Switch),
        s("notify.normal", "Normal stay for", "Critical always waits for you", num(&[3.0, 5.0, 8.0, 10.0, 15.0], " s", 1.0, 600.0, true)),
        s("notify.low", "Quiet ones stay for", "Low urgency", num(&[2.0, 3.0, 5.0, 8.0], " s", 1.0, 600.0, true)),
    ] },
    Page { title: "Clock and calendar", icon: "calendar_month", about: "Time format, week, event files", settings: &[
        s("clock", "Clock format", "%H:%M for 24-hour, %I:%M %p for 12", Kind::Text),
        s("calendar.week_start", "Week starts on", "The calendar's first column", Kind::Pick(&["sunday", "monday", "saturday"])),
        s("calendar.dir", "Calendar folder", "Local .ics files, never the network", Kind::Text),
    ] },
    Page { title: "Night light", icon: "bedtime", about: "Schedule, warmth, dimming", settings: &[
        s("nightlight.schedule", "Schedule", "Like 20:00-07:00, empty is by hand", Kind::Text),
        s("nightlight.warmth", "Warmth", "Lower is warmer", num(&[2500.0, 3000.0, 3400.0, 4000.0, 5000.0, 6500.0], "K", 2500.0, 6500.0, true)),
        s("nightlight.dim", "Brightness while on", "1 is no dimming", num(&[0.6, 0.7, 0.85, 0.9, 1.0], "×", 0.2, 1.0, false)),
    ] },
    Page { title: "Focus timer", icon: "timer", about: "Pomodoro lengths and rounds", settings: &[
        s("pomodoro.focus", "Focus", "One round", num(&[15.0, 20.0, 25.0, 30.0, 45.0, 50.0, 60.0], " min", 1.0, 600.0, true)),
        s("pomodoro.short", "Short break", "Between rounds", num(&[3.0, 5.0, 10.0, 15.0], " min", 1.0, 600.0, true)),
        s("pomodoro.long", "Long break", "After the last round", num(&[10.0, 15.0, 20.0, 30.0], " min", 1.0, 600.0, true)),
        s("pomodoro.rounds", "Rounds", "Before the long break", num(&[2.0, 3.0, 4.0, 5.0, 6.0], "", 1.0, 20.0, true)),
        s("pomodoro.auto", "Start the next on its own", "Off waits for a click", Kind::Switch),
    ] },
    Page { title: "Search and apps", icon: "search", about: "Terminal, web search, screenshots", settings: &[
        s("web_search", "Web search address", "{} is what you typed, empty hides it", Kind::Text),
        s("terminal", "Terminal", "Opens terminal apps", Kind::Text),
        s("screenshot_dir", "Screenshot folder", "Where captures are saved", Kind::Text),
    ] },
    Page { title: "Wallpaper", icon: "wallpaper", about: "Folder and commands", settings: &[
        s("wallpaper.dir", "Folder", "Empty hides the picker", Kind::Text),
        s("wallpaper.set", "Set command", "{path} is the file", Kind::Text),
        s("wallpaper.thumb", "Thumbnail path", "{file} is the file's name", Kind::Text),
        s("wallpaper.size", "Monitor size", "Like 2560x1440, empty detects it", Kind::Text),
        s("wallpaper.on_change", "After a change", "{image} is a full-size still", Kind::Text),
        s("commands.wallpaper_pick", "Picker button", "Used without the picker", Kind::Text),
        s("commands.wallpaper_next", "Next command", "The button's right click", Kind::Text),
        s("commands.wallpaper_toggle", "Pause command", "The button's middle click", Kind::Text),
        s("commands.wallpaper_opt", "Options command", "wall opt, for fit and speed", Kind::Text),
    ] },
    Page { title: "Commands", icon: "terminal", about: "What buttons run", settings: &[
        s("commands.lock", "Lock", "The power menu's Lock", Kind::Text),
        s("commands.mixer", "Mixer", "Right click on volume", Kind::Text),
        s("commands.wifi", "Network settings", "Used without the Wi-Fi picker", Kind::Text),
        s("commands.wifi_alt", "Network, right click", "Anything you like", Kind::Text),
        s("commands.brightness_get", "Read brightness", "Prints 0 to 100, empty detects it", Kind::Text),
        s("commands.brightness_set", "Set brightness", "{} is the percentage", Kind::Text),
    ] },
    Page { title: "Keys", icon: "keyboard", about: "Keys inside search", settings: &[
        s("keys.up", "Up", "Previous row", Kind::Chords),
        s("keys.down", "Down", "Next row", Kind::Chords),
        s("keys.page_up", "Page up", "A screen back", Kind::Chords),
        s("keys.page_down", "Page down", "A screen on", Kind::Chords),
        s("keys.accept", "Choose", "Use the row", Kind::Chords),
        s("keys.close", "Close", "Step back, then close", Kind::Chords),
        s("keys.next_tool", "Next tab", "Tool to the right", Kind::Chords),
        s("keys.prev_tool", "Previous tab", "Tool to the left", Kind::Chords),
        s("keys.clear", "Clear", "Empty the field", Kind::Chords),
        s("keys.delete_word", "Delete a word", "Back to the last space", Kind::Chords),
    ] },
    Page { title: "Claude Code", icon: "star_shine", about: "Permission cards and notes", settings: &[
        s("claude.approve", "Permission cards", "Allow or deny from the island", Kind::Switch),
        s("claude.notify", "Session notes", "When one finishes or needs you", Kind::Switch),
    ] },
];

/// `[[custom]]` and `[[tool]]`: what one is called, its page, and each one's fields.
struct Tables {
    key: &'static str,
    one: &'static str,
    title: &'static str,
    about: &'static str,
    fields: &'static [Setting],
}

const TABLES: &[Tables] = &[
    Tables { key: "custom", one: "module", title: "Your modules", about: "A command's output on the island", fields: &[
        s("name", "Name", "Placed in Layout as custom:name", Kind::Text),
        s("exec", "Command", "Its first line shows, after a restart", Kind::Text),
        s("interval", "Run every", "0 runs it once", num(&[0.0, 5.0, 30.0, 60.0, 300.0, 600.0, 3600.0], " s", 0.0, 604800.0, true)),
        s("on_click", "On click", "A command", Kind::Text),
    ] },
    Tables { key: "tool", one: "search tab", title: "Your search tabs", about: "A searchable list from two commands", fields: &[
        s("name", "Name", "Placed in Layout, Search tabs", Kind::Text),
        s("glyph", "Icon", "An icon's name, or any glyph", Kind::Text),
        s("hint", "Hint", "Shown in the empty field", Kind::Text),
        s("list", "List command", "One item a line", Kind::Text),
        s("run", "Run command", "{} is the item", Kind::Text),
    ] },
];

/// The one a saved override of a key is called, which editing that key supersedes.
pub fn overrides(key: &[String]) -> Vec<&'static str> {
    match key.join(".").as_str() {
        "ui.scale" => vec!["scale"],
        "ui.blur" => vec!["blur"],
        "ui.opacity" => vec!["opacity"],
        "ui.solid_icons" => vec!["icons"],
        "ui.hover_open" => vec!["hover"],
        "disable" => prefs::PARTS.iter().map(|p| p.0).collect(),
        _ => Vec::new(),
    }
}

fn glyph(name: &str) -> Icon {
    Icon::Glyph(name.into())
}

fn words(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    c.next().map_or(String::new(), |first| first.to_uppercase().chain(c).collect())
}

/// A module or tool by the name it's called in the config, as a person would say it.
fn named(name: &str) -> String {
    if let Some(custom) = name.strip_prefix("custom:") {
        return words(custom);
    }
    prefs::PARTS.iter().find(|p| p.0 == name).map_or_else(|| words(name), |p| p.1.to_owned())
}

fn number(v: f64, unit: &str) -> String {
    match unit {
        "×" => format!("{:.0}%", v * 100.0),
        _ => format!("{v}{unit}"),
    }
}

fn num_json(v: f64, whole: bool) -> Json {
    if whole { json!(v as i64) } else { json!(v) }
}

fn back() -> Item {
    Item::new("Back", glyph("arrow_back"), Act::Back)
}

/// `up` steps back out of that many pages once it's saved.
fn change(changes: Vec<Change>, up: usize) -> Act {
    Act::Edit(Edit { changes, up })
}

fn reset(key: &[String], up: usize) -> Item {
    Item::new("Use the default", glyph("restart_alt"), change(vec![(key.to_vec(), None)], up))
}

fn typed(title: &str, label: &str, key: &[String], now: &str, input: Input, up: usize) -> Item {
    let field = Field { key: key.to_vec(), input, up };
    Item::new(title, glyph("edit"), Act::Type { label: label.to_owned(), value: now.to_owned(), field })
}

struct Build {
    /// As written, with what was chosen over it, but before anything switched off is taken out.
    cfg: Config,
    values: Json,
    style: Style,
}

impl Build {
    fn value(&self, key: &[String]) -> &Json {
        key.iter().fold(&self.values, |at, part| part.parse::<usize>().map_or_else(|_| &at[part.as_str()], |i| &at[i]))
    }

    fn options(&self, from: Options) -> Vec<String> {
        let (pill, panel, sidebar) = crate::modules::names();
        let custom = self.cfg.custom.iter().map(|c| format!("custom:{}", c.name));
        let own = |list: Vec<&str>| list.into_iter().map(str::to_owned).collect::<Vec<_>>();
        match from {
            Options::Pill => own(pill).into_iter().chain(custom).collect(),
            Options::Panel => own(panel).into_iter().chain(custom).collect(),
            Options::Sidebar => own(sidebar).into_iter().chain(custom).collect(),
            Options::Visualizer => own(vec!["bars", "wave", "ring"]),
            Options::Tools => super::TOOLS.iter().map(|t| t.name.to_owned()).filter(|n| n != "settings")
                .chain(self.cfg.tool.iter().map(|t| t.name.clone())).collect(),
        }
    }

    fn order(&self, key: &[String], from: Options) -> Vec<Item> {
        let list: Vec<String> = self.value(key).as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect();
        let write = |list: Vec<String>, up: usize| change(vec![(key.to_vec(), Some(json!(list)))], up);
        let mut rows = vec![back()];
        for (i, name) in list.iter().enumerate() {
            let mut each = vec![back()];
            if i > 0 {
                let mut moved = list.clone();
                moved.swap(i, i - 1);
                each.push(Item::new("Move earlier", glyph("keyboard_arrow_up"), write(moved, 0)));
            }
            if i + 1 < list.len() {
                let mut moved = list.clone();
                moved.swap(i, i + 1);
                each.push(Item::new("Move later", glyph("keyboard_arrow_down"), write(moved, 0)));
            }
            let mut without = list.clone();
            without.remove(i);
            each.push(Item::new("Remove", glyph("close"), write(without, 1)));
            let row = Item::new(named(name), Icon::None, Act::Open(each)).tail(Tail::Value(format!("{}", i + 1)));
            rows.push(if self.cfg.off(name) { row.sub("Switched off in Add-ons") } else { row });
        }
        for name in self.options(from).into_iter().filter(|o| !list.contains(o)) {
            let mut with = list.clone();
            with.push(name.clone());
            rows.push(Item::new(format!("Add {}", named(&name)), Icon::None, write(with, 0)).tail(Tail::Value("+".into())));
        }
        rows.push(reset(key, 0));
        rows
    }

    fn row(&self, key: &[String], setting: &Setting) -> Item {
        let Setting { title, help, kind, .. } = *setting;
        let v = self.value(key);
        let text = v.as_str().unwrap_or_default();
        let set = |v: Json, up: usize| change(vec![(key.to_vec(), Some(v))], up);
        let choice = |label: String, on: bool, v: Json| {
            let row = Item::new(label, Icon::None, set(v, 1));
            if on { row.tail(Tail::Value("✓".into())) } else { row }
        };
        let (act, tail) = match kind {
            Kind::Switch => {
                let on = v.as_bool().unwrap_or_default();
                return Item::new(title, Icon::None, set(json!(!on), 0)).sub(help).tail(Tail::Switch(on));
            }
            Kind::Pick(all) => {
                let mut rows = vec![back()];
                rows.extend(all.iter().map(|o| choice(words(o), text == *o, json!(o))));
                (Act::Open(rows), words(text))
            }
            Kind::Style => {
                let mut rows = vec![back()];
                rows.extend(Style::ALL.map(|o| choice(words(o.name()), o == self.style, json!(o.name()))));
                (Act::Open(rows), words(self.style.name()))
            }
            Kind::Num { steps, unit, min, max, whole } => {
                let now = v.as_f64().unwrap_or_default();
                let mut rows = vec![back()];
                rows.extend(steps.iter().map(|&o| choice(number(o, unit), (o - now).abs() < 1e-6, num_json(o, whole))));
                let label = format!("{title}: {min} to {max}");
                rows.push(typed("Type a number", &label, key, &format!("{now}"), Input::Number { min, max, whole }, 1));
                rows.push(reset(key, 1));
                (Act::Open(rows), number(now, unit))
            }
            Kind::Opacity => {
                let now = v.as_f64();
                let auto = Item::new("Follows the style", Icon::None, change(vec![(key.to_vec(), None)], 1));
                let mut rows = vec![back(), if now.is_none() { auto.tail(Tail::Value("✓".into())) } else { auto }];
                rows.extend([1.0, 0.9, 0.8, 0.7, 0.6, 0.5].map(|o| choice(number(o, "×"), now == Some(o), json!(o))));
                rows.push(typed("Type a number", "Opacity: 0.3 to 1", key, "", Input::Number { min: 0.3, max: 1.0, whole: false }, 1));
                (Act::Open(rows), now.map_or("Follows the style".into(), |o| number(o, "×")))
            }
            Kind::Text | Kind::Palette => {
                let input = if matches!(kind, Kind::Palette) { Input::Palette } else { Input::Text };
                let rows = vec![back(), typed("Type it", title, key, text, input, 1).sub(text), reset(key, 1)];
                (Act::Open(rows), if text.is_empty() { "Not set".to_owned() } else { text.to_owned() })
            }
            Kind::Chords => {
                let now = v.as_array().into_iter().flatten().filter_map(Json::as_str).collect::<Vec<_>>().join(", ");
                let rows = vec![back(), typed("Type them", &format!("{title}: keys, with commas"), key, &now, Input::Chords, 1).sub(now.clone()), reset(key, 1)];
                (Act::Open(rows), if now.is_empty() { "None".to_owned() } else { now })
            }
            Kind::Order(from) => {
                let n = v.as_array().map_or(0, Vec::len);
                (Act::Open(self.order(key, from)), format!("{n} shown"))
            }
        };
        Item::new(title, Icon::None, act).sub(help).tail(Tail::Value(tail))
    }

    fn addons(&self) -> Vec<Item> {
        let mut rows = vec![back()];
        for (title, about, name) in [("Light setup", "Only the basics", "light"), ("Full setup", "Every add-on back on", "full")] {
            let Some(all) = prefs::preset(name) else { continue };
            let mut off = self.cfg.disable.clone();
            off.retain(|d| !prefs::is_part(d));
            off.extend(all.iter().filter(|p| p.1 == "off").map(|p| p.0.to_owned()));
            rows.push(Item::new(title, glyph("tune"), change(vec![(path("disable"), Some(json!(off)))], 0)).sub(about));
        }
        for (part, title, _) in prefs::PARTS {
            // Notifications have a line of their own as well.
            let daemon = *part == "notifications";
            let on = !self.cfg.off(part) && (!daemon || self.cfg.notifications);
            let mut off = self.cfg.disable.clone();
            off.retain(|d| d != part);
            if on {
                off.push((*part).to_owned());
            }
            let mut changes = vec![(path("disable"), Some(json!(off)))];
            if daemon && !on && !self.cfg.notifications {
                changes.push((path(part), Some(json!(true))));
            }
            rows.push(Item::new(*title, Icon::None, change(changes, 0)).tail(Tail::Switch(on)));
        }
        rows
    }

    fn app_icons(&self) -> Vec<Item> {
        let key = vec!["icons".to_owned()];
        let mut rows = vec![back(), typed("Add an app", "app id = icon name or glyph", &key, "", Input::Pair, 0)];
        let mut apps: Vec<(&String, &String)> = self.cfg.icons.iter().collect();
        apps.sort();
        for (app, icon) in apps {
            let key = vec!["icons".to_owned(), app.clone()];
            let each = vec![back(), typed("Type its icon", app, &key, icon, Input::Text, 1),
                            Item::new("Remove", glyph("close"), change(vec![(key.clone(), None)], 1))];
            rows.push(Item::new(app.clone(), glyph(icon), Act::Open(each)).tail(Tail::More));
        }
        rows
    }

    fn tables(&self, t: &Tables) -> Vec<Item> {
        let mut rows = vec![back()];
        let entries = self.values[t.key].as_array().cloned().unwrap_or_default();
        let key = path(t.key);
        rows.push(typed(&format!("Add a {}", t.one), "Its name", &key, "", Input::New { at: entries.len() }, 0));
        for (i, entry) in entries.iter().enumerate() {
            let at = |field: &str| vec![t.key.to_owned(), i.to_string(), field.to_owned()];
            let mut each = vec![back()];
            each.extend(t.fields.iter().map(|f| self.row(&at(f.key), f)));
            let gone = change(vec![(vec![t.key.to_owned(), i.to_string()], None)], 1);
            each.push(Item::new("Remove", glyph("close"), Act::Confirm("Enter again to remove it".into(), Box::new(gone))));
            rows.push(Item::new(entry["name"].as_str().unwrap_or_default(), Icon::None, Act::Open(each)).tail(Tail::More));
        }
        rows
    }

    fn root(&self) -> Vec<Item> {
        let page = |title: &str, icon: &str, about: &str, rows: Vec<Item>| {
            Item::new(title, glyph(icon), Act::Open(rows)).sub(about).tail(Tail::More)
        };
        let (mut top, mut found) = (Vec::new(), Vec::new());
        for p in PAGES {
            let mut rows = vec![back()];
            for setting in p.settings {
                let row = self.row(&path(setting.key), setting);
                found.push(row.clone().sub(format!("{} · {}", p.title, setting.help)).deep());
                rows.push(row);
            }
            if p.title == "Appearance" {
                rows.push(Item::new("Icons by app", Icon::None, Act::Open(self.app_icons())).sub("Pick any app's icon").tail(Tail::More));
            }
            top.push(page(p.title, p.icon, p.about, rows));
            if p.title == "Layout" {
                let addons = self.addons();
                found.extend(addons.iter().filter(|r| matches!(r.tail, Tail::Switch(_))).map(|r| r.clone().sub("Add-ons").deep()));
                top.push(page("Add-ons", "view_sidebar", "Switch any part off, or go light", addons));
            }
        }
        for t in TABLES {
            let icon = if t.key == "custom" { "star_shine" } else { "search" };
            top.push(page(t.title, icon, t.about, self.tables(t)));
        }
        let file = Config::path();
        if file.exists() {
            let open = Act::Sh(format!("xdg-open {}", quote(&file.to_string_lossy())));
            top.push(Item::new("Open config.toml", glyph("description"), open).sub("Everything here is saved there"));
        }
        top.extend(found);
        top
    }
}

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let mut cfg = Config::written();
    State::load().apply(&mut cfg);
    build(cfg, ctx.style)
}

fn build(cfg: Config, style: Style) -> Vec<Item> {
    Build { values: serde_json::to_value(&cfg).unwrap_or_default(), cfg, style }.root()
}

impl Field {
    /// What was typed, as the changes it comes to.
    pub fn changes(&self, text: &str) -> Result<Vec<Change>, String> {
        let text = text.trim();
        let mut key = self.key.clone();
        let value = match self.input {
            Input::Text => json!(text),
            Input::Palette => {
                crate::theme::reload(text).ok_or("no palette at that path: it needs surface, accent and the other colors")?;
                json!(text)
            }
            Input::Number { min, max, whole } => {
                let n = text.trim_end_matches('%').parse::<f64>().ok().filter(|n| (min..=max).contains(n) && (!whole || n.fract() == 0.0));
                num_json(n.ok_or_else(|| format!("that takes a {}number from {min} to {max}", if whole { "whole " } else { "" }))?, whole)
            }
            Input::Chords => {
                let list: Vec<&str> = text.split(',').map(str::trim).filter(|c| !c.is_empty()).collect();
                if let Some(bad) = list.iter().find(|c| !crate::settings::valid_chord(c)) {
                    return Err(format!("can't read the key {bad}: try ctrl+n or escape"));
                }
                json!(list)
            }
            Input::Pair => {
                let (app, icon) = text.split_once('=').map(|(a, i)| (a.trim(), i.trim()))
                    .filter(|(a, i)| !a.is_empty() && !i.is_empty()).ok_or("write it as app id = icon")?;
                key.push(app.to_lowercase());
                json!(icon)
            }
            Input::New { at } => {
                if text.is_empty() {
                    return Err("it needs a name".into());
                }
                key.push(at.to_string());
                if self.key[0] == "custom" { json!({"name": text, "exec": ""}) } else { json!({"name": text, "list": "", "run": ""}) }
            }
        };
        Ok(vec![(key, Some(value))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(v: &Json, at: String, out: &mut Vec<String>) {
        match v {
            Json::Object(map) if !map.is_empty() => {
                for (k, v) in map {
                    leaves(v, if at.is_empty() { k.clone() } else { format!("{at}.{k}") }, out);
                }
            }
            _ => out.push(at),
        }
    }

    /// The promise of the page: nothing needs the file opened.
    #[test]
    fn every_config_key_has_a_control() {
        let mut keys = Vec::new();
        leaves(&serde_json::to_value(Config::default()).unwrap(), String::new(), &mut keys);
        for key in keys {
            // These four have pages built for them.
            let built = ["custom", "tool", "icons", "disable"].contains(&key.as_str());
            let listed = PAGES.iter().flat_map(|p| p.settings).any(|s| s.key == key);
            assert!(built || listed, "{key} can't be set from Settings");
        }
        let values = serde_json::to_value(Config::default()).unwrap();
        for setting in PAGES.iter().flat_map(|p| p.settings) {
            assert!(values.pointer(&format!("/{}", setting.key.replace('.', "/"))).is_some(), "{} isn't a key", setting.key);
        }
        let sample: Config = toml::from_str("[[custom]]\nname='a'\nexec='b'\n[[tool]]\nname='c'\nlist='d'\nrun='e'\n").unwrap();
        let sample = serde_json::to_value(sample).unwrap();
        for t in TABLES {
            for key in sample[t.key][0].as_object().unwrap().keys() {
                assert!(t.fields.iter().any(|f| f.key == key), "{}.{key} can't be set", t.key);
            }
        }
    }

    fn edits(items: &[Item], out: &mut Vec<Vec<Change>>) {
        for item in items.iter().filter(|i| !i.deep) {
            match &item.act {
                Act::Edit(e) => out.push(e.changes.clone()),
                Act::Open(inner) => edits(inner, out),
                _ => {}
            }
        }
    }

    /// Every button on every page, pressed on a fresh config: each saves something valid.
    #[test]
    fn every_button_writes_a_config_that_loads() {
        let mut all = Vec::new();
        edits(&build(Config::default(), Style::Pill), &mut all);
        assert!(all.len() > 200, "only {}", all.len());
        for changes in all {
            let mut doc = toml::Table::new();
            for (key, value) in &changes {
                let Some(value) = value else { continue };
                let mut at = &mut doc;
                for part in &key[..key.len() - 1] {
                    at = at.entry(part.clone()).or_insert(toml::Value::Table(Default::default())).as_table_mut().unwrap();
                }
                at.insert(key[key.len() - 1].clone(), toml::Value::try_from(value).unwrap());
            }
            assert!(doc.try_into::<Config>().is_ok(), "{changes:?}");
        }
    }

    #[test]
    fn what_is_typed_is_checked() {
        let field = |input| Field { key: path("ui.scale"), input, up: 0 };
        let size = field(Input::Number { min: 0.75, max: 2.0, whole: false });
        assert!(size.changes("1.3").is_ok() && size.changes("9").is_err() && size.changes("NaN").is_err());
        assert!(field(Input::Chords).changes("ctrl+n, escape").is_ok() && field(Input::Chords).changes("hyper+q").is_err());
        assert!(field(Input::Pair).changes("kitty = terminal").is_ok() && field(Input::Pair).changes("kitty").is_err());
        assert!(field(Input::Palette).changes("/nonexistent").is_err());
    }
}
