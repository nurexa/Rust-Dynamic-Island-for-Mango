//! Session D-Bus: media through playerctld, and the system tray. Waybar owned
//! org.kde.StatusNotifierWatcher, so with Waybar gone this has to serve that name
//! as well as host items, or apps would have nowhere to register.
use crate::Msg;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use zbus::{
    Message,
    blocking::{Connection, MessageIterator, connection::Builder},
    fdo::RequestNameFlags,
    message::{Header, Type},
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Structure, Value},
};

const DBUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const PLAYER: &str = "org.mpris.MediaPlayer2.playerctld";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";

type Props = HashMap<String, OwnedValue>;

fn get_all(c: &Connection, dest: &str, path: &str, iface: &str) -> Option<Props> {
    let reply = c.call_method(Some(dest), path, Some(PROPS), "GetAll", &(iface,)).ok()?;
    reply.body().deserialize().ok()
}

fn string(p: &Props, key: &str) -> Option<String> {
    String::try_from(p.get(key)?.try_clone().ok()?).ok()
}

// ---- Media ----

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub length: f64,
    pub position: f64,
    pub read_at: Instant,
    pub art: Option<PathBuf>,
}

impl Track {
    /// Seconds in, projected from the last read so no polling is needed.
    pub fn now(&self) -> f64 {
        let p = self.position + if self.playing { self.read_at.elapsed().as_secs_f64() } else { 0.0 };
        if self.length > 0.0 { p.min(self.length) } else { p }
    }
}

fn int(v: Option<&Value<'_>>) -> f64 {
    match v {
        Some(Value::I64(n)) => *n as f64,
        Some(Value::U64(n)) => *n as f64,
        _ => 0.0,
    }
}

fn track(p: &Props) -> Option<Track> {
    let meta: HashMap<String, OwnedValue> = p.get("Metadata")?.try_clone().ok()?.try_into().ok()?;
    let status = string(p, "PlaybackStatus")?;
    let title = string(&meta, "xesam:title").filter(|t| !t.is_empty())?;
    if status == "Stopped" {
        return None;
    }
    let artist = meta.get("xesam:artist").and_then(|v| match &**v {
        Value::Array(a) => a.iter().find_map(|s| s.downcast_ref::<&str>().ok().map(str::to_owned)),
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    });
    Some(Track {
        title,
        artist: artist.unwrap_or_default(),
        playing: status == "Playing",
        length: int(meta.get("mpris:length").map(|v| &**v)) / 1e6,
        position: int(p.get("Position").map(|v| &**v)) / 1e6,
        read_at: Instant::now(),
        art: string(&meta, "mpris:artUrl").and_then(|u| art_path(&u)),
    })
}

/// file:// paths decode in place; remote art is fetched once into the cache.
fn art_path(url: &str) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        let mut bytes = Vec::with_capacity(path.len());
        let mut raw = path.bytes();
        while let Some(b) = raw.next() {
            let hex = |b: Option<u8>| (b? as char).to_digit(16);
            bytes.push(if b == b'%' {
                let mut peek = raw.clone();
                match (hex(peek.next()), hex(peek.next())) {
                    (Some(h), Some(l)) => {
                        raw = peek;
                        (h * 16 + l) as u8
                    }
                    _ => b,
                }
            } else {
                b
            });
        }
        return Some(String::from_utf8_lossy(&bytes).into_owned().into());
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    // ponytail: unbounded cache of small thumbnails; a tmp cleaner owns it.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    let dir = crate::settings::home().join(".cache/island/art");
    let file = dir.join(format!("{:016x}", h.finish()));
    if !file.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        let tmp = file.with_extension("part");
        let ok = std::process::Command::new("curl")
            .args(["-sfL", "--max-time", "6", "--max-filesize", "4000000", "-o"]).arg(&tmp).arg(url)
            .status().is_ok_and(|s| s.success());
        if !ok {
            return None;
        }
        std::fs::rename(tmp, &file).ok()?;
    }
    Some(file)
}

fn read_media(c: &Connection) -> Option<Track> {
    track(&get_all(c, PLAYER, PLAYER_PATH, PLAYER_IFACE)?)
}

// ---- Tray ----

#[derive(Clone, Debug, PartialEq)]
pub enum TrayIcon {
    Named { name: String, theme_path: Option<String> },
    /// Premultiplied RGBA.
    Pixels { w: i32, h: i32, rgba: Vec<u8> },
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrayItem {
    /// As registered: bus name + object path.
    pub dest: String,
    pub path: String,
    /// Unique bus name, which is what signals arrive from.
    pub owner: String,
    pub title: String,
    pub icon: TrayIcon,
    pub menu: Option<String>,
    pub is_menu: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub checked: Option<bool>,
    pub children: Vec<MenuEntry>,
}

struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = hdr.sender().map(ToString::to_string).unwrap_or_default();
        let full = canonical(service, &sender);
        let added = {
            let mut items = self.items.lock().unwrap();
            let new = !items.contains(&full);
            if new {
                items.push(full.clone());
            }
            new
        };
        if added {
            let _ = Self::status_notifier_item_registered(&emitter, &full).await;
        }
    }

    async fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(e: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(e: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
}

/// Apps register a bus name, an object path (ayatana) or both; normalise to
/// "name/path" using the caller for whatever they left out.
fn canonical(service: &str, sender: &str) -> String {
    match service.split_once('/') {
        Some(("", path)) => format!("{sender}/{path}"),
        Some(_) => service.to_owned(),
        None => format!("{service}/StatusNotifierItem"),
    }
}

fn split(service: &str) -> Option<(String, String)> {
    let (dest, path) = service.split_once('/')?;
    Some((dest.to_owned(), format!("/{path}")))
}

fn pixmap(v: &Value<'_>) -> TrayIcon {
    // a(iiay): take the largest. Pixels are big-endian ARGB, not premultiplied.
    let Value::Array(sizes) = v else { return TrayIcon::None };
    let mut best: Option<(i32, i32, Vec<u8>)> = None;
    for s in sizes.iter() {
        let Value::Structure(s) = s else { continue };
        let [Value::I32(w), Value::I32(h), Value::Array(px)] = s.fields() else { continue };
        if best.as_ref().is_none_or(|b| b.0 * b.1 < w * h) {
            best = Some((*w, *h, px.iter().filter_map(|b| b.downcast_ref::<u8>().ok()).collect()));
        }
    }
    let Some((w, h, argb)) = best else { return TrayIcon::None };
    if w <= 0 || h <= 0 || argb.len() != (w * h * 4) as usize {
        return TrayIcon::None;
    }
    let mut rgba = Vec::with_capacity(argb.len());
    for p in argb.chunks_exact(4) {
        let a = p[0] as u32;
        let pm = |c: u8| (c as u32 * a / 255) as u8;
        rgba.extend_from_slice(&[pm(p[1]), pm(p[2]), pm(p[3]), p[0]]);
    }
    TrayIcon::Pixels { w, h, rgba }
}

fn read_item(c: &Connection, service: &str) -> Option<TrayItem> {
    let (dest, path) = split(service)?;
    let p = get_all(c, &dest, &path, ITEM)?;
    if string(&p, "Status").as_deref() == Some("Passive") {
        return None;
    }
    let owner: String = c.call_method(Some(DBUS), DBUS_PATH, Some(DBUS), "GetNameOwner", &dest).ok()?
        .body().deserialize().ok()?;
    let needs_attention = string(&p, "Status").as_deref() == Some("NeedsAttention");
    let names = if needs_attention { ["AttentionIconName", "IconName"] } else { ["IconName", "IconName"] };
    let name = names.iter().find_map(|k| string(&p, k)).filter(|n| !n.is_empty());
    let icon = match name {
        Some(name) => TrayIcon::Named { name, theme_path: string(&p, "IconThemePath").filter(|t| !t.is_empty()) },
        None => p.get("IconPixmap").map_or(TrayIcon::None, |v| pixmap(v)),
    };
    let menu = p.get("Menu").and_then(|v| v.try_clone().ok())
        .and_then(|v| zbus::zvariant::OwnedObjectPath::try_from(v).ok())
        .map(|o| o.to_string()).filter(|m| m != "/" && !m.starts_with("/NO_DBUSMENU"));
    let is_menu = p.get("ItemIsMenu").and_then(|v| v.try_clone().ok())
        .and_then(|v| bool::try_from(v).ok()).unwrap_or(false);
    let title = string(&p, "Title").or_else(|| string(&p, "Id")).unwrap_or_default();
    Some(TrayItem { dest, path, owner, title, icon, menu, is_menu })
}

fn read_items(c: &Connection) -> Vec<String> {
    let reply = c.call_method(Some(WATCHER), WATCHER_PATH, Some(PROPS), "Get", &(WATCHER, "RegisteredStatusNotifierItems"));
    reply.ok().and_then(|r| r.body().deserialize::<OwnedValue>().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok()).unwrap_or_default()
}

fn claim_watcher(c: &Connection) {
    let flags = RequestNameFlags::DoNotQueue.into();
    let _ = c.request_name_with_flags(WATCHER, flags);
    let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    let _ = c.request_name_with_flags(host.as_str(), flags);
    let _ = c.call_method(Some(WATCHER), WATCHER_PATH, Some(WATCHER), "RegisterStatusNotifierHost", &host);
}

struct Tray {
    conn: Connection,
    tx: crate::Tx,
    items: HashMap<String, TrayItem>,
    order: Vec<String>,
    owned: Arc<Mutex<Vec<String>>>,
}

impl Tray {
    fn publish(&self) {
        let list = self.order.iter().filter_map(|s| self.items.get(s).cloned()).collect();
        let _ = self.tx.send(Msg::Tray(list));
    }

    fn resync(&mut self) {
        self.order = read_items(&self.conn);
        self.items = self.order.iter()
            .filter_map(|s| read_item(&self.conn, s).map(|i| (s.clone(), i))).collect();
        self.publish();
    }

    fn refresh(&mut self, service: &str) {
        match read_item(&self.conn, service) {
            Some(item) => {
                if !self.order.iter().any(|s| s == service) {
                    self.order.push(service.to_owned());
                }
                self.items.insert(service.to_owned(), item);
            }
            None => {
                self.items.remove(service);
            }
        }
        self.publish();
    }

    fn name_changed(&mut self, name: &str, new_owner: &str) {
        if name == WATCHER && new_owner.is_empty() {
            claim_watcher(&self.conn);
        }
        if name == WATCHER && !new_owner.is_empty() {
            return self.resync();
        }
        if !new_owner.is_empty() {
            return;
        }
        // An app quit: drop whatever it registered, under either of its names.
        let gone: Vec<String> = self.order.iter()
            .filter(|s| s.split('/').next() == Some(name)
                        || self.items.get(*s).is_some_and(|i| i.owner == name)).cloned().collect();
        if gone.is_empty() {
            return;
        }
        self.owned.lock().unwrap().retain(|s| !gone.contains(s));
        self.order.retain(|s| !gone.contains(s));
        self.items.retain(|s, _| !gone.contains(s));
        self.publish();
        let conn = self.conn.clone();
        thread::spawn(move || {
            let Ok(iface) = conn.object_server().interface::<_, Watcher>(WATCHER_PATH) else { return };
            for s in &gone {
                let _ = zbus::block_on(Watcher::status_notifier_item_unregistered(iface.signal_emitter(), s));
            }
        });
    }

    fn signal(&mut self, msg: &Message) {
        let hdr = msg.header();
        let (Some(iface), Some(member)) = (hdr.interface(), hdr.member()) else { return };
        match (iface.as_str(), member.as_str()) {
            (DBUS, "NameOwnerChanged") => {
                if let Ok((name, _, new)) = msg.body().deserialize::<(String, String, String)>() {
                    self.name_changed(&name, &new);
                }
            }
            (WATCHER, "StatusNotifierItemRegistered") => {
                if let Ok(s) = msg.body().deserialize::<String>() {
                    self.refresh(&s);
                }
            }
            (WATCHER, "StatusNotifierItemUnregistered") => {
                if let Ok(s) = msg.body().deserialize::<String>() {
                    self.order.retain(|x| x != &s);
                    self.items.remove(&s);
                    self.publish();
                }
            }
            (ITEM, _) => {
                let sender = hdr.sender().map(ToString::to_string).unwrap_or_default();
                let hit = self.items.iter().find(|(_, i)| i.owner == sender).map(|(s, _)| s.clone());
                // Hidden (Passive) items have no entry; they come back through a full resync.
                match hit {
                    Some(s) => self.refresh(&s),
                    None => self.resync(),
                }
            }
            _ => {}
        }
    }
}

/// dbusmenu layout: id, properties, then children boxed in variants.
type MenuNode = (i32, Props, Vec<OwnedValue>);

fn menu_entry(v: &Value<'_>) -> Option<MenuEntry> {
    let inner: &Structure = match v {
        Value::Structure(s) => s,
        Value::Value(b) => match &**b {
            Value::Structure(s) => s,
            _ => return None,
        },
        _ => return None,
    };
    let [Value::I32(id), Value::Dict(props), Value::Array(children)] = inner.fields() else { return None };
    let props: HashMap<String, OwnedValue> = props.try_clone().ok()?.try_into().ok()?;
    let flag = |k: &str, default: bool| {
        props.get(k).and_then(|v| v.try_clone().ok()).and_then(|v| bool::try_from(v).ok()).unwrap_or(default)
    };
    if !flag("visible", true) {
        return None;
    }
    let kind = string(&props, "type").unwrap_or_default();
    let toggle = string(&props, "toggle-type").filter(|t| !t.is_empty());
    let state = props.get("toggle-state").and_then(|v| v.try_clone().ok()).and_then(|v| i32::try_from(v).ok());
    Some(MenuEntry {
        id: *id,
        label: string(&props, "label").unwrap_or_default().replace('_', ""),
        enabled: flag("enabled", true),
        separator: kind == "separator",
        checked: toggle.map(|_| state == Some(1)),
        children: children.iter().filter_map(menu_entry).collect(),
    })
}

/// Clones of the connection are cheap and thread-safe, and every call here runs
/// on its own short-lived thread so a hung app can't stall the UI.
#[derive(Clone)]
pub struct Bus {
    conn: Connection,
}

impl Bus {
    pub fn activate(&self, item: &TrayItem, method: &'static str) {
        let (c, dest, path) = (self.conn.clone(), item.dest.clone(), item.path.clone());
        thread::spawn(move || {
            let _ = c.call_method(Some(dest.as_str()), path.as_str(), Some(ITEM), method, &(0i32, 0i32));
        });
    }

    pub fn open_menu(&self, item: &TrayItem, index: usize, tx: crate::Tx) {
        let (c, dest, Some(path)) = (self.conn.clone(), item.dest.clone(), item.menu.clone()) else { return };
        thread::spawn(move || {
            let (d, p) = (dest.as_str(), path.as_str());
            let _ = c.call_method(Some(d), p, Some(MENU), "AboutToShow", &0i32);
            let reply = c.call_method(Some(d), p, Some(MENU), "GetLayout", &(0i32, -1i32, Vec::<&str>::new()));
            let Ok(reply) = reply else { return };
            let body = reply.body();
            let Ok((_, (_, _, children))) = body.deserialize::<(u32, MenuNode)>() else { return };
            let _ = tx.send(Msg::Menu(index, children.iter().filter_map(|c| menu_entry(c)).collect()));
        });
    }

    pub fn menu_click(&self, item: &TrayItem, id: i32) {
        let (c, dest, Some(path)) = (self.conn.clone(), item.dest.clone(), item.menu.clone()) else { return };
        thread::spawn(move || {
            let time = 0u32;
            let args = (id, "clicked", Value::from(0i32), time);
            let _ = c.call_method(Some(dest.as_str()), path.as_str(), Some(MENU), "Event", &args);
        });
    }

    pub fn refresh_media(&self, tx: crate::Tx) {
        let c = self.conn.clone();
        thread::spawn(move || {
            let _ = tx.send(Msg::Media(read_media(&c)));
        });
    }
}

fn add_match(c: &Connection, rule: &str) {
    if let Err(e) = c.call_method(Some(DBUS), DBUS_PATH, Some(DBUS), "AddMatch", &rule) {
        eprintln!("island: AddMatch {rule}: {e}");
    }
}

pub fn spawn(tx: crate::Tx) -> Option<Bus> {
    let conn = Builder::session().ok()?.method_timeout(Duration::from_secs(2)).build().ok()?;
    let owned = Arc::new(Mutex::new(Vec::new()));
    let at = conn.object_server().at(WATCHER_PATH, Watcher { items: owned.clone() });
    if let Err(e) = at {
        eprintln!("island: can't serve the tray watcher: {e}");
    }
    for rule in [
        format!("type='signal',sender='{PLAYER}',path='{PLAYER_PATH}'"),
        format!("type='signal',interface='{ITEM}'"),
        format!("type='signal',interface='{WATCHER}'"),
        format!("type='signal',sender='{DBUS}',member='NameOwnerChanged'"),
    ] {
        add_match(&conn, &rule);
    }
    let bus = Bus { conn: conn.clone() };
    thread::spawn(move || {
        // Subscribe before claiming so nothing registered in between is lost.
        let signals = MessageIterator::from(&conn);
        claim_watcher(&conn);
        let mut tray = Tray { conn: conn.clone(), tx: tx.clone(), items: HashMap::new(), order: Vec::new(), owned };
        tray.resync();
        let _ = tx.send(Msg::Media(read_media(&conn)));
        for msg in signals.flatten() {
            if msg.message_type() != Type::Signal {
                continue;
            }
            let from_player = msg.header().path().is_some_and(|p| p.as_str() == PLAYER_PATH);
            if from_player {
                let _ = tx.send(Msg::Media(read_media(&conn)));
            } else {
                tray.signal(&msg);
            }
        }
    });
    Some(bus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_every_registration_shape() {
        assert_eq!(canonical("/org/ayatana/x", ":1.5"), ":1.5/org/ayatana/x");
        assert_eq!(canonical("org.foo.Item", ":1.5"), "org.foo.Item/StatusNotifierItem");
        assert_eq!(canonical(":1.5/StatusNotifierItem", ":1.9"), ":1.5/StatusNotifierItem");
        assert_eq!(split(":1.5/a/b"), Some((":1.5".into(), "/a/b".into())));
    }

    #[test]
    fn local_art_urls_decode_percent_escapes() {
        assert_eq!(art_path("file:///tmp/a%20b.png"), Some("/tmp/a b.png".into()));
        assert_eq!(art_path("file:///tmp/100%"), Some("/tmp/100%".into()));
        assert_eq!(art_path("data:image/png;base64,xx"), None);
    }

    #[test]
    fn track_position_projects_forward_only_while_playing() {
        let mut t = Track { title: "t".into(), artist: String::new(), playing: false, length: 10.0, position: 4.0,
                            read_at: Instant::now() - Duration::from_secs(100), art: None };
        assert_eq!(t.now(), 4.0);
        t.playing = true;
        assert_eq!(t.now(), 10.0);
    }
}
