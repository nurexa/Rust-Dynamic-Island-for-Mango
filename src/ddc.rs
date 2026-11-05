//! Monitor brightness over DDC/CI with ddcutil, for desktops: one slider per
//! monitor. DDC is slow (a read or write takes 50-500 ms), so monitors are found
//! once in the background, and a drag only ever sends the latest value.
use crate::Msg;
use std::{
    collections::HashMap,
    process::{Command, Stdio},
    sync::{OnceLock, mpsc},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    /// The I2C bus, which is how ddcutil is told which monitor.
    pub bus: u32,
    /// Output name (DP-2) when ddcutil knows the connector, else the model.
    pub name: String,
    pub model: String,
    pub pct: u32,
}

fn ddcutil(args: &[&str]) -> Option<String> {
    let out = Command::new("ddcutil").args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `ddcutil detect --terse`: each display's bus, connector and model.
fn parse_detect(out: &str) -> Vec<(u32, String, String)> {
    let mut found = Vec::new();
    for block in out.split("Display ").skip(1) {
        let field = |key: &str| block.lines().find_map(|l| l.trim().strip_prefix(key)).map(|v| v.trim().to_owned());
        let Some(bus) = field("I2C bus:").and_then(|b| b.rsplit('-').next()?.parse().ok()) else { continue };
        let connector = field("DRM connector:").map(|c| c.split_once('-').map_or(c.clone(), |(_, rest)| rest.to_owned()));
        let model = field("Monitor:").map(|m| m.split(':').nth(1).unwrap_or(&m).to_owned()).unwrap_or_default();
        found.push((bus, connector.unwrap_or_else(|| model.clone()), model));
    }
    found
}

/// `getvcp 10 --terse` prints "VCP 10 C <current> <max>".
fn parse_vcp(out: &str) -> Option<u32> {
    let f: Vec<&str> = out.split_whitespace().collect();
    let (cur, max): (f64, f64) = (f.get(3)?.parse().ok()?, f.get(4)?.parse().ok()?);
    (max > 0.0).then(|| (100.0 * cur / max).round() as u32)
}

pub fn spawn(tx: crate::Tx) {
    if !crate::tools::on_path("ddcutil") {
        return;
    }
    thread::spawn(move || {
        let screens: Vec<Screen> = parse_detect(&ddcutil(&["detect", "--terse"]).unwrap_or_default()).into_iter()
            .filter_map(|(bus, name, model)| {
                let pct = parse_vcp(&ddcutil(&["--bus", &bus.to_string(), "getvcp", "10", "--terse"])?)?;
                Some(Screen { bus, name, model, pct })
            }).collect();
        if !screens.is_empty() {
            let _ = tx.send(Msg::Ddc(screens));
        }
    });
}

static SETTER: OnceLock<mpsc::Sender<(u32, u32)>> = OnceLock::new();

/// Set a monitor's brightness. Values that pile up while one write is in flight
/// collapse to the newest per monitor.
pub fn set(bus: u32, pct: u32) {
    let tx = SETTER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(u32, u32)>();
        thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut want = HashMap::from([first]);
                while let Ok((b, p)) = rx.recv_timeout(Duration::from_millis(40)) {
                    want.insert(b, p);
                }
                for (b, p) in want {
                    ddcutil(&["--bus", &b.to_string(), "--noverify", "setvcp", "10", &p.to_string()]);
                }
            }
        });
        tx
    });
    let _ = tx.send((bus, pct));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ddcutil_output() {
        let detect = "Display 1\n   I2C bus:  /dev/i2c-4\n   DRM connector:  card1-DP-2\n   Monitor:  AOC:Q27G2G4:1234\n\n\
                      Display 2\n   I2C bus:  /dev/i2c-6\n   Monitor:  DEL:U2720Q:99\n\nInvalid display\n   I2C bus: /dev/i2c-9\n";
        assert_eq!(parse_detect(detect), [(4, "DP-2".into(), "Q27G2G4".into()), (6, "U2720Q".into(), "U2720Q".into())]);
        assert_eq!(parse_vcp("VCP 10 C 50 100"), Some(50));
        assert_eq!(parse_vcp("VCP 10 C 30 60"), Some(50));
        assert_eq!(parse_vcp("VCP 10 ERR"), None);
    }
}
