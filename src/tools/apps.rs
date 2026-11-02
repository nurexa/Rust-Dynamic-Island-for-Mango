//! Desktop apps. Scanned when the tool opens and dropped when it closes, so nothing
//! stays in memory between uses. Launch counts lift favourites up the list.
use super::{Act, Ctx, Icon, Item};
use crate::{icons::data_dirs, settings::state_dir};
use std::{collections::{HashMap, HashSet}, fs, path::Path};

fn counts_path() -> std::path::PathBuf {
    state_dir().join("launches")
}

fn load_counts() -> HashMap<String, u32> {
    let src = fs::read_to_string(counts_path()).unwrap_or_default();
    src.lines().filter_map(|l| {
        let (n, id) = l.split_once(' ')?;
        Some((id.to_owned(), n.parse().ok()?))
    }).collect()
}

/// Remember a launch. Off the UI thread, and renamed into place like the state file.
pub fn record(id: String) {
    std::thread::spawn(move || {
        let mut counts = load_counts();
        *counts.entry(id).or_default() += 1;
        let body: String = counts.iter().map(|(id, n)| format!("{n} {id}\n")).collect();
        let path = counts_path();
        let tmp = path.with_extension("tmp");
        let _ = path.parent().map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&tmp, body)).and_then(|_| fs::rename(&tmp, &path));
    });
}

/// `%f`-style codes are for file managers; a launcher passes no files.
fn strip_field_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    out.push('%');
                    chars.next();
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Entry {
    name: String,
    comment: String,
    exec: String,
    icon: String,
    terminal: bool,
}

fn parse(src: &str) -> Option<Entry> {
    let mut e = Entry { name: String::new(), comment: String::new(), exec: String::new(), icon: String::new(), terminal: false };
    let (mut in_entry, mut is_app) = (false, false);
    for line in src.lines() {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Name" => e.name = v.trim().to_owned(),
            "Comment" => e.comment = v.trim().to_owned(),
            "Exec" => e.exec = strip_field_codes(v),
            "Icon" => e.icon = v.trim().to_owned(),
            "Terminal" => e.terminal = v.trim() == "true",
            "Type" => is_app = v.trim() == "Application",
            "NoDisplay" | "Hidden" if v.trim() == "true" => return None,
            _ => {}
        }
    }
    (is_app && !e.name.is_empty() && !e.exec.is_empty()).then_some(e)
}

fn scan(dir: &Path, seen: &mut HashSet<String>, counts: &HashMap<String, u32>, terminal: &str, out: &mut Vec<Item>) {
    for file in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = file.path();
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        let id = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        // The first data dir wins, even when it's hiding an entry: that's the spec.
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(e) = fs::read_to_string(&path).ok().as_deref().and_then(parse) else { continue };
        let cmd = if e.terminal { format!("{terminal} -e {}", e.exec) } else { e.exec };
        let icon = if e.icon.is_empty() { Icon::Glyph("\u{f003b}".into()) } else { Icon::Named(e.icon) };
        let launches = counts.get(&id).copied().unwrap_or(0);
        let mut item = Item::new(e.name, icon, Act::Launch { id, cmd }).sub(e.comment);
        // Capped, so a favourite can't outrank a much better match.
        item.boost = (launches as i32 * 12).min(180);
        out.push(item);
    }
}

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let counts = load_counts();
    let (mut seen, mut out) = (HashSet::new(), Vec::new());
    for dir in data_dirs() {
        scan(&dir.join("applications"), &mut seen, &counts, &ctx.cfg.terminal, &mut out);
    }
    out.sort_by_key(|i| (-i.boost, i.title.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_real_visible_apps() {
        let e = parse("[Desktop Entry]\nType=Application\nName=Tool\nExec=tool --new %U\nIcon=tool\n[Desktop Action x]\nName=Wrong\nExec=wrong\n").unwrap();
        assert_eq!((e.name.as_str(), e.exec.as_str()), ("Tool", "tool --new"));
        assert!(parse("[Desktop Entry]\nType=Application\nName=H\nExec=h\nNoDisplay=true\n").is_none());
        assert!(parse("[Desktop Entry]\nType=Link\nName=L\nExec=l\n").is_none());
    }

    #[test]
    fn field_codes_go_and_literal_percents_stay() {
        assert_eq!(strip_field_codes("app %F --x=100%% %i"), "app --x=100%");
    }
}
