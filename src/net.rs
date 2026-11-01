//! Connection status from NetworkManager over D-Bus. Signal strength changes
//! constantly, so it's only refreshed while a panel is open (see `poke`).
use crate::Msg;
use std::{sync::mpsc, thread};
use zbus::{
    blocking::{Connection, MessageIterator},
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue},
};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Unknown,
    Offline,
    Wifi { ssid: String, strength: u8 },
    Wired { name: String },
    Other { name: String },
}

fn get<T: TryFrom<OwnedValue>>(c: &Connection, path: &str, iface: &str, prop: &str) -> Option<T> {
    let reply = c.call_method(Some(NM), path, Some("org.freedesktop.DBus.Properties"), "Get", &(iface, prop)).ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    T::try_from(value).ok()
}

fn path_of(c: &Connection, path: &str, iface: &str, prop: &str) -> Option<String> {
    let p: OwnedObjectPath = get(c, path, iface, prop)?;
    let p: ObjectPath = p.into();
    Some(p.to_string()).filter(|p| p != "/")
}

fn read(c: &Connection) -> State {
    let Some(active) = path_of(c, NM_PATH, NM, "PrimaryConnection") else { return State::Offline };
    let iface = "org.freedesktop.NetworkManager.Connection.Active";
    let name: String = get(c, &active, iface, "Id").unwrap_or_default();
    match get::<String>(c, &active, iface, "Type").as_deref() {
        Some("802-11-wireless") => {
            let strength = (|| {
                let devices: Vec<OwnedObjectPath> = get(c, &active, iface, "Devices")?;
                let dev = devices.first()?.to_string();
                let ap = path_of(c, &dev, "org.freedesktop.NetworkManager.Device.Wireless", "ActiveAccessPoint")?;
                get::<u8>(c, &ap, "org.freedesktop.NetworkManager.AccessPoint", "Strength")
            })();
            State::Wifi { ssid: name, strength: strength.unwrap_or(0) }
        }
        Some("802-3-ethernet") => State::Wired { name },
        _ => State::Other { name },
    }
}

/// Returns the poke used to refresh signal strength while a panel is open.
pub fn spawn(tx: crate::Tx) -> mpsc::Sender<()> {
    let (poke, poked) = mpsc::channel();
    let signals = poke.clone();
    thread::spawn(move || {
        let Ok(conn) = Connection::system() else { return };
        let bus = conn.clone();
        thread::spawn(move || {
            let rule = format!("type='signal',sender='{NM}',path='{NM_PATH}',interface='org.freedesktop.DBus.Properties'");
            if bus.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus",
                               Some("org.freedesktop.DBus"), "AddMatch", &rule).is_err() {
                return;
            }
            // The iterator also sees replies to read()'s own calls; reacting to
            // those would loop forever.
            for msg in MessageIterator::from(&bus).flatten() {
                if msg.message_type() == zbus::message::Type::Signal {
                    let _ = signals.send(());
                }
            }
        });
        let mut last = None;
        let _ = publish(&conn, &tx, &mut last);
        while poked.recv().is_ok() {
            while poked.try_recv().is_ok() {}
            let _ = publish(&conn, &tx, &mut last);
        }
    });
    poke
}

fn publish(conn: &Connection, tx: &crate::Tx, last: &mut Option<State>) -> Result<(), ()> {
    let now = read(conn);
    if last.as_ref() != Some(&now) {
        *last = Some(now.clone());
        tx.send(Msg::Net(now)).map_err(drop)?;
    }
    Ok(())
}

