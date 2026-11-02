//! Pick a wallpaper from a folder, with thumbnails. The island doesn't set wallpapers
//! itself: `[wallpaper] set` runs whatever you already use.
use super::{Act, Ctx, Icon, Item, quote};
use crate::{modules::wallpaper, theme::expand};
use std::{fs, path::Path};

const VIDEO: [&str; 5] = ["mp4", "mkv", "webm", "mov", "avi"];
const IMAGE: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// "Settings ›" opens Fit, Speed, FPS, Quality and which wallpapers they apply to, each
/// expanding to its values with the current one marked. Needs `wallpaper_opt` and a `wall` that has run.
fn settings(ctx: &Ctx) -> Option<Item> {
    let base = &ctx.cfg.commands.wallpaper_opt;
    let wall = wallpaper::load().filter(|_| !base.is_empty())?;
    let glyph = || Icon::Glyph("\u{f0493}".into());
    let back = || Item::new("Back", Icon::Glyph("\u{f004d}".into()), Act::Back);
    let choice = |key: &str, text: &str, value: &str, on: bool| {
        let item = Item::new(text, Icon::None, Act::Sh(format!("{base} {key} {value}")));
        if on { item.sub("current") } else { item }
    };
    let mut rows = vec![back()];
    for &(label, key, chips) in wallpaper::ROWS {
        let mut values = vec![back()];
        values.extend(chips.iter().map(|&(text, value)| choice(key, text, value, wall.get(key) == value)));
        let now = chips.iter().find(|(_, v)| *v == wall.get(key)).map_or(wall.get(key), |(t, _)| t.to_string());
        rows.push(Item::new(label, glyph(), Act::Open(values)).sub(now));
    }
    let scope = vec![back(), choice("scope", "All wallpapers", "all", wall.scope == "all"),
                     choice("scope", "Only this one", "this", wall.scope == "this"),
                     Item::new("Reset this one", Icon::None, Act::Sh(format!("{base} reset")))];
    rows.push(Item::new("Apply to", glyph(), Act::Open(scope)).sub(if wall.scope == "this" { "only this one" } else { "all wallpapers" }));
    Some(Item::new("Settings", glyph(), Act::Open(rows)).sub("fit, speed, fps, quality"))
}

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let w = &ctx.cfg.wallpaper;
    let mut files: Vec<_> = fs::read_dir(expand(&w.dir)).into_iter().flatten().flatten().map(|f| f.path())
        .filter(|p| { let e = extension(p); VIDEO.contains(&e.as_str()) || IMAGE.contains(&e.as_str()) }).collect();
    files.sort();
    settings(ctx).into_iter().chain(files.into_iter().map(|path| {
        let file = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let thumb = (!w.thumb.is_empty()).then(|| expand(&w.thumb.replace("{file}", &file))).filter(|t| t.exists());
        let still = matches!(extension(&path).as_str(), "png" | "jpg" | "jpeg");
        let icon = match thumb {
            Some(t) => Icon::Image(t),
            None if still => Icon::Image(path.clone()),
            None => Icon::Glyph("\u{f0e09}".into()),
        };
        let cmd = w.set.replace("{path}", &quote(&path.to_string_lossy()));
        Item::new(stem, icon, Act::Sh(cmd))
    })).collect()
}
