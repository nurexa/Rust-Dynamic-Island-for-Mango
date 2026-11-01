//! Test fixture: an MPRIS player and a tray item with a dbusmenu, for driving
//! the island on a private bus without needing real apps.
//!   cargo run --example mock -- /path/to/art.png
use std::{collections::HashMap, time::Duration};
use zbus::{blocking::connection::Builder, zvariant::{OwnedValue, Structure, Value}};

struct Player {
    playing: bool,
    art: String,
}

fn log(what: &str) {
    println!("{what}");
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&mut self) {
        self.playing = !self.playing;
        log("media: play-pause");
    }
    fn next(&self) {
        log("media: next");
    }
    fn previous(&self) {
        log("media: previous");
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.playing { "Playing".into() } else { "Paused".into() }
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        61_000_000
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let s = |v: &str| Value::from(v).try_to_owned().unwrap();
        HashMap::from([
            ("xesam:title".to_owned(), s("Teardrop — a very long track title for ellipsis")),
            ("xesam:artist".to_owned(), Value::from(vec!["Massive Attack"]).try_to_owned().unwrap()),
            ("mpris:length".to_owned(), Value::from(240_000_000i64).try_to_owned().unwrap()),
            ("mpris:artUrl".to_owned(), s(&format!("file://{}", self.art))),
        ])
    }
}

struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Mock".into()
    }
}

struct Item;

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, _x: i32, _y: i32) {
        log("tray: activate");
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        log("tray: secondary");
    }
    #[zbus(property)]
    fn id(&self) -> String {
        "mock".into()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        "Mock Tray".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        "audio-volume-high-symbolic".into()
    }
    #[zbus(property)]
    fn menu(&self) -> zbus::zvariant::OwnedObjectPath {
        "/Menu".try_into().unwrap()
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
}

struct Menu;

fn node(id: i32, label: &str, kind: Option<&str>, children: Vec<OwnedValue>) -> OwnedValue {
    let mut props: HashMap<&str, Value> = HashMap::from([("label", Value::from(label.to_owned()))]);
    if let Some(k) = kind {
        props.insert("type", Value::from(k.to_owned()));
    }
    let kids: Vec<Value> = children.iter().map(|c| Value::Value(Box::new(c.try_clone().unwrap().into()))).collect();
    let array = zbus::zvariant::Array::from(kids);
    let s = Structure::from((id, props, array));
    Value::Structure(s).try_to_owned().unwrap()
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }
    fn get_layout(&self, _parent: i32, _depth: i32, _props: Vec<String>) -> (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)) {
        let sub = node(3, "More", None, vec![node(31, "Nested action", None, vec![])]);
        let kids = vec![node(1, "_Open Mock", None, vec![]), node(2, "", Some("separator"), vec![]), sub,
                        node(4, "Quit", None, vec![])];
        (1, (0, HashMap::new(), kids))
    }
    fn event(&self, id: i32, event: String, _data: Value<'_>, _time: u32) {
        log(&format!("menu: {event} {id}"));
    }
}

fn main() -> zbus::Result<()> {
    let art = std::env::args().nth(1).unwrap_or_default();
    let conn = Builder::session()?
        .name("org.mpris.MediaPlayer2.mock")?
        .name(format!("org.freedesktop.StatusNotifierItem-{}-1", std::process::id()))?
        .serve_at("/org/mpris/MediaPlayer2", Player { playing: true, art })?
        .serve_at("/org/mpris/MediaPlayer2", Root)?
        .serve_at("/StatusNotifierItem", Item)?
        .serve_at("/Menu", Menu)?
        .build()?;
    let name = conn.unique_name().unwrap().to_string();
    for _ in 0..10 {
        let r = conn.call_method(Some("org.kde.StatusNotifierWatcher"), "/StatusNotifierWatcher",
                                 Some("org.kde.StatusNotifierWatcher"), "RegisterStatusNotifierItem", &format!("{name}/StatusNotifierItem"));
        if r.is_ok() {
            log("tray: registered");
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
