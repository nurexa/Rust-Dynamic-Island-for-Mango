//! Display brightness as a percentage. Built-in panels are found under
//! /sys/class/backlight and set with `brightnessctl`; anything else (a monitor over
//! DDC, say) is two commands in `[commands]`. No backend means no slider.
use crate::settings::Commands;
use std::{fs, process::Command};

fn sysfs() -> Option<u32> {
    let dir = fs::read_dir("/sys/class/backlight").ok()?.flatten().next()?.path();
    let n = |f: &str| fs::read_to_string(dir.join(f)).ok()?.trim().parse::<f64>().ok();
    let max = n("max_brightness").filter(|m| *m > 0.0)?;
    Some((100.0 * n("brightness")? / max).round() as u32)
}

/// True when the percentage comes from a file read, which is cheap enough to repeat.
pub fn is_cheap(c: &Commands) -> bool {
    c.brightness_get.is_empty()
}

pub fn read(c: &Commands) -> Option<u32> {
    if c.brightness_get.is_empty() {
        return sysfs();
    }
    let out = Command::new("sh").args(["-c", &c.brightness_get]).output().ok()?;
    parse(&String::from_utf8_lossy(&out.stdout))
}

/// First number on the first line, so `ddcutil getvcp` style output still works as long as the command trims it.
fn parse(out: &str) -> Option<u32> {
    let n: String = out.lines().next()?.trim().chars().take_while(char::is_ascii_digit).collect();
    n.parse::<u32>().ok().map(|v| v.min(100))
}

/// The shell command that sets `pct`, or None when there is nothing to set.
pub fn set_cmd(c: &Commands, pct: u32) -> Option<String> {
    if !c.brightness_set.is_empty() {
        return Some(c.brightness_set.replace("{}", &pct.to_string()));
    }
    // 1% floor: a fully black backlight panel is hard to recover from.
    sysfs().map(|_| format!("brightnessctl -q set {}%", pct.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_percentage_and_fills_the_setter() {
        assert_eq!(parse("42\n"), Some(42));
        assert_eq!(parse("250"), Some(100));
        assert_eq!(parse("n/a"), None);
        let c = Commands { brightness_set: "ddcutil setvcp 10 {}".into(), ..Commands::default() };
        assert_eq!(set_cmd(&c, 30).as_deref(), Some("ddcutil setvcp 10 30"));
    }
}
