//! Screenshots with grim and slurp: saved, copied to the clipboard, and shown in the
//! island with a preview. Annotating goes through satty when it's installed.
use super::{Act, Ctx, Icon, Item, Task, on_path};
use crate::{Msg, Tx, notify::{Note, NoteImage}, settings::Config, theme::expand};
use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Output,
    Window,
    Region,
    Annotate,
    Color,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "output" => Kind::Output,
            "window" => Kind::Window,
            "region" => Kind::Region,
            "annotate" => Kind::Annotate,
            "color" => Kind::Color,
            _ => return None,
        })
    }
}

pub fn list(_: &Ctx) -> Vec<Item> {
    let row = |title, sub, glyph: &str, kind| {
        Item::new(title, Icon::Glyph(glyph.into()), Act::Task(Task::Shot(kind))).sub(sub)
    };
    let mut items = vec![
        row("Region", "Drag out an area", "\u{f0a6a}", Kind::Region),
        row("Window", "The focused window", "\u{f05b2}", Kind::Window),
        row("Screen", "Everything on this output", "\u{f0379}", Kind::Output),
    ];
    if on_path("satty") {
        items.push(row("Annotate", "Drag out an area, then draw on it", "\u{f03eb}", Kind::Annotate));
    }
    items.push(row("Pick a color", "Copy the color under the cursor", "\u{f0485}", Kind::Color));
    items
}

fn slurp(args: &[&str]) -> Option<String> {
    let out = Command::new("slurp").args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn say(tx: &Tx, title: &str, body: &str, image: NoteImage) {
    let _ = tx.send(Msg::Notify(Note::local("screenshot", title, body, image)));
}

fn pick_color(tx: &Tx) {
    let Some(point) = slurp(&["-p"]) else { return };
    let Ok(out) = Command::new("grim").args(["-g", &point, "-t", "ppm", "-"]).output() else { return };
    // A 1x1 PPM ends in that pixel's three bytes.
    let [.., r, g, b] = out.stdout[..] else { return };
    let hex = format!("#{r:02x}{g:02x}{b:02x}");
    let _ = Command::new("wl-copy").arg(&hex).status();
    say(tx, "Color copied", &hex, NoteImage::None);
}

fn geometry(kind: Kind, ctx: &Ctx) -> Option<Option<String>> {
    match kind {
        Kind::Output => Some(None),
        Kind::Window => {
            let w = ctx.clients.iter().find(|c| c.is_focused)?;
            Some(Some(format!("{},{} {}x{}", w.x, w.y, w.width, w.height)))
        }
        _ => slurp(&["-d"]).map(Some),
    }
}

fn file_name(cfg: &Config) -> Option<std::path::PathBuf> {
    let dir = expand(&cfg.screenshot_dir);
    fs::create_dir_all(&dir).ok()?;
    let stamp = pango::glib::DateTime::now_local().ok()?.format("%F-%H%M%S").ok()?;
    Some(dir.join(format!("{stamp}.png")))
}

pub fn take(kind: Kind, ctx: &Ctx, tx: &Tx) {
    // Let the palette finish closing, so it isn't in the shot.
    thread::sleep(Duration::from_millis(350));
    if kind == Kind::Color {
        return pick_color(tx);
    }
    let Some(file) = file_name(&ctx.cfg) else { return };
    let Some(area) = geometry(kind, ctx) else { return };
    let mut grim = Command::new("grim");
    if let Some(area) = &area {
        grim.args(["-g", area]);
    }
    if !grim.arg(&file).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
        return say(tx, "Screenshot failed", "grim couldn't capture the screen", NoteImage::None);
    }
    if kind == Kind::Annotate {
        let _ = Command::new("satty").arg("--filename").arg(&file).arg("--output-filename").arg(&file)
            .args(["--actions-on-enter", "save-to-file", "--early-exit"]).status();
    }
    if let Ok(png) = fs::File::open(&file) {
        let _ = Command::new("wl-copy").args(["--type", "image/png"]).stdin(png).status();
    }
    let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
    say(tx, "Screenshot saved", &name, NoteImage::Path(file));
}
