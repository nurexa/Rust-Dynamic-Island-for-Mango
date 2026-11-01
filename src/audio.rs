//! Default sink volume, event-driven through `pactl subscribe`.
//! ponytail: pactl child instead of libpulse bindings; a few MB of mostly shared
//! pages. Bind libpulse if that child ever shows up on a memory budget.
use crate::Msg;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub device: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Deserialize)]
struct Channel {
    value_percent: String,
}

#[derive(Deserialize)]
struct Sink {
    name: String,
    description: Option<String>,
    mute: bool,
    volume: HashMap<String, Channel>,
}

fn parse(default: &str, sinks: &str) -> Option<State> {
    let sinks: Vec<Sink> = serde_json::from_str(sinks).ok()?;
    let sink = sinks.into_iter().find(|s| s.name == default)?;
    let volume = sink.volume.values().filter_map(|c| c.value_percent.trim_end_matches('%').parse().ok()).max()?;
    Some(State { device: sink.description.unwrap_or(sink.name), volume, muted: sink.mute })
}

fn read() -> Option<State> {
    let run = |args: &[&str]| {
        let out = Command::new("pactl").args(args).stderr(Stdio::null()).output().ok()?;
        String::from_utf8(out.stdout).ok()
    };
    parse(run(&["get-default-sink"])?.trim(), &run(&["--format=json", "list", "sinks"])?)
}

fn is_relevant(line: &str) -> bool {
    line.contains("on sink #") || line.contains("on server")
}

pub fn spawn(tx: crate::Tx) {
    let (poke, poked) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut last = None;
        while poked.recv().is_ok() {
            // A held volume key fires a burst; read once after it settles.
            while poked.recv_timeout(Duration::from_millis(30)).is_ok() {}
            let now = read();
            if now != last && now.is_some() {
                last = now.clone();
                let _ = tx.send(Msg::Audio(now.unwrap_or_default()));
            }
        }
    });
    thread::spawn(move || loop {
        let _ = poke.send(());
        let mut cmd = Command::new("pactl");
        cmd.arg("subscribe").stdout(Stdio::piped()).stderr(Stdio::null());
        // Die with the island, however it goes: quit, crash or session end.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = cmd.spawn();
        if let Ok(mut child) = child {
            if let Some(out) = child.stdout.take() {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if is_relevant(&line) {
                        let _ = poke.send(());
                    }
                }
            }
            let _ = child.wait();
        }
        // The audio server restarted or isn't up yet.
        thread::sleep(Duration::from_secs(3));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_default_sink_and_ignores_stream_noise() {
        let json = r#"[{"name":"a","description":"Other","mute":false,"volume":{"mono":{"value_percent":"5%"}}},
            {"name":"b","description":"Kraken","mute":true,
             "volume":{"front-left":{"value_percent":"48%"},"front-right":{"value_percent":"50%"}}}]"#;
        assert_eq!(parse("b", json), Some(State { device: "Kraken".into(), volume: 50, muted: true }));
        assert_eq!(parse("missing", json), None);
        assert!(is_relevant("Event 'change' on sink #56"));
        assert!(!is_relevant("Event 'change' on sink-input #91"));
    }
}
