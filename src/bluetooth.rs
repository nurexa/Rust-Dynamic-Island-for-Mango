//! Bluetooth from BlueZ on the system bus: one GetManagedObjects at start, then
//! only its signals, so nothing runs while nothing changes. A scan makes nearby
//! devices chatter (signal strength, many times a second), so the island only
//! hears about it when something the panel shows has changed.
#![cfg_attr(not(feature = "bluetooth"), allow(dead_code))]
use crate::{
    Msg,
    notify::{Note, NoteImage},
};
use std::{
    collections::{HashMap, HashSet},
    thread,
    time::Duration,
};
use zbus::{
    blocking::{Connection, MessageIterator, connection::Builder},
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const BLUEZ: &str = "org.bluez";
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const OBJECTS: &str = "org.freedesktop.DBus.ObjectManager";
const DBUS: &str = "org.freedesktop.DBus";
/// Warn below this, once, until it charges back past `LOW_CLEAR`.
pub const LOW: u8 = 15;
const LOW_CLEAR: u8 = 20;

type Props = HashMap<String, OwnedValue>;
pub type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub path: String,
    pub name: String,
    /// BlueZ's icon name, such as `audio-headset`.
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub battery: Option<u8>,
}

/// What the panel shows. None in the model means no adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub adapter: String,
    pub powered: bool,
    /// Paired or connected, by name.
    pub devices: Vec<Device>,
}

#[derive(Default)]
struct Dev {
    d: Device,
    /// Reported a name of its own; a scan turns up plenty of anonymous beacons.
    named: bool,
    /// Has a signal strength, which BlueZ only keeps while a scan is seeing it.
    seen: bool,
}

/// BlueZ's object tree, cut down to the few properties shown.
#[derive(Default)]
pub struct Tree {
    adapters: HashMap<String, bool>,
    devices: HashMap<String, Dev>,
}

fn flag(p: &Props, k: &str) -> Option<bool> {
    match &**p.get(k)? {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn text(p: &Props, k: &str) -> Option<String> {
    match &**p.get(k)? {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

impl Tree {
    pub fn new(objects: Objects) -> Self {
        let mut t = Tree::default();
        for (path, ifaces) in &objects {
            for (iface, props) in ifaces {
                t.set(path.as_str(), iface, props);
            }
        }
        t
    }

    /// An interface added, or properties changed.
    fn set(&mut self, path: &str, iface: &str, p: &Props) {
        match iface {
            ADAPTER => {
                let powered = self.adapters.entry(path.to_owned()).or_default();
                *powered = flag(p, "Powered").unwrap_or(*powered);
            }
            DEVICE | BATTERY => {
                let dev = self.devices.entry(path.to_owned()).or_default();
                let d = &mut dev.d;
                d.path = path.to_owned();
                if let Some(name) = text(p, "Alias").or_else(|| text(p, "Name")) {
                    d.name = name;
                }
                dev.named |= p.contains_key("Name");
                dev.seen |= p.contains_key("RSSI");
                d.icon = text(p, "Icon").unwrap_or(std::mem::take(&mut d.icon));
                d.paired = flag(p, "Paired").unwrap_or(d.paired);
                d.connected = flag(p, "Connected").unwrap_or(d.connected);
                if let Some(Value::U8(pct)) = p.get("Percentage").map(|v| &**v) {
                    d.battery = Some(*pct);
                }
            }
            _ => {}
        }
    }

    fn remove(&mut self, path: &str, iface: &str) {
        match iface {
            ADAPTER => drop(self.adapters.remove(path)),
            DEVICE => drop(self.devices.remove(path)),
            BATTERY => {
                if let Some(dev) = self.devices.get_mut(path) {
                    dev.d.battery = None;
                }
            }
            _ => {}
        }
    }

    fn sorted(&self, keep: impl Fn(&Dev) -> bool) -> Vec<Device> {
        let mut out: Vec<Device> = self.devices.values().filter(|d| keep(d)).map(|d| d.d.clone()).collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.path.cmp(&b.path)));
        out
    }

    /// The first adapter; a second dongle is rare enough to ignore.
    pub fn state(&self) -> Option<State> {
        let (adapter, powered) = self.adapters.iter().min_by_key(|(p, _)| *p)?;
        let devices = self.sorted(|d| d.d.paired || d.d.connected);
        Some(State { adapter: adapter.clone(), powered: *powered, devices })
    }

    /// What a scan turned up that isn't paired yet.
    pub fn found(&self) -> Vec<Device> {
        self.sorted(|d| !d.d.paired && d.named && d.seen)
    }
}

pub fn objects(c: &Connection) -> Objects {
    let reply = c.call_method(Some(BLUEZ), "/", Some(OBJECTS), "GetManagedObjects", &());
    reply.ok().and_then(|r| r.body().deserialize().ok()).unwrap_or_default()
}

/// For calls that change things. Pairing can wait on the other device for a while.
pub fn connect() -> Option<Connection> {
    Builder::system().ok()?.method_timeout(Duration::from_secs(60)).build().ok()
}

pub fn call(c: &Connection, path: &str, iface: &str, method: &str) -> zbus::Result<()> {
    c.call_method(Some(BLUEZ), path, Some(iface), method, &()).map(drop)
}

pub fn set(c: &Connection, path: &str, iface: &str, prop: &str, on: bool) -> zbus::Result<()> {
    c.call_method(Some(BLUEZ), path, Some(PROPS), "Set", &(iface, prop, Value::from(on))).map(drop)
}

/// The panel's switch. On a thread of its own: BlueZ can take a while to answer.
pub fn set_power(adapter: String, on: bool) {
    thread::spawn(move || {
        if let Some(c) = connect() {
            let _ = set(&c, &adapter, ADAPTER, "Powered", on);
        }
    });
}

fn note(tx: &crate::Tx, text: String) {
    let _ = tx.send(Msg::Notify(Note::local("bluetooth", "Bluetooth", &text, NoteImage::None)));
}

struct Watch {
    tree: Tree,
    last: Option<State>,
    /// Devices already warned about a low battery.
    warned: HashSet<String>,
}

impl Watch {
    fn publish(&mut self, tx: &crate::Tx) {
        let now = self.tree.state();
        if now == self.last {
            return;
        }
        for d in now.iter().flat_map(|s| &s.devices) {
            match d.battery {
                Some(b) if d.connected && b < LOW => {
                    if self.warned.insert(d.path.clone()) {
                        note(tx, format!("{} battery low: {b}%", d.name));
                    }
                }
                Some(b) if d.connected && b < LOW_CLEAR => {}
                _ => drop(self.warned.remove(&d.path)),
            }
        }
        self.last = now.clone();
        let _ = tx.send(Msg::Bluetooth(now));
    }

    fn signal(&mut self, conn: &Connection, msg: &zbus::Message) {
        let hdr = msg.header();
        let (Some(iface), Some(member)) = (hdr.interface(), hdr.member()) else { return };
        let body = msg.body();
        match (iface.as_str(), member.as_str()) {
            (OBJECTS, "InterfacesAdded") => {
                let Ok((path, ifaces)) = body.deserialize::<(OwnedObjectPath, HashMap<String, Props>)>() else { return };
                for (iface, props) in &ifaces {
                    self.tree.set(path.as_str(), iface, props);
                }
            }
            (OBJECTS, "InterfacesRemoved") => {
                let Ok((path, ifaces)) = body.deserialize::<(OwnedObjectPath, Vec<String>)>() else { return };
                for iface in &ifaces {
                    self.tree.remove(path.as_str(), iface);
                }
            }
            (PROPS, "PropertiesChanged") => {
                let (Some(path), Ok((iface, props, _))) = (hdr.path(), body.deserialize::<(String, Props, Vec<String>)>()) else { return };
                self.tree.set(path.as_str(), &iface, &props);
            }
            // bluetoothd restarted, or went away.
            (DBUS, "NameOwnerChanged") => {
                let Ok((_, _, new)) = body.deserialize::<(String, String, String)>() else { return };
                self.tree = if new.is_empty() { Tree::default() } else { Tree::new(objects(conn)) };
            }
            _ => {}
        }
    }
}

pub fn spawn(tx: crate::Tx) {
    thread::spawn(move || {
        let Ok(conn) = Connection::system() else { return };
        // Listen before reading, so nothing that changes in between is lost.
        let signals = MessageIterator::from(&conn);
        let changed = |iface| format!("type='signal',sender='{BLUEZ}',interface='{PROPS}',member='PropertiesChanged',arg0='{iface}'");
        // Only the interfaces shown: audio transports and players change all the
        // time while music plays.
        for rule in [
            format!("type='signal',sender='{BLUEZ}',interface='{OBJECTS}'"),
            changed(ADAPTER),
            changed(DEVICE),
            changed(BATTERY),
            format!("type='signal',sender='{DBUS}',member='NameOwnerChanged',arg0='{BLUEZ}'"),
        ] {
            let _ = conn.call_method(Some(DBUS), "/org/freedesktop/DBus", Some(DBUS), "AddMatch", &rule);
        }
        let mut w = Watch { tree: Tree::new(objects(&conn)), last: None, warned: HashSet::new() };
        // Sends nothing without an adapter: the model already starts at None.
        w.publish(&tx);
        for msg in signals.flatten() {
            // The iterator also yields replies to this thread's own calls. Reacting
            // to those would loop forever.
            if msg.message_type() != Type::Signal {
                continue;
            }
            w.signal(&conn, &msg);
            w.publish(&tx);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(kv: &[(&str, Value)]) -> Props {
        kv.iter().map(|(k, v)| (k.to_string(), v.try_to_owned().unwrap())).collect()
    }

    fn tree() -> Tree {
        let mut t = Tree::default();
        t.set("/org/bluez/hci0", ADAPTER, &props(&[("Powered", Value::Bool(true)), ("Alias", Value::from("pc"))]));
        let dev = |name: &str, paired: bool, connected: bool| props(&[
            ("Name", Value::from(name)), ("Alias", Value::from(name)), ("Icon", Value::from("audio-headset")),
            ("Paired", Value::Bool(paired)), ("Connected", Value::Bool(connected)), ("RSSI", Value::I16(-60)),
        ]);
        t.set("/org/bluez/hci0/dev_1", DEVICE, &dev("zed buds", true, true));
        t.set("/org/bluez/hci0/dev_1", BATTERY, &props(&[("Percentage", Value::U8(80))]));
        t.set("/org/bluez/hci0/dev_2", DEVICE, &dev("Arc Mouse", true, false));
        t.set("/org/bluez/hci0/dev_3", DEVICE, &dev("Speaker", false, false));
        // An anonymous beacon: only an address-shaped alias.
        t.set("/org/bluez/hci0/dev_4", DEVICE, &props(&[("Alias", Value::from("4A-11-22")), ("RSSI", Value::I16(-90))]));
        t
    }

    #[test]
    fn paired_devices_sort_by_name_and_carry_their_battery() {
        let s = tree().state().unwrap();
        assert!(s.powered);
        let names: Vec<&str> = s.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Arc Mouse", "zed buds"]);
        assert_eq!(s.devices[1].battery, Some(80));
        assert!(s.devices[1].connected);
    }

    #[test]
    fn a_scan_lists_named_unpaired_devices_only() {
        let found: Vec<String> = tree().found().into_iter().map(|d| d.name).collect();
        assert_eq!(found, ["Speaker"]);
    }

    #[test]
    fn changes_merge_and_removals_take_things_away() {
        let mut t = tree();
        t.set("/org/bluez/hci0/dev_1", DEVICE, &props(&[("Connected", Value::Bool(false))]));
        t.set("/org/bluez/hci0", ADAPTER, &props(&[("Discovering", Value::Bool(true))]));
        let s = t.state().unwrap();
        assert!(s.powered, "an unrelated change keeps Powered");
        assert_eq!(s.devices[1].icon, "audio-headset", "and keeps the device's other fields");
        assert!(!s.devices[1].connected);
        t.remove("/org/bluez/hci0/dev_1", BATTERY);
        assert_eq!(t.state().unwrap().devices[1].battery, None);
        t.remove("/org/bluez/hci0", ADAPTER);
        assert_eq!(t.state(), None, "no adapter, nothing to show");
    }

    #[test]
    fn low_battery_warns_once_until_it_charges() {
        let (tx, rx) = smithay_client_toolkit::reexports::calloop::channel::channel();
        let mut w = Watch { tree: tree(), last: None, warned: HashSet::new() };
        let mut notes = 0;
        for pct in [80, 12, 10, 18, 9, 25, 14] {
            w.tree.set("/org/bluez/hci0/dev_1", BATTERY, &props(&[("Percentage", Value::U8(pct))]));
            w.publish(&tx);
            while let Ok(m) = rx.try_recv() {
                notes += matches!(m, Msg::Notify(_)) as i32;
            }
        }
        assert_eq!(notes, 2, "12 warns, 10/18/9 don't, 25 clears, 14 warns again");
    }

    #[test]
    fn reads_a_real_get_managed_objects_shape() {
        let mut objects = Objects::new();
        let path = OwnedObjectPath::try_from("/org/bluez/hci0").unwrap();
        objects.insert(path, HashMap::from([(ADAPTER.to_owned(), props(&[("Powered", Value::Bool(false))]))]));
        let s = Tree::new(objects).state().unwrap();
        assert_eq!((s.adapter.as_str(), s.powered), ("/org/bluez/hci0", false));
    }
}
