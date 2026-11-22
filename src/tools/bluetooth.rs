//! Bluetooth through BlueZ: connect paired devices, scan for and pair new ones,
//! turn the adapter on and off. src/bluetooth.rs watches it for the panel.
use super::{Act, Ctx, Icon, Item, Task};
use crate::{
    Msg, Tx,
    bluetooth::{self as bt, ADAPTER, DEVICE, Device, Tree},
    notify::{Note, NoteImage},
};
use std::{thread, time::Duration};
use zbus::blocking::Connection;

const SCAN: Duration = Duration::from_secs(15);
const BT: &str = "bluetooth";

#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    /// Object path, name.
    Connect(String, String),
    Disconnect(String, String),
    Pair(String, String),
    /// Adapter path, on.
    Power(String, bool),
}

fn say(tx: &Tx, text: String) {
    let _ = tx.send(Msg::Notify(Note::local("bluetooth", "Bluetooth", &text, NoteImage::None)));
}

fn glyph(icon: &str) -> Icon {
    let g = match icon {
        "audio-headset" => "headset_mic",
        "audio-headphones" => "headphones",
        "audio-card" | "audio-speakers" => "speaker",
        "input-keyboard" => "keyboard",
        "input-mouse" | "input-tablet" => "mouse",
        "input-gaming" => "sports_esports",
        "phone" => "mobile",
        "computer" => "computer",
        _ => BT,
    };
    Icon::Glyph(g.into())
}

fn device(d: &Device) -> Item {
    let (path, name) = (d.path.clone(), d.name.clone());
    let (pick, sub) = if d.connected {
        let sub = d.battery.map_or("Connected".into(), |b| format!("Connected · {b}%"));
        (Pick::Disconnect(path, name), sub)
    } else {
        (Pick::Connect(path, name), "Paired".into())
    };
    Item::new(&d.name, glyph(&d.icon), Act::Task(Task::Bluetooth(pick))).sub(sub)
}

pub fn list(_: &Ctx) -> Vec<Item> {
    let Ok(c) = Connection::system() else { return Vec::new() };
    let Some(s) = Tree::new(bt::objects(&c)).state() else { return Vec::new() };
    let power = |on| Act::Task(Task::Bluetooth(Pick::Power(s.adapter.clone(), on)));
    if !s.powered {
        return vec![Item::new("Turn Bluetooth on", Icon::Glyph(BT.into()), power(true))];
    }
    let mut paired: Vec<&Device> = s.devices.iter().filter(|d| d.paired).collect();
    paired.sort_by_key(|d| !d.connected);
    let mut items: Vec<Item> = paired.into_iter().map(device).collect();
    items.push(Item::new("Scan for devices", Icon::Glyph("bluetooth_searching".into()), Act::Load(scan))
        .sub("Looks for new devices for 15 seconds"));
    items.push(Item::new("Turn Bluetooth off", Icon::Glyph("bluetooth_disabled".into()), power(false)));
    items
}

/// A list that takes 15 seconds to build; the palette shows it loading meanwhile.
fn scan(_: &Ctx) -> Vec<Item> {
    let Ok(c) = Connection::system() else { return Vec::new() };
    let Some(adapter) = Tree::new(bt::objects(&c)).state().map(|s| s.adapter) else { return Vec::new() };
    // Discovery belongs to this connection, so BlueZ ends it if we die mid-scan.
    let started = bt::call(&c, &adapter, ADAPTER, "StartDiscovery").is_ok();
    thread::sleep(SCAN);
    // Read before stopping: BlueZ forgets signal strengths when discovery ends,
    // and that's what tells a device in range from one it merely remembers.
    let found = Tree::new(bt::objects(&c)).found();
    if started {
        let _ = bt::call(&c, &adapter, ADAPTER, "StopDiscovery");
    }
    if found.is_empty() {
        return vec![Item::new("Nothing found", Icon::Glyph(BT.into()), Act::Back)
            .sub("Put the device in pairing mode and scan again")];
    }
    found.iter().map(|d| {
        Item::new(&d.name, glyph(&d.icon), Act::Task(Task::Bluetooth(Pick::Pair(d.path.clone(), d.name.clone()))))
            .sub("Pair and connect")
    }).collect()
}

pub fn run(pick: Pick, tx: &Tx) {
    let Some(c) = bt::connect() else { return say(tx, "Can't reach BlueZ".into()) };
    match pick {
        Pick::Power(adapter, on) => {
            if bt::set(&c, &adapter, ADAPTER, "Powered", on).is_err() {
                say(tx, format!("Couldn't turn Bluetooth {}", if on { "on" } else { "off" }));
            }
        }
        Pick::Connect(path, name) => {
            let ok = bt::call(&c, &path, DEVICE, "Connect").is_ok();
            say(tx, format!("{} {name}", if ok { "Connected to" } else { "Couldn't connect to" }));
        }
        Pick::Disconnect(path, name) => {
            if bt::call(&c, &path, DEVICE, "Disconnect").is_err() {
                say(tx, format!("Couldn't disconnect {name}"));
            }
        }
        Pick::Pair(path, name) => {
            // Already paired is fine: then this just connects it.
            let paired = match bt::call(&c, &path, DEVICE, "Pair") {
                Err(zbus::Error::MethodError(e, ..)) => e.as_str() == "org.bluez.Error.AlreadyExists",
                r => r.is_ok(),
            };
            if !paired {
                return say(tx, format!("Couldn't pair with {name}"));
            }
            // Trusted lets it reconnect by itself next time.
            let _ = bt::set(&c, &path, DEVICE, "Trusted", true);
            let ok = bt::call(&c, &path, DEVICE, "Connect").is_ok();
            say(tx, format!("{} {name}", if ok { "Paired and connected to" } else { "Paired, but couldn't connect to" }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_devices_offer_disconnect_and_show_their_battery() {
        let mut d = Device { path: "/d".into(), name: "Buds".into(), icon: "audio-headset".into(), paired: true,
                             connected: true, battery: Some(40) };
        let item = device(&d);
        assert_eq!(item.sub, "Connected · 40%");
        assert_eq!(item.act, Act::Task(Task::Bluetooth(Pick::Disconnect("/d".into(), "Buds".into()))));
        assert_eq!(item.icon, Icon::Glyph("headset_mic".into()));
        d.connected = false;
        assert_eq!(device(&d).act, Act::Task(Task::Bluetooth(Pick::Connect("/d".into(), "Buds".into()))));
    }
}
