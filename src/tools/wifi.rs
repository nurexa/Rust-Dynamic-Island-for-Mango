//! Wi-Fi through nmcli: list networks, connect, ask for a password when needed.
use super::{Act, Ctx, Icon, Item, Task, Then};
use crate::{Msg, Tx, notify::{Note, NoteImage}};
use std::{
    collections::HashSet,
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Join(String),
    Radio(bool),
}

fn say(tx: &Tx, text: String) {
    let _ = tx.send(Msg::Notify(Note::local("wifi", "Wi-Fi", &text, NoteImage::None)));
}

fn nmcli(args: &[&str]) -> Option<String> {
    let out = Command::new("nmcli").args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn signal_glyph(signal: u32) -> &'static str {
    match signal {
        75.. => "\u{f0928}",
        50.. => "\u{f0925}",
        25.. => "\u{f0922}",
        _ => "\u{f091f}",
    }
}

/// Rows of `nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID`. One SSID can be several access
/// points, and the connected one has to win the dedupe, so it's taken first.
fn parse(src: &str, saved: &HashSet<String>) -> Vec<Item> {
    let mut rows: Vec<(bool, u32, bool, String)> = src.lines().filter_map(|line| {
        let mut f = line.splitn(4, ':');
        let (inuse, signal, security) = (f.next()?, f.next()?, f.next()?);
        let ssid = f.next()?.replace("\\:", ":");
        let secured = !security.is_empty() && security != "--";
        Some((inuse == "*", signal.parse().ok()?, secured, ssid)).filter(|r| !r.3.is_empty())
    }).collect();
    rows.sort_by_key(|r| !r.0);
    let mut seen = HashSet::new();
    rows.into_iter().filter(|r| seen.insert(r.3.clone())).map(|(inuse, signal, secured, ssid)| {
        let needs_password = secured && !saved.contains(&ssid);
        let sub = match (inuse, secured) {
            (true, _) => "Connected",
            (_, true) => "Secured",
            _ => "Open",
        };
        let act = if needs_password {
            Act::Ask { label: format!("Password for {ssid}"), password: true, then: Then::WifiPassword(ssid.clone()) }
        } else {
            Act::Task(Task::Wifi(Pick::Join(ssid.clone())))
        };
        Item::new(ssid, Icon::Glyph(signal_glyph(signal).into()), act).sub(sub)
    }).collect()
}

pub fn list(_: &Ctx) -> Vec<Item> {
    if nmcli(&["-g", "WIFI", "radio"]).is_some_and(|s| s.trim() != "enabled") {
        return vec![Item::new("Turn Wi-Fi on", Icon::Glyph("\u{f0928}".into()), Act::Task(Task::Wifi(Pick::Radio(true))))];
    }
    let saved = nmcli(&["-t", "-f", "NAME", "connection", "show"]).unwrap_or_default();
    let saved: HashSet<String> = saved.lines().map(str::to_owned).collect();
    let rows = nmcli(&["-t", "-f", "IN-USE,SIGNAL,SECURITY,SSID", "device", "wifi", "list", "--rescan", "auto"]);
    let mut items = parse(&rows.unwrap_or_default(), &saved);
    items.push(Item::new("Turn Wi-Fi off", Icon::Glyph("\u{f092d}".into()), Act::Task(Task::Wifi(Pick::Radio(false)))));
    items
}

pub fn connect(pick: Pick, tx: &Tx) {
    match pick {
        Pick::Radio(on) => drop(nmcli(&["radio", "wifi", if on { "on" } else { "off" }])),
        Pick::Join(ssid) => {
            let known = nmcli(&["-t", "-f", "NAME", "connection", "show"]).is_some_and(|l| l.lines().any(|n| n == ssid));
            let ok = if known { nmcli(&["connection", "up", "id", &ssid]) } else { nmcli(&["device", "wifi", "connect", &ssid]) };
            say(tx, format!("{} {ssid}", if ok.is_some() { "Connected to" } else { "Couldn't connect to" }));
        }
    }
}

/// `--ask` reads the password from stdin, which keeps it out of `ps`.
pub fn connect_with_password(ssid: &str, password: &str, tx: &Tx) {
    let child = Command::new("nmcli").args(["--ask", "device", "wifi", "connect", ssid])
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    if child.wait().is_ok_and(|s| s.success()) {
        return say(tx, format!("Connected to {ssid}"));
    }
    // A failed try leaves a saved profile with the wrong password behind.
    nmcli(&["connection", "delete", "id", ssid]);
    say(tx, format!("Couldn't connect to {ssid}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_access_point_wins_the_dedupe_and_colons_survive() {
        let src = " :95:WPA2:Home\n*:80:WPA2:Home\n :40:--:Cafe\\:Free\n :30:WPA2:Home\n :10:WPA3:Far\n :5:WPA2:\n";
        let saved = HashSet::from(["Home".to_owned()]);
        let items = parse(src, &saved);
        let names: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(names, ["Home", "Cafe:Free", "Far"]);
        assert_eq!(items[0].sub, "Connected");
        assert_eq!(items[1].act, Act::Task(Task::Wifi(Pick::Join("Cafe:Free".into()))));
        assert!(matches!(&items[2].act, Act::Ask { password: true, .. }), "unsaved and secured asks first");
    }
}
