//! Pick a wallpaper from a folder, with thumbnails. The island doesn't set wallpapers
//! itself: `[wallpaper] set` runs whatever you already use.
use super::{Act, Ctx, Icon, Item, quote};
use crate::theme::expand;
use std::{fs, path::Path};

const VIDEO: [&str; 5] = ["mp4", "mkv", "webm", "mov", "avi"];
const IMAGE: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let w = &ctx.cfg.wallpaper;
    let mut files: Vec<_> = fs::read_dir(expand(&w.dir)).into_iter().flatten().flatten().map(|f| f.path())
        .filter(|p| { let e = extension(p); VIDEO.contains(&e.as_str()) || IMAGE.contains(&e.as_str()) }).collect();
    files.sort();
    files.into_iter().map(|path| {
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
    }).collect()
}
