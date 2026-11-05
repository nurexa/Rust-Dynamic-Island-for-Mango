//! Claude Code on the island. Claude Code runs `island claude-hook` on its hook
//! events; the hook hands each event to the running island over its socket. The
//! pill shows what each session is doing, permission prompts become Allow/Deny
//! cards, and a finished session you weren't looking at leaves a note.
//!
//! Only PermissionRequest waits for an answer. Every other hook is registered
//! async, so the island can never slow Claude down, and if the island isn't
//! running the hook exits at once and Claude carries on as usual.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

/// How long a permission card waits before leaving it to the terminal.
const ASK_FOR: Duration = Duration::from_secs(300);
const HOOK: &str = "island claude-hook";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Working,
    /// Waiting on you: a permission prompt or an idle prompt.
    Waiting,
    /// Just finished: shows for DONE_FOR, then the session goes quiet.
    Done,
    /// Open but waiting for your next prompt; not on the pill.
    Idle,
}

pub const DONE_FOR: Duration = Duration::from_secs(20);

/// Whether a session is on the pill right now.
pub fn shown(s: &Session, now: Instant) -> bool {
    match s.state {
        State::Working | State::Waiting => true,
        State::Done => now < s.since + DONE_FOR,
        State::Idle => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    /// The project folder's name.
    pub project: String,
    pub state: State,
    pub since: Instant,
    /// Process ids from the hook up to init: one of them is the terminal's window.
    pub pids: Vec<u32>,
}

/// One hook event as the island sees it.
#[derive(Debug, PartialEq)]
pub struct Event {
    pub kind: String,
    pub session: String,
    pub project: String,
    pub pids: Vec<u32>,
    pub raw: Value,
}

impl Event {
    pub fn parse(line: &str) -> Option<Event> {
        let raw: Value = serde_json::from_str(line).ok()?;
        let s = |k: &str| raw.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let cwd = s("cwd");
        let project = std::path::Path::new(&cwd).file_name().map_or(cwd.clone(), |n| n.to_string_lossy().into_owned());
        let pids = raw.get("island_pids").and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|p| p.as_u64().map(|p| p as u32)).collect()).unwrap_or_default();
        Some(Event { kind: s("hook_event_name"), session: s("session_id"), project, pids, raw })
    }

    /// "Bash: cargo test", "Edit: src/main.rs", in a line or two.
    pub fn ask_text(&self) -> (String, String) {
        let tool = self.raw.get("tool_name").and_then(Value::as_str).unwrap_or("a tool");
        let input = self.raw.get("tool_input");
        let field = |k: &str| input.and_then(|i| i.get(k)).and_then(Value::as_str);
        let detail = field("command").or_else(|| field("file_path")).or_else(|| field("url"))
            .or_else(|| field("pattern")).or_else(|| field("description")).unwrap_or("");
        let detail: String = detail.lines().next().unwrap_or("").chars().take(160).collect();
        (format!("Claude wants to use {tool} · {}", self.project), detail)
    }

    pub fn can_remember(&self) -> bool {
        self.raw.get("permission_suggestions").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
    }

    /// What the hook prints for an answer from a card: allow, always or deny.
    pub fn decision(&self, answer: &str) -> Option<String> {
        let decision = match answer {
            "allow" => json!({"behavior": "allow"}),
            "always" => json!({"behavior": "allow", "updatedPermissions": self.raw.get("permission_suggestions").cloned().unwrap_or(json!([]))}),
            "deny" => json!({"behavior": "deny", "message": "Denied from the island"}),
            _ => return None,
        };
        Some(json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}}).to_string())
    }

    /// The session's state after this event, or None when the session ends.
    pub fn state(&self) -> Option<State> {
        match self.kind.as_str() {
            "SessionEnd" => None,
            "Stop" => Some(State::Done),
            "PermissionRequest" => Some(State::Waiting),
            "Notification" if self.raw.get("notification_type").and_then(Value::as_str) == Some("idle_prompt") => Some(State::Idle),
            "Notification" => Some(State::Waiting),
            "SessionStart" => Some(State::Idle),
            _ => Some(State::Working),
        }
    }

    pub fn last_message(&self) -> String {
        let m = self.raw.get("last_assistant_message").and_then(Value::as_str).unwrap_or("");
        m.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(200).collect()
    }
}

/// Our pid and every ancestor's, so the island can find the terminal window.
fn ancestry() -> Vec<u32> {
    let mut pids = Vec::new();
    let mut pid = std::process::id();
    while pid > 1 && pids.len() < 32 {
        pids.push(pid);
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        pid = stat.rsplit(')').next().and_then(|s| s.split_whitespace().nth(1)).and_then(|p| p.parse().ok()).unwrap_or(0);
    }
    pids
}

fn shorten(v: &mut Value) {
    match v {
        Value::String(s) if s.len() > 2000 => *s = s.chars().take(500).collect(),
        Value::Array(a) => a.iter_mut().for_each(shorten),
        Value::Object(o) => o.values_mut().for_each(shorten),
        _ => {}
    }
}

/// `island claude-hook`: forward the event on stdin; for a permission request,
/// print the island's answer. Never fails loudly: Claude must work without us.
pub fn hook() -> ! {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let Ok(mut event) = serde_json::from_str::<Value>(&input) else { std::process::exit(0) };
    // A Write's whole file can be in here; the island only ever shows a line of it.
    shorten(&mut event);
    event["island_pids"] = json!(ancestry());
    let asks = event.get("hook_event_name").and_then(Value::as_str) == Some("PermissionRequest");
    let talk = || -> std::io::Result<String> {
        let mut s = UnixStream::connect(crate::socket_path())?;
        writeln!(s, "claude\t{event}")?;
        let mut reply = String::new();
        if asks {
            s.set_read_timeout(Some(ASK_FOR))?;
            BufReader::new(s).read_line(&mut reply)?;
        }
        Ok(reply)
    };
    if let Ok(reply) = talk() {
        if !reply.trim().is_empty() {
            println!("{}", reply.trim());
        }
    }
    std::process::exit(0)
}

fn settings_path() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map_or(crate::settings::home().join(".claude"), PathBuf::from).join("settings.json")
}

/// Add (or with `remove`, take out) the island's hooks in ~/.claude/settings.json.
/// Only entries whose command is ours are touched; the rest of the file is kept.
pub fn setup(bin: &str, remove: bool) -> Result<String, String> {
    let path = settings_path();
    let src = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let mut doc: Value = serde_json::from_str(&src).map_err(|e| format!("{} isn't valid JSON ({e}); left alone", path.display()))?;
    let out = edit(&mut doc, bin, remove);
    if !remove && !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(|e| e.to_string())?;
    } else if !remove {
        let _ = std::fs::copy(&path, path.with_extension("json.island-backup"));
    }
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(out)
}

fn edit(doc: &mut Value, bin: &str, remove: bool) -> String {
    let ours = |h: &Value| h.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(HOOK) || c.ends_with("island claude-hook"));
    if !doc.is_object() {
        *doc = json!({});
    }
    let hooks = doc.as_object_mut().map(|o| o.entry("hooks").or_insert(json!({}))).filter(|h| h.is_object());
    let Some(hooks) = hooks else { return "settings.json has a hooks value that isn't an object; left alone".into() };
    // Drop ours everywhere first, so a re-run never doubles up.
    for (_, groups) in hooks.as_object_mut().into_iter().flatten() {
        if let Some(groups) = groups.as_array_mut() {
            for g in groups.iter_mut() {
                if let Some(list) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                    list.retain(|h| !ours(h));
                }
            }
            groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|l| !l.is_empty()));
        }
    }
    let map = hooks.as_object_mut().expect("checked");
    map.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if remove {
        return "Removed the island's Claude Code hooks".into();
    }
    let command = format!("{bin} claude-hook");
    for event in ["SessionStart", "UserPromptSubmit", "PostToolUse", "Notification", "Stop", "SessionEnd", "PermissionRequest"] {
        let mut hook = json!({"type": "command", "command": command});
        if event == "PermissionRequest" {
            hook["timeout"] = json!(ASK_FOR.as_secs() + 10);
        } else {
            hook["async"] = json!(true);
        }
        let group = json!({"hooks": [hook]});
        map.entry(event).or_insert(json!([])).as_array_mut().map(|a| a.push(group));
    }
    "Added the island's hooks to Claude Code".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_merge_into_settings_and_come_out_clean() {
        let mut doc = json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}});
        edit(&mut doc, "/bin/island", false);
        edit(&mut doc, "/bin/island", false);
        let stop = doc["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "theirs kept, ours added once");
        assert_eq!(doc["hooks"]["PermissionRequest"][0]["hooks"][0]["async"], Value::Null, "the one that answers blocks");
        assert_eq!(doc["hooks"]["PostToolUse"][0]["hooks"][0]["async"], json!(true));
        edit(&mut doc, "/bin/island", true);
        assert_eq!(doc, json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}}));
    }

    #[test]
    fn events_become_states_cards_and_answers() {
        let e = Event::parse(r#"{"hook_event_name":"PermissionRequest","session_id":"s1","cwd":"/home/u/island",
            "tool_name":"Bash","tool_input":{"command":"cargo test\nmore"},"island_pids":[9,8],
            "permission_suggestions":[{"type":"addRules","behavior":"allow"}]}"#).unwrap();
        assert_eq!((e.state(), e.project.as_str(), e.pids.as_slice()), (Some(State::Waiting), "island", &[9, 8][..]));
        assert_eq!(e.ask_text().1, "cargo test");
        assert!(e.can_remember());
        let always: Value = serde_json::from_str(&e.decision("always").unwrap()).unwrap();
        assert_eq!(always["hookSpecificOutput"]["decision"]["updatedPermissions"][0]["type"], "addRules");
        assert_eq!(e.decision("dismissed"), None, "no answer leaves it to the terminal");
        let idle = Event::parse(r#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#).unwrap();
        assert_eq!(idle.state(), Some(State::Idle));
        assert_eq!(Event::parse(r#"{"hook_event_name":"SessionEnd"}"#).unwrap().state(), None);
    }
}
