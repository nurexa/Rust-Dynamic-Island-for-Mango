//! Mango IPC. mmsg is a thin client over a line protocol on
//! $MANGO_INSTANCE_SIGNATURE, so we talk to the socket directly: one connection
//! per `watch` stream, and a fresh one per dispatch.
use crate::Msg;
use serde::Deserialize;
use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    thread,
};

#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Tag {
    pub index: u32,
    pub is_active: bool,
    pub is_urgent: bool,
}

#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Monitor {
    pub name: String,
    /// True on the monitor mango has selected, not merely enabled.
    pub active: bool,
    pub layout_symbol: String,
    pub tags: Vec<Tag>,
}

#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Client {
    pub id: u64,
    pub title: Option<String>,
    pub appid: Option<String>,
    pub monitor: String,
    #[serde(default)]
    pub tags: Vec<u32>,
    pub is_focused: bool,
    pub is_minimized: bool,
    pub is_urgent: bool,
}

#[derive(Deserialize)]
struct Monitors {
    monitors: Vec<Monitor>,
}

#[derive(Deserialize)]
struct Clients {
    clients: Vec<Client>,
}

fn connect() -> std::io::Result<UnixStream> {
    let path = env::var("MANGO_INSTANCE_SIGNATURE").map_err(std::io::Error::other)?;
    UnixStream::connect(path)
}

/// Mango checks for hangup before reading, so a client that writes and closes
/// straight away has its command dropped: wait for the reply, off the UI thread.
pub fn dispatch(cmd: &str) {
    let cmd = cmd.to_owned();
    thread::spawn(move || {
        let reply = connect().and_then(|mut s| {
            writeln!(s, "dispatch {cmd}")?;
            let mut reply = String::new();
            s.read_to_string(&mut reply)?;
            Ok(reply)
        });
        match reply {
            Ok(r) if r.contains("error") => eprintln!("island: dispatch {cmd}: {}", r.trim()),
            Err(e) => eprintln!("island: dispatch {cmd}: {e}"),
            Ok(_) => {}
        }
    });
}

fn stream(what: &str, tx: crate::Tx, parse: fn(&str) -> serde_json::Result<Msg>) {
    let what = what.to_owned();
    thread::spawn(move || {
        let lines = connect().and_then(|mut s| {
            writeln!(s, "watch {what}")?;
            Ok(BufReader::new(s).lines())
        });
        match lines {
            Ok(lines) => {
                for line in lines.map_while(Result::ok) {
                    match parse(&line) {
                        Ok(msg) => drop(tx.send(msg)),
                        Err(e) => eprintln!("island: bad {what} update: {e}"),
                    }
                }
            }
            Err(e) => eprintln!("island: can't watch {what}: {e}"),
        }
        // The stream only ends with the compositor; so do we.
        let _ = tx.send(Msg::Quit);
    });
}

pub fn watch(tx: crate::Tx) {
    stream("all-monitors", tx.clone(), |l| Ok(Msg::Monitors(serde_json::from_str::<Monitors>(l)?.monitors)));
    stream("all-clients", tx, |l| Ok(Msg::Clients(serde_json::from_str::<Clients>(l)?.clients)));
}

pub fn same_but_titles(a: &[Client], b: &[Client]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| {
        (a.id, &a.appid, &a.monitor, &a.tags, a.is_focused, a.is_minimized, a.is_urgent)
            == (b.id, &b.appid, &b.monitor, &b.tags, b.is_focused, b.is_minimized, b.is_urgent)
    })
}

/// Windows on one tag of one monitor, in mango's order. Minimized windows stay
/// out, matching what the Waybar pills showed.
pub fn clients_on<'a>(clients: &'a [Client], mon: &'a str, tag: u32) -> impl Iterator<Item = &'a Client> {
    clients.iter().filter(move |c| !c.is_minimized && c.monitor == mon && c.tags.contains(&tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_payloads_and_filters_by_monitor_and_tag() {
        let m: Monitors = serde_json::from_str(r#"{"monitors":[{"name":"DP-2","active":true,"is_hdr":false,
            "layout_symbol":"DW","tags":[{"index":1,"is_active":true,"is_urgent":false,"layout":"DW","client_count":1}]}]}"#).unwrap();
        assert_eq!(m.monitors[0].tags[0].index, 1);
        let c: Clients = serde_json::from_str(r#"{"clients":[
            {"id":49,"title":"t","appid":"discord","monitor":"DP-2","tags":[1],"is_focused":true,"is_minimized":false,"is_urgent":false},
            {"id":50,"appid":null,"monitor":"DP-2","tags":[1,3],"is_focused":false,"is_minimized":true,"is_urgent":false},
            {"id":51,"monitor":"HDMI-1","tags":[1],"is_focused":false,"is_minimized":false,"is_urgent":false}]}"#).unwrap();
        let ids: Vec<u64> = clients_on(&c.clients, "DP-2", 1).map(|c| c.id).collect();
        assert_eq!(ids, [49]);
    }
}
