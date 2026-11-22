//! File search. Live: each query asks plocate (instant, from the system's index)
//! or, without an index, fd over your home folder. Enter opens the file.
use super::{Act, Ctx, Icon, Item, on_path, quote};
use std::{path::Path, process::Command};

const LIMIT: &str = "60";

pub fn usable() -> bool {
    on_path("plocate") || on_path("fd")
}

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let q = ctx.query.trim();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    let home = crate::settings::home();
    let out = if on_path("plocate") && Path::new("/var/lib/plocate/plocate.db").exists() {
        // Every word must match somewhere in the path, like the other searches.
        let mut cmd = Command::new("plocate");
        cmd.args(["-i", "-l", LIMIT]);
        for word in q.split_whitespace() {
            cmd.arg(word);
        }
        cmd.output()
    } else {
        Command::new("fd").args(["-i", "--max-results", LIMIT, "--", q]).arg(&home).output()
    };
    let Ok(out) = out else { return Vec::new() };
    let mut rows: Vec<&str> = std::str::from_utf8(&out.stdout).unwrap_or("").lines().collect();
    // Yours first, then the rest of the system; shorter paths before deep ones.
    let mine = home.to_string_lossy().into_owned();
    rows.sort_by_key(|p| (!p.starts_with(&mine), p.matches('/').count(), p.len()));
    rows.into_iter().map(|path| {
        let p = Path::new(path);
        let name = p.file_name().map_or(path.into(), |n| n.to_string_lossy().into_owned());
        let dir = p.parent().map_or(String::new(), |d| d.to_string_lossy().replacen(&mine, "~", 1));
        let glyph = if p.is_dir() { "folder" } else { "description" };
        Item::new(name, Icon::Glyph(glyph.into()), Act::Sh(format!("xdg-open {}", quote(path)))).sub(dir)
    }).collect()
}
