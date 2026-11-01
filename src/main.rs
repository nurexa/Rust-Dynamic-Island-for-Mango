//! A Dynamic Island for MangoWC. See README.md.
mod audio;
mod bus;
mod draw;
mod icons;
mod island;
mod mango;
mod net;
mod settings;
mod sys;
mod theme;

use cairo::ImageSurface;
use draw::{Action, Scroll, View};
use island::{Island, Outcome};
use settings::{Config, State};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, channel},
        calloop_wayland_source::WaylandSource,
        client::{Connection, QueueHandle, globals::registry_queue_init,
                 protocol::{wl_output, wl_pointer, wl_seat, wl_surface}},
        protocols::wp::{
            cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
            relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager},
        relative_pointer::{RelativeMotionEvent, RelativePointerHandler, RelativePointerState},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler},
};
use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub type Tx = channel::Sender<Msg>;

const STATS_EVERY: Duration = Duration::from_millis(1500);
const NAMESPACE: &str = "island";

pub enum Msg {
    Monitors(Vec<mango::Monitor>),
    Clients(Vec<mango::Client>),
    Audio(audio::State),
    Media(Option<bus::Track>),
    Net(net::State),
    Tray(Vec<bus::TrayItem>),
    Menu(usize, Vec<bus::MenuEntry>),
    Theme,
    Command(String),
    Quit,
}

/// Everything on screen comes from here; workers only ever feed it messages.
pub struct Model {
    pub monitors: Vec<mango::Monitor>,
    pub clients: Vec<mango::Client>,
    pub sys: sys::Snapshot,
    pub audio: Option<audio::State>,
    pub media: Option<bus::Track>,
    pub art_small: Option<ImageSurface>,
    pub art_large: Option<ImageSurface>,
    pub net: net::State,
    pub tray: Vec<(bus::TrayItem, Option<(ImageSurface, bool)>)>,
    pub pal: theme::Palette,
}

pub struct App {
    registry: RegistryState,
    seat: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    relative: RelativePointerState,
    cursor_shapes: Option<CursorShapeManager>,
    pointer: Option<wl_pointer::WlPointer>,
    relative_pointer: Option<ZwpRelativePointerV1>,
    shape_device: Option<WpCursorShapeDeviceV1>,
    enter_serial: u32,
    cursor: Option<Shape>,
    qh: QueueHandle<App>,

    islands: Vec<Island>,
    under_pointer: Option<wl_surface::WlSurface>,
    model: Model,
    icons: Icons,
    cfg: Config,
    state: State,
    sampler: sys::Sampler,
    bus: Option<bus::Bus>,
    net_poke: mpsc::Sender<()>,
    tx: Tx,
    next_stats: Option<Instant>,
    next_clock: Instant,
    scroll: f64,
    quit: bool,
}

use icons::Icons;

fn spawn_sh(cmd: &str) {
    let child = Command::new("sh").arg("-c").arg(cmd).stdin(Stdio::null())
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if let Ok(mut child) = child {
        thread::spawn(move || child.wait());
    }
}

fn next_minute() -> Instant {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    Instant::now() + Duration::from_secs_f64(60.0 - secs % 60.0 + 0.05)
}

impl App {
    fn sync_outputs(&mut self) {
        let qh = self.qh.clone();
        let monitors = self.model.monitors.len();
        for output in self.outputs.outputs().collect::<Vec<_>>() {
            let Some(info) = self.outputs.info(&output) else { continue };
            let Some(name) = info.name.clone() else { continue };
            let size = info.logical_size.or_else(|| info.modes.iter().find(|m| m.current).map(|m| m.dimensions));
            let Some((w, h)) = size else { continue };
            let out = (w as f64, h as f64);
            let scale = info.scale_factor.max(1);
            if let Some(i) = self.islands.iter_mut().find(|i| i.output == output) {
                if i.out != out || i.scale != scale {
                    i.set_output(out, scale, monitors);
                }
                continue;
            }
            let surface = self.compositor.create_surface(&qh);
            let layer = self.layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some(NAMESPACE), Some(&output));
            layer.set_anchor(Anchor::TOP);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            let island = Island::new(output.clone(), name.clone(), out, scale, layer, &self.shm,
                                     self.state.pos.get(&name).copied(), self.state.pinned, self.state.tucked, monitors);
            if let Some(island) = island {
                let size = island.surface_size();
                island.layer.set_size(size.0 as u32, size.1 as u32);
                island.layer.commit();
                self.islands.push(island);
            }
        }
    }

    fn all(&mut self, compact: bool, panel: bool) {
        for i in &mut self.islands {
            i.invalidate(compact, panel);
        }
    }

    fn save(&mut self) {
        self.state.pos = self.islands.iter().filter_map(|i| Some((i.name.clone(), i.pos?))).collect();
        self.state.save();
    }

    fn set_art(&mut self) {
        let path = self.model.media.as_ref().and_then(|t| t.art.clone());
        let scale = self.islands.first().map_or(1, |i| i.scale);
        let load = |p: &PathBuf, px: i32| icons::load_art(p, px * scale);
        self.model.art_small = path.as_ref().and_then(|p| load(p, 20)).map(|s| rounded(s, 5.0 * scale as f64));
        self.model.art_large = path.as_ref().and_then(|p| load(p, 58));
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Monitors(m) => {
                if m != self.model.monitors {
                    let count = m.len() != self.model.monitors.len();
                    self.model.monitors = m;
                    if count {
                        self.sync_outputs();
                        for i in &mut self.islands {
                            let (out, scale) = (i.out, i.scale);
                            i.set_output(out, scale, self.model.monitors.len());
                        }
                    }
                    self.all(true, true);
                }
            }
            Msg::Clients(c) => {
                if c != self.model.clients {
                    self.model.clients = c;
                    self.all(true, true);
                }
            }
            Msg::Audio(a) => {
                let changed = self.model.audio.as_ref()
                    .is_some_and(|o| o.volume != a.volume || o.muted != a.muted);
                self.model.audio = Some(a);
                if changed {
                    for i in &mut self.islands {
                        i.flash();
                    }
                }
                self.all(false, true);
            }
            Msg::Media(t) => {
                let same_art = self.model.media.as_ref().map(|t| &t.art) == t.as_ref().map(|t| &t.art);
                let visible = |t: &Option<bus::Track>| t.as_ref().map(|t| (t.playing, t.art.clone()));
                let compact = visible(&self.model.media) != visible(&t);
                self.model.media = t;
                if !same_art {
                    self.set_art();
                }
                self.all(compact, true);
            }
            Msg::Net(n) => {
                let compact = (n == net::State::Offline) != (self.model.net == net::State::Offline);
                self.model.net = n;
                self.all(compact, true);
            }
            Msg::Tray(items) => {
                let scale = self.islands.first().map_or(1, |i| i.scale);
                let icons = &mut self.icons;
                self.model.tray = items.into_iter().map(|item| {
                    let img = icons.tray(&item.icon, 18 * scale);
                    (item, img)
                }).collect();
                self.all(false, true);
            }
            Msg::Menu(index, entries) => {
                let title = self.model.tray.get(index).map_or(String::new(), |(i, _)| i.title.clone());
                for i in self.islands.iter_mut().filter(|i| i.wants_panel()) {
                    i.view = View::Menu { item: index, title: title.clone(), entries: entries.clone(), root: entries.clone() };
                    i.invalidate(false, true);
                }
            }
            Msg::Theme => {
                self.model.pal = theme::load();
                self.all(true, true);
            }
            Msg::Command(c) => self.command(&c),
            Msg::Quit => self.quit = true,
        }
    }

    fn command(&mut self, c: &str) {
        match c {
            "pin" | "toggle" => self.act(Action::Pin, 0.0, None),
            "tuck" => self.act(Action::Tuck, 0.0, None),
            "reset" => self.act(Action::Reset, 0.0, None),
            "quit" => self.quit = true,
            _ => eprintln!("island: unknown command {c}"),
        }
    }

    fn act(&mut self, action: Action, frac: f64, from: Option<usize>) {
        let tray = |m: &Model, i: usize| m.tray.get(i).map(|(item, _)| item.clone());
        match action {
            Action::None => {}
            Action::View { tag, mon } => mango::dispatch(&format!("viewcrossmon,{tag},{mon}")),
            Action::MoveTo(tag) => mango::dispatch(&format!("tagsilent,{tag}")),
            Action::Focus(id) => mango::dispatch(&format!("focusid client,{id}")),
            Action::Sh(cmd) => spawn_sh(cmd),
            Action::Mute => spawn_sh("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"),
            Action::Volume => {
                let pct = (frac * 100.0).round() as u32;
                spawn_sh(&format!("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ {pct}%"));
            }
            Action::Pin => {
                self.state.pinned = !self.state.pinned;
                let p = self.state.pinned;
                self.islands.iter_mut().for_each(|i| i.set_pinned(p));
                self.save();
            }
            Action::Tuck => {
                self.state.tucked = !self.state.tucked;
                let t = self.state.tucked;
                self.islands.iter_mut().for_each(|i| i.set_tucked(t));
                self.save();
            }
            Action::Reset => {
                self.state.tucked = false;
                for i in &mut self.islands {
                    i.set_tucked(false);
                    i.reset();
                }
                self.save();
            }
            Action::TrayClick(i) => {
                let Some(item) = tray(&self.model, i) else { return };
                if item.is_menu && item.menu.is_some() {
                    self.act(Action::TrayRight(i), 0.0, from);
                } else if let Some(b) = &self.bus {
                    b.activate(&item, "Activate");
                }
            }
            Action::TrayMiddle(i) => {
                if let (Some(item), Some(b)) = (tray(&self.model, i), &self.bus) {
                    b.activate(&item, "SecondaryActivate");
                }
            }
            Action::TrayRight(i) => {
                let (Some(item), Some(b)) = (tray(&self.model, i), &self.bus) else { return };
                if item.menu.is_some() {
                    b.open_menu(&item, i, self.tx.clone());
                } else {
                    b.activate(&item, "ContextMenu");
                }
            }
            Action::MenuClick(id) => {
                if let Some(f) = from {
                    if let View::Menu { item, .. } = self.islands[f].view {
                        if let (Some(entry), Some(b)) = (tray(&self.model, item), &self.bus) {
                            b.menu_click(&entry, id);
                        }
                    }
                    self.islands[f].view = View::Main;
                    self.islands[f].invalidate(false, true);
                }
            }
            Action::MenuSub(id) => {
                if let Some(f) = from {
                    if let View::Menu { entries, .. } = &mut self.islands[f].view {
                        if let Some(e) = entries.iter().find(|e| e.id == id) {
                            *entries = e.children.clone();
                        }
                    }
                    self.islands[f].invalidate(false, true);
                }
            }
            Action::MenuBack => {
                if let Some(f) = from {
                    let isl = &mut self.islands[f];
                    isl.view = match &isl.view {
                        View::Menu { item, title, entries, root } if entries != root => {
                            View::Menu { item: *item, title: title.clone(), entries: root.clone(), root: root.clone() }
                        }
                        _ => View::Main,
                    };
                    isl.invalidate(false, true);
                }
            }
        }
    }

    fn next_deadline(&self) -> Instant {
        let islands = self.islands.iter().filter_map(Island::next_deadline);
        islands.chain(self.next_stats).chain([self.next_clock]).min().unwrap_or(self.next_clock)
    }

    fn tick(&mut self) {
        let now = Instant::now();
        for i in &mut self.islands {
            i.tick(now);
        }
        if self.islands.iter_mut().fold(false, |due, i| i.take_pin(now) | due) {
            self.act(Action::Pin, 0.0, None);
        }
        if now >= self.next_clock {
            self.next_clock = next_minute();
            self.all(true, true);
        }
        let visible = self.islands.iter().any(Island::panel_visible);
        match (visible, self.next_stats) {
            (false, _) => self.next_stats = None,
            (true, None) => self.stats(now),
            (true, Some(t)) if now >= t => self.stats(now),
            _ => {}
        }
    }

    fn stats(&mut self, now: Instant) {
        let first = self.next_stats.is_none();
        self.next_stats = Some(now + STATS_EVERY);
        self.model.sys = self.sampler.sample(&self.model.sys);
        let _ = self.net_poke.send(());
        if first {
            if let Some(b) = &self.bus {
                b.refresh_media(self.tx.clone());
            }
        }
        self.all(false, true);
    }

    fn flush(&mut self) {
        let App { islands, model, icons, cfg, compositor, qh, .. } = self;
        for i in islands {
            i.flush(model, icons, cfg, compositor, qh);
        }
    }

    fn island_at(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.islands.iter().position(|i| i.surface() == surface)
    }

    fn set_cursor(&mut self, shape: Shape) {
        if self.cursor != Some(shape) {
            if let Some(d) = &self.shape_device {
                d.set_shape(self.enter_serial, shape);
                self.cursor = Some(shape);
            }
        }
    }
}

fn rounded(src: ImageSurface, r: f64) -> ImageSurface {
    let (w, h) = (src.width() as f64, src.height() as f64);
    let Ok(out) = ImageSurface::create(cairo::Format::ARgb32, src.width(), src.height()) else { return src };
    let Ok(cr) = cairo::Context::new(&out) else { return src };
    cr.arc(w - r, r, r, -1.57, 0.0);
    cr.arc(w - r, h - r, r, 0.0, 1.57);
    cr.arc(r, h - r, r, 1.57, 3.14);
    cr.arc(r, r, r, 3.14, 4.71);
    cr.close_path();
    cr.clip();
    if cr.set_source_surface(&src, 0.0, 0.0).is_ok() {
        let _ = cr.paint();
    }
    drop(cr);
    out
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(i) = self.island_at(surface) {
            self.islands[i].frame_done();
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.islands.retain(|i| i.output != output);
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.islands.retain(|i| &i.layer != layer);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(i) = self.islands.iter_mut().find(|i| &i.layer == layer) {
            i.configured();
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat.get_pointer(qh, &seat) {
                self.relative_pointer = self.relative.get_relative_pointer(&pointer, qh).ok();
                self.shape_device = self.cursor_shapes.as_ref().map(|m| m.get_shape_device(&pointer, qh));
                self.pointer = Some(pointer);
            }
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer {
            self.pointer = None;
            self.relative_pointer = None;
            self.shape_device = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            let Some(idx) = self.island_at(&e.surface) else { continue };
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    self.enter_serial = serial;
                    self.cursor = None;
                    self.under_pointer = Some(e.surface.clone());
                    self.islands[idx].enter(e.position);
                }
                PointerEventKind::Leave { .. } => {
                    self.under_pointer = None;
                    self.islands[idx].leave();
                }
                PointerEventKind::Motion { .. } => {
                    if let Outcome::Act(a, f) = self.islands[idx].motion(e.position) {
                        self.act(a, f, Some(idx));
                    }
                }
                PointerEventKind::Press { button, .. } => {
                    if let Outcome::Act(a, f) = self.islands[idx].press(button) {
                        self.act(a, f, Some(idx));
                    }
                }
                PointerEventKind::Release { .. } => match self.islands[idx].release() {
                    Outcome::Act(a, f) => self.act(a, f, Some(idx)),
                    Outcome::Moved => self.save(),
                    Outcome::None => {}
                },
                PointerEventKind::Axis { vertical, .. } => {
                    self.scroll += if vertical.value120 != 0 { vertical.value120 as f64 / 120.0 } else { vertical.absolute / 15.0 };
                    let notches = self.scroll.trunc();
                    self.scroll -= notches;
                    if notches != 0.0 {
                        match self.islands[idx].scroll_kind() {
                            Scroll::Volume => {
                                let sign = if notches > 0.0 { '-' } else { '+' };
                                spawn_sh(&format!("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ {}%{sign}", 5.0 * notches.abs()));
                            }
                            Scroll::Tags => {
                                let f = if notches > 0.0 { "viewtoright_have_client" } else { "viewtoleft_have_client" };
                                mango::dispatch(f);
                            }
                        }
                    }
                }
            }
            let shape = if self.islands[idx].hovering_target() { Shape::Pointer } else { Shape::Default };
            self.set_cursor(shape);
        }
    }
}

impl RelativePointerHandler for App {
    fn relative_pointer_motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ZwpRelativePointerV1,
                               _: &wl_pointer::WlPointer, e: RelativeMotionEvent) {
        for i in &mut self.islands {
            i.relative(e.delta);
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
delegate_dispatch2!(App);

fn socket_path() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR").map_or("/tmp".into(), PathBuf::from).join(format!(
        "island-{}.sock", env::var("WAYLAND_DISPLAY").unwrap_or_default()))
}

fn run_client(cmd: &str) -> ! {
    let sent = UnixStream::connect(socket_path()).and_then(|mut s| writeln!(s, "{cmd}"));
    if let Err(e) = sent {
        eprintln!("island: not running? {e}");
        std::process::exit(1);
    }
    std::process::exit(0)
}

/// One instance per display, and the way `island pin` reaches it. Accepting on
/// a parked thread costs nothing until someone connects.
fn control_socket(tx: Tx) -> Result<(), String> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err("already running".into());
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            let mut reader = BufReader::new(stream.take(64));
            if reader.read_line(&mut line).is_ok() {
                let _ = tx.send(Msg::Command(line.trim().to_owned()));
            }
        }
    });
    Ok(())
}

/// Parked in read(2) until matugen rewrites the palette.
fn watch_theme(tx: Tx) {
    thread::spawn(move || unsafe {
        let Ok(dir) = std::ffi::CString::new(theme::dir().to_string_lossy().as_bytes()) else { return };
        let fd = libc::inotify_init1(libc::IN_CLOEXEC);
        if fd < 0 || libc::inotify_add_watch(fd, dir.as_ptr(), libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO) < 0 {
            return eprintln!("island: can't watch {dir:?}; wallpaper colors won't update live");
        }
        let mut buf = [0u8; 4096];
        while libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) > 0 {
            // matugen writes several files in a burst; read the settled result once.
            thread::sleep(Duration::from_millis(120));
            let _ = tx.send(Msg::Theme);
        }
    });
}

fn main() {
    if let Some(cmd) = env::args().nth(1) {
        if matches!(cmd.as_str(), "pin" | "toggle" | "tuck" | "reset" | "quit") {
            run_client(&cmd);
        }
        eprintln!("usage: island [pin|tuck|reset|quit]");
        std::process::exit(2);
    }

    let (tx, rx) = channel::channel::<Msg>();
    if let Err(e) = control_socket(tx.clone()) {
        eprintln!("island: {e}");
        std::process::exit(1);
    }

    let conn = Connection::connect_to_env().expect("not on a Wayland session");
    let (globals, queue) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();

    let mut event_loop: EventLoop<App> = EventLoop::try_new().expect("event loop");
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle()).expect("wayland source");
    event_loop.handle().insert_source(rx, |e, _, app: &mut App| {
        if let channel::Event::Msg(m) = e {
            app.handle(m);
        }
    }).expect("message source");

    let net_poke = net::spawn(tx.clone());
    let mut sampler = sys::Sampler::new();
    sampler.prime();
    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).expect("wl_compositor"),
        layer_shell: LayerShell::bind(&globals, &qh).expect("layer shell"),
        shm: Shm::bind(&globals, &qh).expect("wl_shm"),
        relative: RelativePointerState::bind(&globals, &qh),
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        pointer: None, relative_pointer: None, shape_device: None, enter_serial: 0, cursor: None,
        qh,
        islands: Vec::new(),
        under_pointer: None,
        model: Model { monitors: Vec::new(), clients: Vec::new(), sys: sys::Snapshot::default(), audio: None,
                       media: None, art_small: None, art_large: None, net: net::State::Unknown, tray: Vec::new(),
                       pal: theme::load() },
        icons: Icons::new(),
        cfg: Config::load(),
        state: State::load(),
        sampler,
        bus: bus::spawn(tx.clone()),
        net_poke,
        tx: tx.clone(),
        next_stats: None,
        next_clock: next_minute(),
        scroll: 0.0,
        quit: false,
    };
    mango::watch(tx.clone());
    audio::spawn(tx.clone());
    watch_theme(tx);

    while !app.quit {
        let wait = app.next_deadline().saturating_duration_since(Instant::now());
        if let Err(e) = event_loop.dispatch(Some(wait), &mut app) {
            eprintln!("island: event loop: {e}");
            break;
        }
        app.tick();
        app.flush();
    }
    let _ = std::fs::remove_file(socket_path());
    let _ = conn.flush();
}
