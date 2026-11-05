//! Hyprland backend. Two sockets under $XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE:
//! `.socket2.sock` streams events, one per line, and `.socket.sock` answers one
//! request per connection (`j/clients`, `dispatch workspace 3`). On any event that
//! can move a window or a workspace we ask again for the whole picture; Hyprland
//! answers in well under a millisecond.
use super::{Backend, Client, Monitor, Tag};
use crate::Msg;
use serde::Deserialize;
use std::{
    collections::HashSet,
    env,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::Duration,
};

/// Workspaces always offered, like Mango's tags.
const TAGS: u32 = 10;

fn dir() -> PathBuf {
    let runtime = env::var_os("XDG_RUNTIME_DIR").map_or("/tmp".into(), PathBuf::from);
    runtime.join("hypr").join(env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default())
}

fn request(cmd: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(dir().join(".socket.sock"))?;
    s.write_all(cmd.as_bytes())?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

/// Off the UI thread: a busy compositor must never stall a frame. Several
/// commands go as one batch, so they run in order.
fn dispatch(cmds: &[String]) {
    let batch = cmds.iter().map(|c| format!("dispatch {c}")).collect::<Vec<_>>().join(";");
    thread::spawn(move || match request(&format!("[[BATCH]]{batch}")) {
        Ok(r) if r.split_whitespace().any(|w| w != "ok") => eprintln!("island: {batch}: {}", r.trim()),
        Err(e) => eprintln!("island: {batch}: {e}"),
        _ => {}
    });
}

#[derive(Deserialize)]
struct HMonitor {
    id: i64,
    name: String,
    focused: bool,
    #[serde(rename = "activeWorkspace")]
    active: HRef,
}

#[derive(Deserialize)]
struct HRef {
    id: i64,
}

#[derive(Deserialize)]
struct HWorkspace {
    id: i64,
    #[serde(rename = "monitorID", default)]
    monitor: i64,
}

#[derive(Deserialize)]
struct HClient {
    address: String,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    workspace: HRef,
    monitor: i64,
    class: String,
    title: String,
    pid: i64,
    #[serde(rename = "focusHistoryID", default)]
    focus: i64,
    #[serde(default)]
    at: (i32, i32),
    #[serde(default)]
    size: (i32, i32),
}

fn hex(address: &str) -> u64 {
    u64::from_str_radix(address.trim_start_matches("0x"), 16).unwrap_or(0)
}

/// Hyprland's monitors, workspaces and clients as the island's monitors and tags.
fn convert(monitors: &str, workspaces: &str, clients: &str, urgent: &HashSet<u64>) -> serde_json::Result<(Vec<Monitor>, Vec<Client>)> {
    let monitors: Vec<HMonitor> = serde_json::from_str(monitors)?;
    let workspaces: Vec<HWorkspace> = serde_json::from_str(workspaces)?;
    let clients: Vec<HClient> = serde_json::from_str(clients)?;
    let name_of = |id: i64| monitors.iter().find(|m| m.id == id).map_or(String::new(), |m| m.name.clone());
    let clients: Vec<Client> = clients.into_iter()
        // Special workspaces (scratchpads) have negative ids and aren't tags.
        .filter(|c| c.mapped && c.workspace.id > 0)
        .map(|c| {
            let id = hex(&c.address);
            Client {
                id, pid: c.pid.max(0) as u32, title: Some(c.title), appid: Some(c.class).filter(|s| !s.is_empty()),
                monitor: name_of(c.monitor), tags: vec![c.workspace.id as u32], is_focused: c.focus == 0,
                is_minimized: c.hidden, is_urgent: urgent.contains(&id),
                x: c.at.0, y: c.at.1, width: c.size.0, height: c.size.1,
            }
        }).collect();
    let monitors = monitors.iter().map(|m| {
        let mut ids: Vec<u32> = (1..=TAGS).collect();
        ids.extend(workspaces.iter().filter(|w| w.monitor == m.id && w.id > TAGS as i64).map(|w| w.id as u32));
        let tags = ids.into_iter().map(|index| Tag {
            index,
            is_active: index as i64 == m.active.id,
            is_urgent: clients.iter().any(|c| c.is_urgent && c.tags.contains(&index)),
        }).collect();
        Monitor { name: m.name.clone(), active: m.focused, layout_symbol: String::new(), tags }
    }).collect();
    Ok((monitors, clients))
}

/// Events that change what the pill shows. Titles change constantly and only feed tips.
fn relevant(event: &str) -> bool {
    !matches!(event, "activewindow" | "windowtitle" | "windowtitlev2" | "submap" | "activelayout" | "screencast" | "bell")
}

pub struct Hyprland;

impl Backend for Hyprland {
    fn watch(&self, tx: crate::Tx) {
        let (poke, poked) = mpsc::channel::<Option<u64>>();
        let reader_tx = tx.clone();
        thread::spawn(move || {
            let mut urgent = HashSet::new();
            let mut last = None;
            while let Ok(first) = poked.recv() {
                let mut events = vec![first];
                // A workspace switch is a burst of events; read once it settles.
                while let Ok(e) = poked.recv_timeout(Duration::from_millis(15)) {
                    events.push(e);
                }
                urgent.extend(events.into_iter().flatten());
                let read = (|| Some((request("j/monitors").ok()?, request("j/workspaces").ok()?, request("j/clients").ok()?)))();
                let Some((m, w, c)) = read else { continue };
                match convert(&m, &w, &c, &urgent) {
                    Ok((monitors, clients)) => {
                        // Urgency ends once the window has been focused.
                        urgent.retain(|id| clients.iter().any(|c| c.id == *id && !c.is_focused));
                        let now = Some((monitors.clone(), clients.clone()));
                        if now != last {
                            last = now;
                            let _ = reader_tx.send(Msg::Monitors(monitors));
                            let _ = reader_tx.send(Msg::Clients(clients));
                        }
                    }
                    Err(e) => eprintln!("island: bad Hyprland reply: {e}"),
                }
            }
        });
        thread::spawn(move || {
            let _ = poke.send(None);
            let Ok(s) = UnixStream::connect(dir().join(".socket2.sock")) else {
                eprintln!("island: can't reach Hyprland's event socket");
                return;
            };
            for line in BufReader::new(s).lines().map_while(Result::ok) {
                let (event, data) = line.split_once(">>").unwrap_or((&line, ""));
                if event == "urgent" {
                    let _ = poke.send(Some(hex(data)));
                } else if relevant(event) {
                    let _ = poke.send(None);
                }
            }
            // The stream only ends with the compositor; so do we.
            let _ = tx.send(Msg::Quit);
        });
    }

    fn view(&self, tag: u32, monitor: &str) {
        // Bring the monitor forward first, so a workspace that doesn't exist yet opens there.
        dispatch(&[format!("focusmonitor {monitor}"), format!("workspace {tag}")]);
    }

    fn send_to(&self, tag: u32) {
        dispatch(&[format!("movetoworkspacesilent {tag}")]);
    }

    fn focus(&self, window: u64) {
        dispatch(&[format!("focuswindow address:0x{window:x}")]);
    }

    fn cycle(&self, forward: bool) {
        dispatch(&[if forward { "workspace m+1" } else { "workspace m-1" }.into()]);
    }

    fn quit(&self) {
        dispatch(&["exit".into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_json_becomes_monitors_tags_and_windows() {
        let monitors = r#"[{"id":0,"name":"DP-1","focused":true,"activeWorkspace":{"id":2,"name":"2"}},
                           {"id":1,"name":"HDMI-A-1","focused":false,"activeWorkspace":{"id":12,"name":"12"}}]"#;
        let workspaces = r#"[{"id":2,"monitorID":0},{"id":12,"monitorID":1},{"id":-98,"monitorID":0}]"#;
        let clients = r#"[{"address":"0x5f2a","mapped":true,"hidden":false,"workspace":{"id":2},"monitor":0,"class":"kitty",
                           "title":"t","pid":42,"focusHistoryID":0,"at":[0,0],"size":[10,10]},
                          {"address":"0x10","mapped":true,"workspace":{"id":-98},"monitor":0,"class":"scratch","title":"","pid":1},
                          {"address":"0x11","mapped":true,"workspace":{"id":12},"monitor":1,"class":"firefox","title":"f","pid":7,"focusHistoryID":3}]"#;
        let urgent = HashSet::from([0x11]);
        let (m, c) = convert(monitors, workspaces, clients, &urgent).unwrap();
        assert_eq!(c.len(), 2, "scratchpads stay out");
        assert_eq!((c[0].id, c[0].pid, c[0].is_focused, c[0].monitor.as_str()), (0x5f2a, 42, true, "DP-1"));
        assert!(m[0].active && m[0].tags.iter().find(|t| t.is_active).unwrap().index == 2);
        assert_eq!(m[1].tags.last().map(|t| (t.index, t.is_active, t.is_urgent)), Some((12, true, true)), "a high workspace joins its monitor's tags");
        assert!(!relevant("windowtitle") && relevant("workspace") && relevant("openwindow"));
    }
}
