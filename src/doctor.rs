//! `island doctor`: what works on this machine, what doesn't, and how to fix it.
//! Every line is a check you could run by hand; nothing here changes anything.
use crate::{settings::Config, tools::on_path};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use std::process::Command;

/// Color only on a terminal, so piping the report into an issue stays readable.
fn paint(code: &str, text: &str) -> String {
    if unsafe { libc::isatty(1) } == 1 { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_owned() }
}

fn section(title: &str) {
    println!("{}", paint("1", title));
}

#[derive(Default)]
struct Report {
    problems: u32,
    warnings: u32,
}

impl Report {
    fn ok(&self, what: &str) {
        println!("  {} {what}", paint("32", "✓"));
    }
    fn warn(&mut self, what: &str, fix: &str) {
        self.warnings += 1;
        println!("  {} {what}\n      {fix}", paint("33", "!"));
    }
    fn bad(&mut self, what: &str, fix: &str) {
        self.problems += 1;
        println!("  {} {what}\n      {fix}", paint("31", "✗"));
    }
    fn check(&mut self, good: bool, what: &str, fix: &str) {
        if good { self.ok(what) } else { self.warn(what, fix) }
    }
}

struct Globals;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Globals {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

/// The compositor's protocol names, or None off Wayland.
fn globals() -> Option<Vec<String>> {
    let conn = Connection::connect_to_env().ok()?;
    let (list, _) = registry_queue_init::<Globals>(&conn).ok()?;
    Some(list.contents().with_list(|l| l.iter().map(|g| g.interface.clone()).collect()))
}

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Who owns a bus name, as a process name.
fn owner(name: &str) -> Option<String> {
    let out = output("busctl", &["--user", "status", name])?;
    out.lines().find_map(|l| l.strip_prefix("Comm=")).map(str::to_owned)
}

pub fn run() -> ! {
    // Piped into head or grep: stop quietly when the reader goes away.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let mut r = Report::default();
    let cfg_path = Config::path();
    section("Session");
    match globals() {
        None => r.bad("Not on a Wayland session", "Run this from a terminal inside your desktop session."),
        Some(g) => {
            let has = |p: &str| g.iter().any(|i| i == p);
            if has("zwlr_layer_shell_v1") {
                r.ok("Layer shell: the island can draw over your desktop");
            } else {
                r.bad("No layer shell", "This compositor can't host bars or overlays; the island needs wlr-layer-shell.");
            }
            r.check(has("wp_cursor_shape_manager_v1"), "Cursor shapes", "The pointer keeps its default look over the island.");
            r.check(has("zwlr_gamma_control_manager_v1"), "Gamma control: night light works", "This compositor can't tint the screen, so night light stays hidden.");
            r.check(has("zwlr_screencopy_manager_v1") || has("ext_image_copy_capture_manager_v1"), "Screen capture: screenshots work",
                    "grim can't capture on this compositor, so the screenshot tool can't either.");
        }
    }
    let wm = if std::env::var_os("MANGO_INSTANCE_SIGNATURE").is_some() { Some("Mango") }
             else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() { Some("Hyprland") } else { None };
    match wm {
        Some(w) => r.ok(&format!("{w}: workspaces and windows show in the pill")),
        None => r.warn("No supported window manager", "Workspaces stay empty. Mango and Hyprland are supported; MODDING.md says how to add one."),
    }
    let running = std::os::unix::net::UnixStream::connect(crate::socket_path()).is_ok();
    r.check(running, "The island is running", "Start it with `island`, or log in again if the installer set up autostart.");

    println!();
    section("Config");
    match std::fs::read_to_string(&cfg_path) {
        Err(_) => r.ok(&format!("No {}: defaults are in use", cfg_path.display())),
        Ok(src) => match Config::parse(&src) {
            Err(e) => r.bad(&format!("{} doesn't parse", cfg_path.display()), &format!("The island ignores it and uses defaults: {e}")),
            Ok(cfg) => {
                r.ok(&format!("{} parses", cfg_path.display()));
                let (pill, panel) = crate::modules::names();
                let unknown = |list: &[String], known: &[&str]| -> Vec<String> {
                    list.iter().filter(|n| !n.starts_with("custom:") && !known.contains(&n.as_str())).cloned().collect()
                };
                for (what, bad) in [("pill", unknown(&cfg.pill, &pill)), ("panel", unknown(&cfg.panel, &panel))] {
                    if !bad.is_empty() {
                        r.warn(&format!("Unknown {what} modules: {}", bad.join(", ")), &format!("Known ones: {}", if what == "pill" { pill.join(", ") } else { panel.join(", ") }));
                    }
                }
            }
        },
    }
    let cfg = Config::load();
    for (font, what) in [(&cfg.font, "Text font"), (&cfg.icon_font, "Icon font")] {
        let found = output("fc-list", &[font.as_str(), "family"]).is_some_and(|o| !o.trim().is_empty());
        r.check(found, &format!("{what}: {font}"), "Not installed, so another font stands in. Any Nerd Font works for icons (set icon_font).");
    }
    r.check(crate::icons::adwaita_symbolic().is_some(), "Adwaita symbolic icons", "Install adwaita-icon-theme, or the panel's buttons draw blank.");

    println!();
    section("Features (each one only needs its own programs)");
    let features: &[(&str, &[&str], &str)] = &[
        ("Volume", &["pactl", "wpctl"], "PipeWire's pipewire-pulse and wireplumber"),
        ("Media", &["playerctl", "playerctld"], "playerctl"),
        ("Wi-Fi", &["nmcli"], "networkmanager"),
        ("Clipboard history", &["cliphist", "wl-copy"], "cliphist and wl-clipboard, with `wl-paste --watch cliphist store` running"),
        ("Screenshots", &["grim", "slurp"], "grim and slurp"),
        ("File search", &["plocate"], "plocate (and `sudo updatedb`), or fd"),
        ("Opening files and links", &["xdg-open"], "xdg-utils"),
        ("Monitor brightness", &["ddcutil"], "ddcutil, plus the i2c-dev module loaded"),
        ("Claude Code status", &["claude"], "Claude Code, then ./install.sh --manual to add its hooks"),
    ];
    for (name, bins, pkg) in features {
        let missing: Vec<&str> = bins.iter().copied().filter(|b| !on_path(b)).collect();
        let alt = *name == "File search" && on_path("fd");
        if missing.is_empty() || alt {
            r.ok(name);
        } else {
            r.warn(&format!("{name}: missing {}", missing.join(", ")), &format!("Install {pkg}. Until then it stays hidden."));
        }
    }
    if output("busctl", &["--system", "status", "org.bluez"]).is_some() {
        r.ok("Bluetooth (BlueZ)");
    } else {
        r.warn("Bluetooth: BlueZ isn't running", "Install bluez and `systemctl enable --now bluetooth`, if this machine has Bluetooth.");
    }

    println!();
    section("Desktop services");
    match (cfg.notifications, owner("org.freedesktop.Notifications")) {
        (true, Some(o)) if o == "island" => r.ok("The island shows notifications"),
        (true, Some(o)) => r.warn(&format!("{o} is the notification daemon"), "The island takes over when it stops. Remove it from autostart (the installer does this)."),
        (_, None) => r.warn("No notification daemon", "Start the island, or another daemon, so apps can notify."),
        (false, Some(o)) => r.ok(&format!("{o} shows notifications (notifications = false)")),
    }
    match owner("org.kde.StatusNotifierWatcher") {
        Some(o) if o == "island" => r.ok("The island hosts the tray"),
        Some(o) => r.warn(&format!("{o} hosts the tray"), "Tray icons still show; the island takes over the watcher when it's released."),
        None => r.warn("No tray watcher", "Apps with tray icons won't show until the island runs."),
    }
    if wm == Some("Mango") && on_path("mango") {
        let conf = crate::settings::home().join(".config/mango");
        let sourced = ["config.conf", "config.toml"].iter()
            .any(|f| std::fs::read_to_string(conf.join(f)).is_ok_and(|s| s.contains("island.conf")));
        r.check(sourced, "Mango config sources island.conf", "Run ./install.sh so the island's keys and window rule load.");
        let check = Command::new("mango").arg("-p").env_remove("DBUS_SESSION_BUS_ADDRESS").output();
        let clean = check.as_ref().is_ok_and(|o| o.status.success());
        r.check(clean, "mango -p: no config warnings", "Run `mango -p` to see them; a key bound twice is the usual one.");
    }

    println!();
    let code = match (r.problems, r.warnings) {
        (0, 0) => { println!("All good."); 0 }
        (0, w) => { println!("{w} thing(s) to look at; nothing broken."); 0 }
        (p, _) => { println!("{p} problem(s) need fixing."); 1 }
    };
    std::process::exit(code)
}
