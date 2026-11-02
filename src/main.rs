//! A Dynamic Island for MangoWC. See README.md.
mod audio;
mod bus;
mod icons;
mod island;
mod modules;
mod net;
mod notify;
mod palette;
mod pen;
mod settings;
mod sys;
mod theme;
mod tools;
mod wm;

use cairo::ImageSurface;
use island::{Island, Outcome};
use notify::Note;
use palette::Palette;
use pen::{Action, Scroll, View};
use settings::{Config, State};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle, channel},
        calloop_wayland_source::WaylandSource,
        client::{Connection, QueueHandle, globals::registry_queue_init,
                 protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface}},
        protocols::wp::{
            cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
            relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
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
    collections::HashMap,
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
/// Bounds a `dmenu` request, which is anything a local script cares to pipe in.
const MAX_REQUEST: u64 = 8 << 20;

pub enum Msg {
    Monitors(Vec<wm::Monitor>),
    Clients(Vec<wm::Client>),
    Custom(String, String),
    Audio(audio::State),
    Sys(sys::Snapshot),
    Media(Option<bus::Track>),
    Net(net::State),
    Tray(Vec<bus::TrayItem>),
    Menu(usize, Vec<bus::MenuEntry>),
    Theme,
    Command(String),
    Items(u64, Vec<tools::Item>),
    Notify(Note),
    NoteClosed(u32),
    Dmenu(palette::Dmenu, String, bool, Vec<tools::Item>),
    Quit,
}

/// Everything on screen comes from here; workers only ever feed it messages.
pub struct Model {
    pub monitors: Vec<wm::Monitor>,
    pub clients: Vec<wm::Client>,
    pub custom: HashMap<String, String>,
    pub sys: sys::Snapshot,
    pub audio: Option<audio::State>,
    pub media: Option<bus::Track>,
    pub art_small: Option<ImageSurface>,
    pub art_large: Option<ImageSurface>,
    pub net: net::State,
    pub tray: Vec<(bus::TrayItem, Option<(ImageSurface, bool)>)>,
    pub pal: theme::Palette,
    pub palette: Option<Palette>,
    pub notes: notify::Center,
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
    loop_handle: LoopHandle<'static, App>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    /// Output the palette is open on.
    palette_on: Option<String>,
    generation: u64,

    islands: Vec<Island>,
    under_pointer: Option<wl_surface::WlSurface>,
    model: Model,
    icons: Icons,
    tray_items: Vec<bus::TrayItem>,
    cfg: Config,
    state: State,
    stats: mpsc::Sender<sys::Cmd>,
    bus: Option<bus::Bus>,
    wm: Box<dyn wm::Backend>,
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
        let mut rescaled = false;
        for output in self.outputs.outputs().collect::<Vec<_>>() {
            let Some(info) = self.outputs.info(&output) else { continue };
            let Some(name) = info.name.clone() else { continue };
            let size = info.logical_size.or_else(|| info.modes.iter().find(|m| m.current).map(|m| m.dimensions));
            let Some((w, h)) = size else { continue };
            let out = (w as f64, h as f64);
            let scale = info.scale_factor.max(1);
            if let Some(i) = self.islands.iter_mut().find(|i| i.output == output) {
                rescaled |= i.scale != scale;
                if i.out != out || i.scale != scale {
                    i.set_output(out, scale);
                }
                continue;
            }
            let surface = self.compositor.create_surface(&qh);
            let layer = self.layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some(NAMESPACE), Some(&output));
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            // Mango gives zone-0 layers a rectangular drop shadow, decided at map
            // time. It would outline the whole transparent surface.
            layer.set_exclusive_zone(-1);
            let island = Island::new(output.clone(), name.clone(), out, scale, layer, &self.shm,
                                     self.state.pos.get(&name).copied(), self.state.pinned, self.state.tucked);
            if let Some(island) = island {
                let size = island.surface_size();
                island.layer.set_size(size.0 as u32, size.1 as u32);
                island.layer.commit();
                self.islands.push(island);
                rescaled = true;
            }
        }
        if rescaled {
            self.set_art();
            self.set_tray();
        }
    }

    fn focused_island(&self) -> Option<usize> {
        let active = self.model.monitors.iter().find(|m| m.active)?;
        self.islands.iter().position(|i| i.name == active.name).or((!self.islands.is_empty()).then_some(0))
    }

    fn tool_ctx(&self) -> tools::Ctx {
        tools::Ctx { cfg: self.cfg.clone(), clients: self.model.clients.clone() }
    }

    fn open_palette(&mut self, tool: Option<&str>) {
        let entries = tools::enabled(&self.cfg);
        let wanted = tool.and_then(|t| entries.iter().position(|e| e.name() == t));
        if let Some(open) = &mut self.model.palette {
            // The same key again closes it; another tool's key switches to it.
            let same = wanted.is_none_or(|w| w == open.current) && open.dmenu.is_none();
            if same {
                return self.close_palette();
            }
            let outcome = wanted.map_or(palette::Outcome::Nothing, |w| open.switch(w));
            return self.palette_outcome(outcome);
        }
        if entries.is_empty() || tool.is_some() && wanted.is_none() {
            return;
        }
        let Some(island) = self.focused_island() else { return };
        self.generation += 1;
        let palette = Palette::new(entries, wanted.unwrap_or(0), self.generation);
        tools::load(&palette.entries[palette.current], self.generation, self.tool_ctx(), self.tx.clone());
        self.model.palette = Some(palette);
        self.palette_on = Some(self.islands[island].name.clone());
        self.islands[island].open_palette();
    }

    fn close_palette(&mut self) {
        if let Some(p) = self.model.palette.take() {
            self.generation = self.generation.max(p.generation) + 1;
        }
        if let Some(name) = self.palette_on.take() {
            self.islands.iter_mut().filter(|i| i.name == name).for_each(Island::close_palette);
        }
        self.icons.forget_files();
        // A long list and its icons leave a lot of freed heap behind.
        let _ = self.stats.send(sys::Cmd::Release);
    }

    fn palette_redraw(&mut self) {
        let Some(name) = &self.palette_on else { return };
        self.islands.iter_mut().filter(|i| i.name == *name).for_each(|i| i.invalidate(false, true));
    }

    fn palette_outcome(&mut self, outcome: palette::Outcome) {
        use palette::Outcome as O;
        match outcome {
            O::Nothing => {}
            O::Redraw => self.palette_redraw(),
            O::Close => self.close_palette(),
            O::Run(act) => {
                self.close_palette();
                self.run(act);
            }
            O::Answer(then, text) => {
                self.close_palette();
                tools::run_then(then, text, self.tx.clone());
            }
            O::Reload => {
                if let Some(p) = &self.model.palette {
                    tools::load(&p.entries[p.current], p.generation, self.tool_ctx(), self.tx.clone());
                }
                self.palette_redraw();
            }
        }
    }

    fn run(&mut self, act: tools::Act) {
        use tools::Act as A;
        match act {
            A::Sh(cmd) => tools::spawn_detached(&cmd),
            A::Launch { id, cmd } => {
                #[cfg(feature = "apps")]
                tools::apps::record(id);
                #[cfg(not(feature = "apps"))]
                let _ = id;
                tools::spawn_detached(&cmd);
            }
            A::Focus(window) => self.wm.focus(window),
            A::Quit => self.wm.quit(),
            A::Task(task) => tools::run_task(task, self.tool_ctx(), self.tx.clone()),
            A::Confirm(..) | A::Ask { .. } | A::Reply(_) => {}
        }
    }

    fn key(&mut self, e: KeyEvent) {
        let mods = self.mods;
        let Some(p) = &mut self.model.palette else { return };
        let outcome = p.key(e.keysym, e.utf8.as_deref(), mods);
        self.palette_outcome(outcome);
    }

    fn end_toasts(&mut self) {
        self.islands.iter_mut().for_each(Island::end_toast);
    }

    fn note_gone(&mut self, id: u32, reason: u32) {
        if let Some(bus) = self.bus.as_ref().filter(|_| id < 0xfff0_0000) {
            bus.note_closed(id, reason);
        }
    }

    /// Move toasts along: expire the one showing, then show the next if there's room.
    fn pump_notes(&mut self, now: Instant) {
        if self.model.notes.until.is_some_and(|t| t <= now) {
            if self.islands.iter().any(Island::hovered) {
                self.model.notes.until = Some(now + Duration::from_secs(1));
            } else if let Some(note) = self.model.notes.finish() {
                self.note_gone(note.id, 1);
                self.end_toasts();
            }
        }
        if self.model.notes.showing.is_some() || self.model.notes.queue.is_empty() || self.model.palette.is_some() {
            return;
        }
        let Some(i) = self.focused_island() else { return };
        // The history in an open panel is already showing them.
        if self.islands[i].wants_panel() {
            return self.model.notes.queue.clear();
        }
        let times = &self.cfg.notify;
        let (low, normal) = (Duration::from_secs(times.low), Duration::from_secs(times.normal));
        if let Some(note) = self.model.notes.next(low, normal, now) {
            let critical = note.urgency == 2;
            self.islands[i].show_toast(critical);
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

    /// Sharp on the densest output; the others scale it down.
    fn raster_scale(&self) -> i32 {
        self.islands.iter().map(|i| i.scale).max().unwrap_or(1)
    }

    fn set_art(&mut self) {
        let path = self.model.media.as_ref().and_then(|t| t.art.clone());
        let scale = self.raster_scale();
        let load = |p: &PathBuf, px: i32| icons::load_art(p, px * scale);
        self.model.art_small = path.as_ref().and_then(|p| load(p, 20)).map(|s| rounded(s, 5.0 * scale as f64));
        self.model.art_large = path.as_ref().and_then(|p| load(p, 58));
    }

    fn set_tray(&mut self) {
        let px = 18 * self.raster_scale();
        let icons = &mut self.icons;
        self.model.tray = self.tray_items.iter().map(|i| (i.clone(), icons.tray(&i.icon, px))).collect();
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Monitors(m) => {
                if m != self.model.monitors {
                    self.model.monitors = m;
                    self.all(true, true);
                }
            }
            Msg::Clients(c) => {
                // Terminals rewrite their title constantly, and titles only show up
                // as hover tips, so those changes wait for the next hover.
                let moved = !wm::same_but_titles(&c, &self.model.clients);
                self.model.clients = c;
                if moved {
                    self.all(true, true);
                } else {
                    self.islands.iter_mut().for_each(Island::titles_changed);
                }
            }
            Msg::Items(generation, items) => {
                if let Some(p) = &mut self.model.palette {
                    p.set_items(generation, items);
                    self.palette_redraw();
                }
            }
            Msg::Notify(note) => {
                if self.cfg.notifications {
                    self.model.notes.add(note);
                    self.all(false, true);
                } else {
                    // Someone else is the daemon, so our own messages go through it.
                    tools::spawn_note(&note);
                }
            }
            Msg::NoteClosed(id) => {
                if self.model.notes.remove(id) {
                    self.end_toasts();
                }
                self.note_gone(id, 3);
                self.all(false, true);
            }
            Msg::Dmenu(dmenu, prompt, password, items) => {
                self.close_palette();
                let Some(island) = self.focused_island() else { return };
                self.model.palette = Some(Palette::dmenu(prompt, password, items, dmenu));
                self.palette_on = Some(self.islands[island].name.clone());
                self.islands[island].open_palette();
            }
            Msg::Custom(name, text) => {
                self.model.custom.insert(name, text);
                self.all(true, true);
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
            Msg::Sys(s) => {
                if s != self.model.sys {
                    self.model.sys = s;
                    self.all(false, true);
                }
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
                self.tray_items = items;
                self.set_tray();
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
                if let Some(pal) = theme::reload(&self.cfg.palette) {
                    self.model.pal = pal;
                    self.all(true, true);
                }
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
            "dnd" => self.act(Action::Dnd, 0.0, None),
            "open" => self.open_palette(None),
            "close" => self.close_palette(),
            "reload" => {
                self.cfg = Config::load();
                self.model.pal = theme::load(&self.cfg.palette);
                self.all(true, true);
            }
            "quit" => self.quit = true,
            "" => {} // a second instance checking whether we're alive
            _ => match c.split_once(' ') {
                Some(("open", tool)) => self.open_palette(Some(tool)),
                Some(("shot", kind)) => {
                    if let Some(task) = tools::shot_task(kind) {
                        self.run(tools::Act::Task(task));
                    }
                }
                _ => eprintln!("island: unknown command {c}"),
            },
        }
    }

    fn act(&mut self, action: Action, frac: f64, from: Option<usize>) {
        let tray = |m: &Model, i: usize| m.tray.get(i).map(|(item, _)| item.clone());
        match action {
            Action::None => {}
            Action::View { tag, mon } => self.wm.view(tag, &mon),
            Action::MoveTo(tag) => self.wm.send_to(tag),
            Action::Focus(id) => self.wm.focus(id),
            Action::Sh(cmd) if cmd.is_empty() => {}
            Action::Sh(cmd) => spawn_sh(&cmd),
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
            Action::Open(tool) => self.open_palette(Some(tool)),
            Action::PaletteRow(row) => {
                let Some(p) = &mut self.model.palette else { return };
                p.selected = row;
                let outcome = p.choose();
                self.palette_outcome(outcome);
            }
            Action::PaletteTool(i) => {
                let Some(p) = &mut self.model.palette else { return };
                let outcome = p.switch(i);
                self.palette_outcome(outcome);
            }
            Action::NoteAction(id, key) => {
                if let Some(bus) = self.bus.as_ref().filter(|_| id < 0xfff0_0000) {
                    bus.note_action(id, key);
                }
                self.act(Action::NoteDismiss(id), 0.0, None);
            }
            Action::NoteDismiss(id) => {
                if self.model.notes.remove(id) {
                    self.end_toasts();
                }
                self.note_gone(id, 2);
                self.all(false, true);
            }
            Action::NoteClear => {
                self.model.notes.history.clear();
                self.all(false, true);
            }
            Action::Dnd => {
                self.model.notes.dnd = !self.model.notes.dnd;
                self.state.dnd = self.model.notes.dnd;
                self.save();
                self.all(false, true);
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
        islands.chain(self.next_stats).chain(self.model.notes.until).chain([self.next_clock]).min().unwrap_or(self.next_clock)
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
        self.pump_notes(now);
        let visible = self.islands.iter().any(Island::wants_stats);
        match (visible, self.next_stats) {
            (false, None) => {}
            (false, Some(_)) => {
                self.next_stats = None;
                let _ = self.stats.send(sys::Cmd::Release);
            }
            (true, None) => self.stats(now),
            (true, Some(t)) if now >= t => self.stats(now),
            _ => {}
        }
    }

    fn stats(&mut self, now: Instant) {
        let first = self.next_stats.is_none();
        self.next_stats = Some(now + STATS_EVERY);
        let _ = self.stats.send(sys::Cmd::Sample);
        let _ = self.net_poke.send(());
        if first {
            if let Some(b) = &self.bus {
                b.refresh_media(self.tx.clone());
            }
        }
        // Refreshes the progress bar, which moves without any event of its own.
        if self.model.media.as_ref().is_some_and(|t| t.playing) {
            self.all(false, true);
        }
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
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let handle = self.loop_handle.clone();
            let repeat = Box::new(|app: &mut App, _: &wl_keyboard::WlKeyboard, e: KeyEvent| app.key(e));
            self.keyboard = self.seat.get_keyboard_with_repeat(qh, &seat, None, handle, repeat).ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat.get_pointer(qh, &seat) {
                self.relative_pointer = self.relative.get_relative_pointer(&pointer, qh).ok();
                self.shape_device = self.cursor_shapes.as_ref().map(|m| m.get_shape_device(&pointer, qh));
                self.pointer = Some(pointer);
            }
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard {
            self.keyboard = None;
        }
        if cap == Capability::Pointer {
            self.pointer = None;
            self.relative_pointer = None;
            self.shape_device = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface,
             _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, e: KeyEvent) {
        self.key(e);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, e: KeyEvent) {
        self.key(e);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32,
                        mods: Modifiers, _: RawModifiers, _: u32) {
        self.mods = mods;
    }
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
                            Scroll::Tags => self.wm.cycle(notches > 0.0),
                            Scroll::Palette => {
                                if let Some(p) = &mut self.model.palette {
                                    p.scroll(notches as isize);
                                    self.palette_redraw();
                                }
                            }
                        }
                    }
                }
            }
            let isl = &self.islands[idx];
            let shape = if isl.dragging() { Shape::Grabbing } else if isl.hovering_target() { Shape::Pointer } else { Shape::Default };
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

/// `island dmenu`: pick a line from stdin, the way `rofi -dmenu` does, so scripts
/// written for rofi work by swapping the command. Exits 1 when cancelled.
fn run_dmenu(args: &[String]) -> ! {
    let (mut prompt, mut password, mut index) = (String::from("Search"), false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.trim_start_matches('-') {
            "p" => prompt = it.next().cloned().unwrap_or_default(),
            "password" => password = true,
            "format" => index = it.next().is_some_and(|f| f == "i"),
            "l" | "theme" | "mesg" | "theme-str" => drop(it.next()),
            _ => {}
        }
    }
    let mut input = String::new();
    if !password {
        let _ = std::io::stdin().read_to_string(&mut input);
    }
    let talk = || -> std::io::Result<String> {
        let mut s = UnixStream::connect(socket_path())?;
        writeln!(s, "dmenu\t{}\t{}\t{}", password as u8, index as u8, prompt.replace(['\t', '\n'], " "))?;
        s.write_all(input.as_bytes())?;
        s.shutdown(std::net::Shutdown::Write)?;
        let mut reply = String::new();
        s.read_to_string(&mut reply)?;
        Ok(reply)
    };
    match talk() {
        Ok(r) if !r.is_empty() => {
            println!("{r}");
            std::process::exit(0)
        }
        Ok(_) => std::process::exit(1),
        Err(e) => {
            eprintln!("island: not running? {e}");
            std::process::exit(1)
        }
    }
}

/// One line of a `dmenu` list. Rofi's `text\0icon\x1fvalue` form sets an icon.
fn dmenu_item(line: &str) -> tools::Item {
    let (text, meta) = line.split_once('\0').unwrap_or((line, ""));
    let icon = meta.strip_prefix("icon\u{1f}").map_or(tools::Icon::None, |v| {
        if v.starts_with('/') { tools::Icon::Image(v.into()) } else { tools::Icon::Named(v.into()) }
    });
    tools::Item::new(text, icon, tools::Act::Reply(text.to_owned()))
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
            let tx = tx.clone();
            // A dmenu client waits for its answer, so each gets a thread of its own.
            thread::spawn(move || {
                let Ok(mut out) = stream.try_clone() else { return };
                let mut reader = BufReader::new(stream.take(MAX_REQUEST));
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let line = line.trim_end_matches('\n');
                let Some(header) = line.strip_prefix("dmenu\t") else {
                    let _ = tx.send(Msg::Command(line.trim().to_owned()));
                    return;
                };
                let mut fields = header.splitn(3, '\t');
                let (password, index) = (fields.next() == Some("1"), fields.next() == Some("1"));
                let prompt = fields.next().unwrap_or("").to_owned();
                let mut body = String::new();
                let _ = reader.read_to_string(&mut body);
                let items = body.lines().map(dmenu_item).collect();
                let (reply, answer) = mpsc::channel();
                let _ = tx.send(Msg::Dmenu(palette::Dmenu { reply, index }, prompt, password, items));
                if let Ok(text) = answer.recv() {
                    let _ = out.write_all(text.as_bytes());
                }
            });
        }
    });
    Ok(())
}

/// Parked in read(2) until matugen rewrites the palette.
fn watch_theme(palette: &str, tx: Tx) {
    let dir = theme::expand(palette).parent().map(|d| d.to_owned()).unwrap_or_default();
    thread::spawn(move || unsafe {
        let Ok(dir) = std::ffi::CString::new(dir.to_string_lossy().as_bytes()) else { return };
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
        if cmd == "--gpu-sampler" {
            return sys::gpu_sampler();
        }
        let args: Vec<String> = env::args().skip(1).collect();
        match cmd.as_str() {
            "dmenu" => run_dmenu(&args[1..]),
            "pin" | "toggle" | "tuck" | "reset" | "reload" | "quit" | "dnd" | "open" | "close" | "shot" => run_client(&args.join(" ")),
            _ => {}
        }
        eprintln!("usage: island [pin|tuck|reset|reload|quit|dnd|close|open [tool]|shot <kind>|dmenu [-p prompt]]");
        std::process::exit(2);
    }

    let (tx, rx) = channel::channel::<Msg>();
    if let Err(e) = control_socket(tx.clone()) {
        eprintln!("island: {e}");
        // Success, so the autostart restart loop doesn't spin on a second copy.
        std::process::exit(0);
    }

    let conn = Connection::connect_to_env().expect("not on a Wayland session");
    let (globals, queue) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();

    let mut event_loop: EventLoop<App> = EventLoop::try_new().expect("event loop");
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle()).expect("wayland source");
    let loop_handle = event_loop.handle();
    event_loop.handle().insert_source(rx, |e, _, app: &mut App| {
        if let channel::Event::Msg(m) = e {
            app.handle(m);
        }
    }).expect("message source");

    let cfg = Config::load();
    let state = State::load();
    let net_poke = net::spawn(tx.clone());
    let wm = wm::detect();
    wm.watch(tx.clone());
    modules::custom::spawn(&cfg.custom, tx.clone());
    let cfg_notifications = cfg.notifications;
    watch_theme(&cfg.palette, tx.clone());
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
        loop_handle,
        keyboard: None,
        mods: Modifiers::default(),
        palette_on: None,
        generation: 0,
        islands: Vec::new(),
        under_pointer: None,
        model: Model { monitors: Vec::new(), clients: Vec::new(), custom: HashMap::new(), sys: sys::Snapshot::default(),
                       audio: None, media: None, art_small: None, art_large: None, net: net::State::Unknown,
                       tray: Vec::new(), pal: theme::load(&cfg.palette), palette: None,
                       notes: notify::Center::new(state.dnd) },
        icons: Icons::new(),
        tray_items: Vec::new(),
        cfg,
        state,
        stats: sys::spawn(tx.clone()),
        bus: bus::spawn(tx.clone(), cfg_notifications),
        wm,
        net_poke,
        tx: tx.clone(),
        next_stats: None,
        next_clock: next_minute(),
        scroll: 0.0,
        quit: false,
    };
    audio::spawn(tx);

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
