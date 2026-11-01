//! ~/.config/island/config is hand-edited and optional. ~/.local/state/island/state
//! is written by the island: where each pill sits, and pin and tuck.
use std::{collections::HashMap, env, fs, path::PathBuf};

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

fn state_path() -> PathBuf {
    env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state"))
        .join("island/state")
}

pub struct Config {
    pub font: String,
    pub clock: String,
    /// Keep tiled windows out from under the pill while it's docked.
    pub reserve: bool,
}

impl Config {
    pub fn load() -> Self {
        let mut c = Config { font: "Adwaita Sans".into(), clock: "%I:%M %p".into(), reserve: true };
        let dir = env::var_os("XDG_CONFIG_HOME").map_or(home().join(".config"), PathBuf::from);
        let src = fs::read_to_string(dir.join("island/config")).unwrap_or_default();
        for line in src.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            match line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("font", v)) => c.font = v.into(),
                Some(("clock", v)) => c.clock = v.into(),
                Some(("reserve", v)) => c.reserve = v != "false",
                _ => {}
            }
        }
        c
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct State {
    /// Output name -> pill centre x, pill top y. Absent means docked top-centre.
    pub pos: HashMap<String, (f64, f64)>,
    pub pinned: bool,
    pub tucked: bool,
}

impl State {
    pub fn parse(src: &str) -> Self {
        let mut s = State::default();
        for line in src.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f[..] {
                ["pos", name, x, y] => {
                    if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                        s.pos.insert(name.into(), (x, y));
                    }
                }
                ["pinned"] => s.pinned = true,
                ["tucked"] => s.tucked = true,
                _ => {}
            }
        }
        s
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (name, (x, y)) in &self.pos {
            out += &format!("pos {name} {x} {y}\n");
        }
        if self.pinned {
            out += "pinned\n";
        }
        if self.tucked {
            out += "tucked\n";
        }
        out
    }

    pub fn load() -> Self {
        Self::parse(&fs::read_to_string(state_path()).unwrap_or_default())
    }

    /// Rename into place so a crash mid-write can't lose the old state.
    pub fn save(&self) {
        let path = state_path();
        let tmp = path.with_extension("tmp");
        let written = path.parent().map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&tmp, self.render()))
            .and_then(|_| fs::rename(&tmp, &path));
        if let Err(e) = written {
            eprintln!("island: can't save {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_ignores_garbage() {
        let mut s = State { pinned: true, ..Default::default() };
        s.pos.insert("DP-2".into(), (640.5, 900.0));
        assert_eq!(State::parse(&s.render()), s);
        assert_eq!(State::parse("pos DP-2 x y\nnonsense\n\n"), State::default());
    }
}
