//! Audio, event-driven through `pactl subscribe`: the default output's volume,
//! the devices to pick from, what each app plays at, and who's using a mic.
//! ponytail: pactl child instead of libpulse bindings; a few MB of mostly shared
//! pages. Bind libpulse if that child ever shows up on a memory budget.
use crate::Msg;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

/// Set while the panel's app list is showing: only then do per-app volume
/// changes matter, and some apps change theirs constantly.
pub static DETAILED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub device: String,
    pub volume: u32,
    pub muted: bool,
    /// Outputs and inputs to choose from, and which is the default.
    pub sinks: Vec<Device>,
    pub sources: Vec<Device>,
    pub sink: String,
    pub source: String,
    /// What each app is playing at.
    pub apps: Vec<Stream>,
    /// Apps recording from a microphone (not from a monitor of the output).
    pub recording: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stream {
    pub index: u32,
    pub app: String,
    pub icon: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Deserialize)]
struct Channel {
    value_percent: String,
}

fn percent(v: &HashMap<String, Channel>) -> Option<u32> {
    v.values().filter_map(|c| c.value_percent.trim_end_matches('%').parse().ok()).max()
}

#[derive(Deserialize)]
struct Sink {
    name: String,
    description: Option<String>,
    mute: bool,
    volume: HashMap<String, Channel>,
}

#[derive(Deserialize)]
struct Source {
    index: u32,
    name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct SinkInput {
    index: u32,
    mute: bool,
    volume: HashMap<String, Channel>,
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct SourceOutput {
    source: u32,
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
}

fn prop<'a>(p: &'a HashMap<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    p.get(key)?.as_str().filter(|s| !s.is_empty())
}

fn app_name(p: &HashMap<String, serde_json::Value>) -> String {
    prop(p, "application.name").or_else(|| prop(p, "application.process.binary")).unwrap_or("Unknown").to_owned()
}

/// The raw `pactl --format=json` lists, by name.
#[derive(Default)]
struct Lists {
    sinks: String,
    sources: String,
    sink_inputs: String,
    source_outputs: String,
}

fn parse(default_sink: &str, default_source: &str, l: &Lists) -> Option<State> {
    let sinks: Vec<Sink> = serde_json::from_str(&l.sinks).ok()?;
    let sink = sinks.iter().find(|s| s.name == default_sink)?;
    let label = |name: &str, d: &Option<String>| d.clone().unwrap_or_else(|| name.to_owned());
    // Each extra list is optional: a server that can't give one still has a volume.
    let sources: Vec<Source> = serde_json::from_str(&l.sources).unwrap_or_default();
    let inputs: Vec<SinkInput> = serde_json::from_str(&l.sink_inputs).unwrap_or_default();
    let outputs: Vec<SourceOutput> = serde_json::from_str(&l.source_outputs).unwrap_or_default();
    let mics: HashMap<u32, &Source> = sources.iter().filter(|s| !s.name.ends_with(".monitor")).map(|s| (s.index, s)).collect();
    let mut recording: Vec<String> = outputs.iter()
        .filter(|o| mics.contains_key(&o.source))
        // pavucontrol's level meters read every source while it's open; that isn't recording.
        .filter(|o| prop(&o.properties, "application.id") != Some("org.PulseAudio.pavucontrol"))
        .map(|o| app_name(&o.properties)).collect();
    recording.dedup();
    let mut sources: Vec<Device> = mics.values().map(|s| Device { name: s.name.clone(), label: label(&s.name, &s.description) }).collect();
    sources.sort_by(|a, b| a.label.cmp(&b.label));
    Some(State {
        device: label(&sink.name, &sink.description),
        volume: percent(&sink.volume)?,
        muted: sink.mute,
        sinks: sinks.iter().map(|s| Device { name: s.name.clone(), label: label(&s.name, &s.description) }).collect(),
        sources,
        sink: default_sink.to_owned(),
        source: default_source.to_owned(),
        apps: inputs.iter().map(|i| Stream {
            index: i.index,
            app: app_name(&i.properties),
            icon: prop(&i.properties, "application.icon_name").or_else(|| prop(&i.properties, "application.process.binary")).unwrap_or("").to_owned(),
            volume: percent(&i.volume).unwrap_or(0),
            muted: i.mute,
        }).collect(),
        recording,
    })
}

fn read() -> Option<State> {
    let run = |args: &[&str]| {
        let out = Command::new("pactl").args(args).stderr(Stdio::null()).output().ok()?;
        String::from_utf8(out.stdout).ok()
    };
    let list = |what| run(&["--format=json", "list", what]).unwrap_or_default();
    let lists = Lists { sinks: list("sinks"), sources: list("sources"), sink_inputs: list("sink-inputs"), source_outputs: list("source-outputs") };
    let source = run(&["get-default-source"]).unwrap_or_default();
    parse(run(&["get-default-sink"])?.trim(), source.trim(), &lists)
}

/// Which `pactl subscribe` lines are worth a read. Streams come and go (that's
/// the mic dot and the app list), but their volume changes only matter while shown.
fn is_relevant(line: &str) -> bool {
    let stream = line.contains("on sink-input #") || line.contains("on source-output #");
    if stream {
        return !line.contains("'change'") || DETAILED.load(Ordering::Relaxed);
    }
    line.contains("on sink #") || line.contains("on source #") || line.contains("on server")
}

/// Ask for a fresh read, as if pactl had reported a change.
pub fn poke() {
    if let Some(p) = POKE.get() {
        let _ = p.send(());
    }
}

static POKE: std::sync::OnceLock<mpsc::Sender<()>> = std::sync::OnceLock::new();

pub fn spawn(tx: crate::Tx) {
    let (poke, poked) = mpsc::channel::<()>();
    let _ = POKE.set(poke.clone());
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
        let sinks = r#"[{"name":"a","description":"Other","mute":false,"volume":{"mono":{"value_percent":"5%"}}},
            {"name":"b","description":"Speakers","mute":true,
             "volume":{"front-left":{"value_percent":"48%"},"front-right":{"value_percent":"50%"}}}]"#;
        let l = Lists { sinks: sinks.into(), ..Default::default() };
        let s = parse("b", "", &l).unwrap();
        assert_eq!((s.device.as_str(), s.volume, s.muted, s.sinks.len()), ("Speakers", 50, true, 2));
        assert_eq!(parse("missing", "", &l), None);
        assert!(is_relevant("Event 'change' on sink #56"));
        assert!(!is_relevant("Event 'change' on sink-input #91"), "stream volume only while the list shows");
        assert!(is_relevant("Event 'new' on source-output #91"), "a new recording lights the mic dot");
    }

    #[test]
    fn apps_and_who_records_a_microphone() {
        let l = Lists {
            sinks: r#"[{"name":"b","mute":false,"volume":{"mono":{"value_percent":"50%"}}}]"#.into(),
            sources: r#"[{"index":1,"name":"mic","description":"Headset"},{"index":2,"name":"b.monitor","description":"Monitor"}]"#.into(),
            sink_inputs: r#"[{"index":7,"mute":true,"volume":{"mono":{"value_percent":"30%"}},
                "properties":{"application.name":"Firefox","application.icon_name":"firefox"}}]"#.into(),
            source_outputs: r#"[{"source":1,"properties":{"application.name":"Discord"}},
                {"source":2,"properties":{"application.name":"OBS"}},
                {"source":1,"properties":{"application.id":"org.PulseAudio.pavucontrol"}}]"#.into(),
        };
        let s = parse("b", "mic", &l).unwrap();
        assert_eq!(s.apps, [Stream { index: 7, app: "Firefox".into(), icon: "firefox".into(), volume: 30, muted: true }]);
        assert_eq!(s.recording, ["Discord"], "a monitor isn't a mic, and level meters aren't recording");
        assert_eq!(s.sources.iter().map(|d| d.label.as_str()).collect::<Vec<_>>(), ["Headset"]);
    }
}
