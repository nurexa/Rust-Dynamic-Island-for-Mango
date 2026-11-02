//! Clipboard history. cliphist stores it, so this only needs to search and restore,
//! and the `wl-paste --watch cliphist store` lines keep doing the capturing.
use super::{Act, Ctx, Icon, Item, Task};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const GLYPH: &str = "\u{f0192}";
const IMAGE_GLYPH: &str = "\u{f02e9}";

/// `cliphist list` prints "<id>\t<preview>", newest first.
fn item(line: &str) -> Option<Item> {
    let (_, preview) = line.split_once('\t')?;
    let image = preview.starts_with("[[ binary data");
    let glyph = if image { IMAGE_GLYPH } else { GLYPH };
    Some(Item::new(preview.trim(), Icon::Glyph(glyph.into()), Act::Task(Task::Copy(line.to_owned()))))
}

pub fn list(_: &Ctx) -> Vec<Item> {
    let Ok(out) = Command::new("cliphist").arg("list").stderr(Stdio::null()).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout).lines().filter_map(item).collect()
}

/// `cliphist decode | wl-copy`, wired up directly: the line is clipboard content,
/// which anyone could have put there, so it must never pass through a shell.
pub fn copy(line: &str) {
    let decode = Command::new("cliphist").arg("decode").stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::null()).spawn();
    let Ok(mut decode) = decode else { return };
    if let Some(mut stdin) = decode.stdin.take() {
        let _ = writeln!(stdin, "{line}");
    }
    let Some(out) = decode.stdout.take() else { return };
    let _ = Command::new("wl-copy").stdin(Stdio::from(out)).stderr(Stdio::null()).status();
    let _ = decode.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_cliphist_rows_and_marks_images() {
        let text = item("12\thello\tworld").unwrap();
        assert_eq!(text.title, "hello\tworld");
        assert_eq!(text.act, Act::Task(Task::Copy("12\thello\tworld".into())));
        assert_eq!(item("13\t[[ binary data 2 MiB png 800x600 ]]").unwrap().icon, Icon::Glyph(IMAGE_GLYPH.into()));
        assert!(item("no tab here").is_none());
    }
}
