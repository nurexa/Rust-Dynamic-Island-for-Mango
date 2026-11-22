//! What can be changed from the island itself: a few looks, and every part on or off.
//! A choice is saved beside the state and holds until that setting's line in
//! config.toml changes, the same as a style. This is `island set`; Settings writes the file.
use crate::settings::{Blur, Config};

/// What's switched, by key, with what it can be. The first spelling is canonical.
pub const LOOKS: &[(&str, &[&str])] = &[
    ("scale", &["0.75", "0.85", "1", "1.1", "1.25", "1.5", "1.75", "2"]),
    ("blur", &["auto", "on", "off"]),
    ("opacity", &["auto", "1", "0.9", "0.8", "0.7", "0.6", "0.5"]),
    ("icons", &["solid", "outline"]),
    ("hover", &["on", "off"]),
];

/// Every add-on: the name `disable` takes, what it's called, and whether a light setup keeps it.
pub const PARTS: &[(&str, &str, bool)] = &[
    ("header", "Panel header", true),
    ("workspaces", "Workspaces", true),
    ("clock", "Clock", true),
    ("title", "Focused app's name", false),
    ("network", "Network indicator", true),
    ("status", "Tray and connection", true),
    ("battery", "Battery", true),
    ("audio", "Volume", true),
    ("brightness", "Brightness", false),
    ("stats", "CPU, RAM and network stats", false),
    ("media", "Media player", false),
    ("visualizer", "Music visualizers", false),
    ("notifications", "Notifications", true),
    ("sidebar", "Sidebar", false),
    ("calendar", "Calendar", false),
    ("weather", "Weather", false),
    ("about", "System info", false),
    ("controls", "Quick controls", false),
    ("nightlight", "Night light", false),
    ("bluetooth", "Bluetooth", false),
    ("timer", "Pomodoro timer", false),
    ("claude", "Claude Code status", false),
    ("privacy", "Mic and camera indicator", false),
    ("wallpaper", "Wallpapers", false),
    ("apps", "App launcher", true),
    ("run", "Run a command", true),
    ("windows", "Window switcher", false),
    ("files", "File search", false),
    ("clipboard", "Clipboard history", false),
    ("screenshot", "Screenshots", false),
    ("wifi", "Wi-Fi picker", false),
    ("power", "Power menu", true),
    ("calc", "Calculator", false),
    ("web", "Web search", false),
];

pub fn is_part(name: &str) -> bool {
    PARTS.iter().any(|p| p.0 == name)
}

pub fn get(cfg: &Config, key: &str) -> Option<String> {
    let on = |b: bool| if b { "on" } else { "off" }.to_owned();
    let ui = &cfg.ui;
    Some(match key {
        "scale" => ui.scale.to_string(),
        "blur" => match ui.blur {
            Blur::Auto => "auto".into(),
            Blur::On => "on".into(),
            Blur::Off => "off".into(),
        },
        "opacity" => ui.opacity.map_or("auto".into(), |a| a.to_string()),
        "icons" => if ui.solid_icons { "solid" } else { "outline" }.into(),
        "hover" => on(ui.hover_open),
        part if is_part(part) => on(!cfg.off(part)),
        _ => return None,
    })
}

/// False if the key or the value means nothing.
pub fn put(cfg: &mut Config, key: &str, value: &str) -> bool {
    let number = |lo: f64, hi: f64| value.parse::<f64>().ok().filter(|v| (lo..=hi).contains(v));
    let switch = match value {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };
    let ui = &mut cfg.ui;
    match key {
        "scale" => number(crate::scale::MIN, crate::scale::MAX).map(|v| ui.scale = v).is_some(),
        "blur" => match value {
            "auto" => Some(Blur::Auto),
            "on" => Some(Blur::On),
            "off" => Some(Blur::Off),
            _ => None,
        }.map(|v| ui.blur = v).is_some(),
        "opacity" if value == "auto" => {
            ui.opacity = None;
            true
        }
        "opacity" => number(0.3, 1.0).map(|v| ui.opacity = Some(v)).is_some(),
        "icons" => match value {
            "solid" => Some(true),
            "outline" => Some(false),
            _ => None,
        }.map(|v| ui.solid_icons = v).is_some(),
        "hover" => switch.map(|v| ui.hover_open = v).is_some(),
        part if is_part(part) => switch.map(|keep| {
            cfg.disable.retain(|d| d != part);
            if !keep {
                cfg.disable.push(part.to_owned());
            }
        }).is_some(),
        _ => false,
    }
}

/// The whole of a preset, as the choices it's made of.
pub fn preset(name: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let light = match name {
        "light" => true,
        "full" => false,
        _ => return None,
    };
    Some(PARTS.iter().map(|&(part, _, kept)| (part, if kept || !light { "on" } else { "off" })).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_value_offered_is_taken_and_read_back() {
        let mut cfg = Config::default();
        for (key, values) in LOOKS {
            for value in *values {
                assert!(put(&mut cfg, key, value), "{key} {value}");
                assert_eq!(get(&cfg, key).as_deref(), Some(*value), "{key}");
            }
            assert!(!put(&mut cfg, key, "sideways"), "{key}");
        }
        assert!(!put(&mut cfg, "scale", "9") && !put(&mut cfg, "scale", "NaN") && !put(&mut cfg, "nonsense", "on"));
    }

    #[test]
    fn light_leaves_a_working_bar_and_full_brings_everything_back() {
        let mut cfg = Config::default();
        for (key, value) in preset("light").unwrap() {
            assert!(put(&mut cfg, key, value));
        }
        cfg.finish();
        assert!(cfg.pill.contains(&"workspaces".into()) && cfg.pill.contains(&"clock".into()));
        assert!(cfg.sidebar.is_empty() && cfg.visualizer.is_empty() && !cfg.calc() && !cfg.tools.contains(&"files".into()));
        let mut cfg = Config::default();
        for (key, value) in preset("full").unwrap() {
            assert!(put(&mut cfg, key, value));
        }
        assert!(cfg.disable.is_empty());
    }

    #[test]
    fn the_installers_light_list_is_this_ones() {
        let sh = include_str!("../install.sh");
        let line = sh.lines().find_map(|l| l.strip_prefix("LIGHT_OFF='")).unwrap().trim_end_matches('\'');
        let off: Vec<&str> = PARTS.iter().filter(|p| !p.2).map(|p| p.0).collect();
        assert_eq!(line.split(' ').collect::<Vec<_>>(), off);
    }

    #[test]
    fn every_default_module_can_be_switched_off() {
        let cfg = Config::default();
        for name in [&cfg.pill, &cfg.panel, &cfg.sidebar, &cfg.tools].into_iter().flatten() {
            assert!(is_part(name), "{name} has no switch");
        }
    }
}
