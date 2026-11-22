//! The island's own settings: looks first, then every add-on with its switch.
//! Type to find one; Enter switches it.
use super::{Act, Ctx, Icon, Item};
use crate::{prefs, style::Style};

fn glyph(name: &str) -> Icon {
    Icon::Glyph(name.into())
}

fn label(key: &str, value: &str) -> String {
    match (key, value) {
        ("scale", v) => format!("{:.0}%", v.parse::<f64>().unwrap_or(1.0) * 100.0),
        (_, "auto") => "Follows the style".into(),
        ("opacity", v) => format!("{:.0}%", v.parse::<f64>().unwrap_or(1.0) * 100.0),
        (_, v) => {
            let mut c = v.chars();
            c.next().map_or(String::new(), |first| first.to_uppercase().chain(c).collect())
        }
    }
}

/// A setting as a list to choose from.
fn choices(title: &str, now: &str, all: Vec<(String, String, String)>) -> Item {
    let mut rows = vec![Item::new("Back", glyph("arrow_back"), Act::Back)];
    let mut shown = String::new();
    for (value, label, cmd) in all {
        let row = Item::new(label.clone(), Icon::None, Act::Setting(cmd));
        if value == now {
            shown = label;
            rows.push(row.sub("current"));
        } else {
            rows.push(row);
        }
    }
    let shown = if shown.is_empty() { now.to_owned() } else { shown };
    Item::new(title, glyph("settings"), Act::Open(rows)).sub(shown)
}

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let styles = Style::ALL.map(|s| (s.name().to_owned(), label("style", s.name()), format!("style {}", s.name())));
    let mut rows = vec![choices("Style", ctx.style.name(), styles.to_vec())];
    for (key, values) in prefs::LOOKS {
        let title = match *key {
            "scale" => "Size",
            "blur" => "Background blur",
            "opacity" => "Opacity",
            "icons" => "Icons",
            _ => "Open on hover",
        };
        let all = values.iter().map(|v| (v.to_string(), label(key, v), format!("set {key} {v}"))).collect();
        rows.push(choices(title, &prefs::get(&ctx.cfg, key).unwrap_or_default(), all));
    }
    for (name, title, what) in [("light", "Light setup", "only the basics"), ("full", "Full setup", "every add-on back on")] {
        rows.push(Item::new(title, glyph("settings"), Act::Setting(format!("preset {name}"))).sub(what));
    }
    for (part, title, _) in prefs::PARTS {
        let on = !ctx.cfg.off(part);
        let act = Act::Setting(format!("set {part} {}", if on { "off" } else { "on" }));
        rows.push(Item::new(*title, if on { glyph("check") } else { Icon::None }, act).sub(if on { "on" } else { "off" }));
    }
    rows
}
