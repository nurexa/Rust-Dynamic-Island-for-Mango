//! MangoWC backend. mmsg is a thin client over a line protocol on
//! $MANGO_INSTANCE_SIGNATURE, so we talk to the socket directly: one connection
//! per `watch` stream, and a fresh one per dispatch.
use super::{Backend, Client, Monitor};
use crate::Msg;
use serde::Deserialize;
use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    thread,
};

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
fn dispatch(cmd: &str) {
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
                // Mango repeats itself several times per change; skip the parse.
                let mut last = String::new();
                for line in lines.map_while(Result::ok) {
                    if line == last {
                        continue;
                    }
                    match parse(&line) {
                        Ok(msg) => drop(tx.send(msg)),
                        Err(e) => eprintln!("island: bad {what} update: {e}"),
                    }
                    last = line;
                }
            }
            Err(e) => eprintln!("island: can't watch {what}: {e}"),
        }
        // The stream only ends with the compositor; so do we.
        let _ = tx.send(Msg::Quit);
    });
}

pub struct Mango;

impl Backend for Mango {
    fn watch(&self, tx: crate::Tx) {
        stream("all-monitors", tx.clone(), |l| Ok(Msg::Monitors(serde_json::from_str::<Monitors>(l)?.monitors)));
        stream("all-clients", tx, |l| Ok(Msg::Clients(serde_json::from_str::<Clients>(l)?.clients)));
    }

    fn view(&self, tag: u32, monitor: &str) {
        dispatch(&format!("viewcrossmon,{tag},{monitor}"));
    }

    fn send_to(&self, tag: u32) {
        dispatch(&format!("tagsilent,{tag}"));
    }

    fn focus(&self, window: u64) {
        dispatch(&format!("focusid client,{window}"));
    }

    fn cycle(&self, forward: bool) {
        dispatch(if forward { "viewtoright_have_client" } else { "viewtoleft_have_client" });
    }

    fn quit(&self) {
        dispatch("quit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wm::clients_on;

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
