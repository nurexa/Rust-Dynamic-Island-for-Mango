//! A Dynamic Island for MangoWC. See README.md.
mod audio;
mod bluetooth;
mod brightness;
mod bus;
mod calendar;
mod canvas;
mod config_edit;
mod claude;
mod ddc;
mod doctor;
mod edges;
mod icons;
mod island;
mod modules;
mod net;
mod nightlight;
mod notify;
mod palette;
mod pen;
mod power;
mod prefs;
mod privacy;
mod scale;
mod settings;
mod sidebar;
mod symbols;
mod style;
mod sys;
mod theme;
mod timer;
mod tools;
mod viz;
mod wm;

use cairo::ImageSurface;
use edges::Edges;
use island::{Island, Outcome};
use notify::Note;
use palette::Palette;
use pen::{Action, Scroll, View};
use settings::{Config, State};
use sidebar::Sidebar;
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
/// Its own name, so compositor rules can blur these and leave the rest alone.
const BLURRED: &str = "island-blur";

#[derive(Default)]
struct Feeds {
    bluetooth: bool,
    privacy: bool,
    power: bool,
    ddc: bool,
    audio: bool,
}

fn namespace(cfg: &Config, style: style::Style) -> &'static str {
    if cfg.blurred(style) { BLURRED } else { NAMESPACE }
}
/// parec dies with the audio server, which is back in a few seconds.
const CAPTURE_RETRY: Duration = Duration::from_secs(5);
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
    /// Wi-Fi switched on or off; None without a card.
    Radio(Option<bool>),
    Levels(viz::Levels),
    /// Events for the range they were loaded for.
    Calendar((i64, i64), Vec<calendar::Occurrence>),
    Bluetooth(Option<bluetooth::State>),
    Tray(Vec<bus::TrayItem>),
    Menu(usize, Vec<bus::MenuEntry>),
    Theme,
    /// Processes with the camera open.
    Camera(Vec<String>),
    Battery(power::Battery),
    /// Monitors whose brightness ddcutil can set.
    Ddc(Vec<ddc::Screen>),
    /// A Claude Code hook event; a permission request carries where to send the answer.
    Claude(claude::Event, Option<mpsc::Sender<String>>),
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
    pub brightness: Option<u32>,
    pub media: Option<bus::Track>,
    pub art_small: Option<ImageSurface>,
    pub art_large: Option<ImageSurface>,
    pub net: net::State,
    pub radio: Option<bool>,
    pub levels: viz::Levels,
    /// parec is here, so visualizers get their room.
    pub can_capture: bool,
    pub calendar: calendar::View,
    pub sidebar_open: bool,
    pub bluetooth: Option<bluetooth::State>,
    pub tray: Vec<(bus::TrayItem, Option<(ImageSurface, bool)>)>,
    pub pal: theme::Palette,
    pub style: style::Style,
    pub palette: Option<Palette>,
    pub notes: notify::Center,
    pub wall: Option<modules::wallpaper::Wall>,
    pub timer: Option<timer::Timer>,
    pub camera: Vec<String>,
    pub battery: Option<power::Battery>,
    pub claude: Vec<claude::Session>,
    pub ddc: Vec<ddc::Screen>,
    /// The pill's brief flash is about power, not volume.
    pub flash_power: bool,
    /// The lowest battery warning given since the last charge, so each shows once.
    pub battery_warned: u8,
    /// The panel's list of outputs, apps and microphones is open.
    pub audio_open: bool,
    pub night: nightlight::Night,
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
    /// The frame's or bar's strips, for the styles that have any.
    edges: Vec<Edges>,
    sidebar: Option<Sidebar>,
    /// Resting on the frame's right border: when the sidebar opens, and where.
    sidebar_at: Option<(Instant, String)>,
    /// When an open sidebar's progress bar next moves.
    progress_at: Option<Instant>,
    capture: Option<viz::Capture>,
    /// When to try again, after capture died under us.
    capture_retry: Option<Instant>,
    had_panel: bool,
    /// The range the calendar's loading or showing.
    calendar_range: Option<(i64, i64)>,
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
    /// The watchers running. Each starts when something first shows what it feeds.
    feeds: Feeds,
    tx: Tx,
    next_stats: Option<Instant>,
    /// Permission cards waiting on a click: note id, the request, where the hook waits.
    claude_asks: Vec<(u32, claude::Event, mpsc::Sender<String>)>,
    /// Notes that focus a session's terminal when clicked.
    claude_notes: Vec<(u32, Vec<u32>)>,
    /// When a finished session next drops off the pill.
    claude_until: Option<Instant>,
    /// When the timer's text next changes, and whether that's counting seconds.
    timer_at: Option<Instant>,
    timer_fine: bool,
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
            let scale = scale::Scale { buffer: info.scale_factor.max(1), ui: self.cfg.ui.scale };
            let out = scale.units((w as f64, h as f64));
            if self.sidebar.as_ref().is_some_and(|s| s.output == output && !s.fits(out, scale)) {
                self.drop_sidebar();
            }
            let style = self.model.style;
            if let Some(n) = self.islands.iter().position(|i| i.output == output) {
                let i = &mut self.islands[n];
                let changed = i.out != out || i.scale != scale;
                rescaled |= i.scale != scale;
                if !changed || style.insets() == Default::default() {
                    if changed {
                        i.set_output(out, scale);
                    }
                    continue;
                }
                // Strips fit one mode, and the island has to stay stacked over them: both start again.
                self.islands.remove(n);
                self.edges.retain(|e| e.output != output);
            }
            let edges = Edges::new(&output, &name, out, scale, style, namespace(&self.cfg, style), &self.shm, &self.layer_shell, &self.compositor, &qh);
            self.edges.extend(edges);
            let surface = self.compositor.create_surface(&qh);
            let layer = self.layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some(namespace(&self.cfg, style)), Some(&output));
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            // Mango gives zone-0 layers a rectangular drop shadow, decided at map
            // time. It would outline the whole transparent surface.
            layer.set_exclusive_zone(-1);
            let island = Island::new(output.clone(), name.clone(), out, scale, layer, &self.shm, style,
                                     self.state.pos.get(&name).map(|p| scale.units(*p)), self.state.pinned, self.state.tucked);
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

    fn set_style(&mut self, style: style::Style) {
        if self.model.style != style {
            self.rebuild_surfaces(style);
        }
    }

    /// What nothing shows costs no thread, bus connection or wakeup. Switched off, one
    /// already running lasts until the island next starts.
    fn start_feeds(&mut self) {
        let (cfg, on, tx) = (&self.cfg, &mut self.feeds, &self.tx);
        let controls = cfg.uses("controls");
        let first = |flag: &mut bool, wanted: bool| wanted && !std::mem::replace(flag, true);
        if first(&mut on.bluetooth, cfg.uses("bluetooth") || controls && !cfg.off("bluetooth")) {
            bluetooth::spawn(tx.clone());
        }
        if first(&mut on.privacy, cfg.uses("privacy")) {
            privacy::spawn(tx.clone());
        }
        if first(&mut on.power, cfg.uses("battery")) {
            power::spawn(tx.clone());
        }
        if first(&mut on.ddc, cfg.uses("brightness") || controls && !cfg.off("brightness")) {
            ddc::spawn(tx.clone());
        }
        if first(&mut on.audio, cfg.uses("audio") || controls && !cfg.off("audio")) {
            audio::spawn(tx.clone());
        }
    }

    /// config.toml and what was chosen over it, read again and put on screen.
    fn configure(&mut self) {
        let cfg = Config::load(&self.state);
        let style = self.state.style(cfg.style);
        // These are fixed when a surface is made.
        let remake = style != self.model.style || cfg.ui.scale != self.cfg.ui.scale
            || cfg.blurred(style) != self.cfg.blurred(self.model.style);
        if cfg.notifications != self.cfg.notifications && let Some(bus) = &self.bus {
            bus.set_notifications(cfg.notifications, &self.tx);
        }
        if (&cfg.font, &cfg.icon_font) != (&self.cfg.font, &self.cfg.icon_font) {
            pen::fonts_changed();
        }
        if cfg.palette != self.cfg.palette {
            watch_theme(&cfg.palette, self.tx.clone());
        }
        // A new module starts now; one that changed, when the island next starts.
        let new: Vec<_> = cfg.custom.iter().filter(|m| !m.exec.is_empty()
            && !self.cfg.custom.iter().any(|o| o.name == m.name && !o.exec.is_empty())).cloned().collect();
        modules::custom::spawn(&new, self.tx.clone());
        self.cfg = cfg;
        if remake {
            self.rebuild_surfaces(style);
        }
        if self.cfg.sidebar.is_empty() {
            self.drop_sidebar();
        }
        self.model.can_capture = !self.cfg.visualizer.is_empty() && tools::on_path("parec");
        self.load_calendar();
        self.model.pal = theme::load(&self.cfg.palette);
        self.model.brightness = brightness::read(&self.cfg.commands);
        self.model.night.configure(&self.cfg.nightlight);
        self.night_apply();
        self.start_feeds();
        self.all(true, true);
    }

    fn choose(&mut self, choices: &[(&str, &str)]) {
        let written = Config::written();
        for (key, value) in choices {
            if !self.state.choose(&written, key, value) {
                eprintln!("island: can't set {key} to {value}");
            }
        }
        self.configure();
        self.save();
    }

    /// A change from Settings: into config.toml, on screen, and back on the same page.
    fn edit(&mut self, changes: &[config_edit::Change], up: usize) {
        let Some(place) = self.model.palette.as_ref().map(|p| p.place(up)) else { return };
        let saved = config_edit::write(changes);
        if saved.is_ok() {
            for (key, _) in changes {
                if key.len() == 1 && key[0] == "style" {
                    self.state.style = None;
                }
                // What the file now says wins over a choice saved over what it said before.
                for chosen in tools::settings::overrides(key) {
                    self.state.chosen.remove(chosen);
                }
            }
            // Before the list is built again: it reads this.
            self.save();
            self.configure();
        }
        // New surfaces closed it.
        if self.model.palette.is_none() {
            self.open_palette(Some("settings"));
        }
        let (ctx, notice) = (self.tool_ctx(), match saved {
            Ok(()) => ("Saved to config.toml".to_owned(), true),
            Err(e) => (e, false),
        });
        let Some(p) = &mut self.model.palette else { return };
        p.notice = Some(notice);
        p.replay = Some(place);
        p.generation += 1;
        if let Some(settings) = tools::builtin("settings") {
            tools::load(&settings, p.generation, ctx, self.tx.clone());
        }
        self.palette_redraw();
    }

    fn rebuild_surfaces(&mut self, style: style::Style) {
        self.close_palette();
        self.model.notes.cancel_reply(Instant::now());
        self.save();
        self.model.style = style;
        // New surfaces, so the edges are always under the island.
        self.under_pointer = None;
        self.drop_sidebar();
        self.sidebar_at = None;
        self.islands.clear();
        self.edges.clear();
        let _ = self.stats.send(sys::Cmd::Release);
        self.sync_outputs();
        if let Some(critical) = self.model.notes.showing.as_ref().map(|t| t.note.urgency == 2) {
            if let Some(i) = self.focused_island() {
                self.islands[i].show_toast(critical);
            }
        }
    }

    fn drop_sidebar(&mut self) {
        if self.sidebar.take().is_some() {
            self.model.sidebar_open = false;
            self.icons.forget_files();
            // Its content was a screen's height of pixels.
            let _ = self.stats.send(sys::Cmd::Release);
            self.progress_at = None;
            self.all(false, true);
            self.edges_changed();
        }
    }

    fn open_sidebar(&mut self, island: usize, by_hover: bool) {
        if self.cfg.sidebar.is_empty() {
            return;
        }
        let i = &self.islands[island];
        self.sidebar = Sidebar::open(&i.output, &i.name, i.out, i.scale, self.model.style, by_hover, namespace(&self.cfg, self.model.style),
                                     &self.shm, &self.layer_shell, &self.compositor, &self.qh);
        self.sidebar_shown();
    }

    /// Freshen what only gets read while something is looking at it.
    fn sidebar_shown(&mut self) {
        if self.sidebar.is_none() {
            return;
        }
        self.model.sidebar_open = true;
        self.model.calendar.offset = 0;
        self.model.calendar.selected = None;
        self.load_calendar();
        let _ = self.net_poke.send(());
        self.model.brightness = brightness::read(&self.cfg.commands);
        if let Some(b) = &self.bus {
            b.refresh_media(self.tx.clone());
        }
        self.all(false, true);
        self.edges_changed();
    }

    fn toggle_sidebar(&mut self, from: Option<usize>) {
        let target = from.or_else(|| self.focused_island());
        let open = self.sidebar.as_mut().filter(|s| s.is_open());
        if let Some(s) = open {
            let here = target.is_none_or(|i| self.islands[i].name == s.name);
            if here {
                s.close();
                self.model.sidebar_open = false;
                self.all(false, true);
                self.edges_changed();
                return;
            }
        }
        let Some(i) = target else { return };
        match self.sidebar.as_mut().filter(|s| s.name == self.islands[i].name) {
            // Caught on its way out: turn it round.
            Some(s) => {
                s.reopen();
                self.sidebar_shown();
            }
            None => self.open_sidebar(i, false),
        }
    }

    fn calendar_placed(&self) -> bool {
        [&self.cfg.sidebar, &self.cfg.panel].iter().any(|l| l.iter().any(|n| n == "calendar"))
    }

    fn load_calendar(&mut self) {
        if !self.calendar_placed() {
            return;
        }
        let first = self.cfg.calendar.first_weekday();
        let range = calendar::range(calendar::now().0, self.model.calendar.offset, first);
        self.calendar_range = Some(range);
        if self.cfg.calendar.dir.is_empty() {
            self.model.calendar.events.clear();
            return;
        }
        let (dir, tx) = (theme::expand(&self.cfg.calendar.dir), self.tx.clone());
        thread::spawn(move || tx.send(Msg::Calendar(range, calendar::load(&dir, range))));
    }

    /// Capture runs exactly while there's music and a visualizer someone can see.
    fn sync_capture(&mut self) {
        let now = Instant::now();
        if self.capture.as_mut().is_some_and(viz::Capture::ended) {
            self.capture = None;
            self.capture_retry = Some(now + CAPTURE_RETRY);
            self.set_levels([0.0; viz::BANDS]);
        }
        if self.capture_retry.is_some_and(|t| t > now) {
            return;
        }
        self.capture_retry = None;
        let m = &self.model;
        let covered = |name: &str| wm::covered(&m.clients, &m.monitors, name);
        let wanted = m.can_capture && m.media.as_ref().is_some_and(|t| t.playing)
            && (self.islands.iter().any(|i| i.wants_levels() && !covered(&i.name))
                || self.sidebar.as_ref().is_some_and(|s| s.wants_levels() && !covered(&s.name)));
        if wanted == self.capture.is_some() {
            return;
        }
        self.capture = wanted.then(|| viz::Capture::start(self.tx.clone())).flatten();
        if wanted && self.capture.is_none() {
            self.model.can_capture = false; // it won't start: stop trying, and stop leaving room
            self.all(true, true);
        }
        if !wanted {
            self.set_levels([0.0; viz::BANDS]);
            let _ = self.stats.send(sys::Cmd::Release);
        }
    }

    fn set_levels(&mut self, levels: viz::Levels) {
        self.model.levels = levels;
        self.islands.iter_mut().for_each(Island::levels_changed);
        if let Some(s) = &mut self.sidebar {
            s.levels_changed();
        }
    }

    fn focused_island(&self) -> Option<usize> {
        let active = self.model.monitors.iter().find(|m| m.active)?;
        self.islands.iter().position(|i| i.name == active.name).or((!self.islands.is_empty()).then_some(0))
    }

    fn tool_ctx(&self) -> tools::Ctx {
        let query = self.model.palette.as_ref().map_or(String::new(), |p| p.query.clone());
        tools::Ctx { cfg: self.cfg.clone(), style: self.model.style, clients: self.model.clients.clone(), query }
    }

    fn open_palette(&mut self, tool: Option<&str>) {
        // The palette's field takes the keyboard from a notification's.
        self.model.notes.cancel_reply(Instant::now());
        self.islands.iter_mut().for_each(|i| i.set_typing(false));
        let mut entries = tools::enabled(&self.cfg);
        // Not a tab, so it opens even with every tool switched off.
        if tool == Some("settings") && !entries.iter().any(|e| e.name() == "settings") {
            entries.extend(tools::builtin("settings"));
            if let Some(open) = &self.model.palette {
                if open.entries.get(open.current).is_none_or(|e| e.name() != "settings") {
                    self.close_palette();
                }
            }
        }
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
        let mut palette = Palette::new(entries, wanted.unwrap_or(0), self.generation);
        palette.web = self.cfg.web_search.clone();
        palette.calc = self.cfg.calc();
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
            O::Run(tools::Act::Edit(e)) => self.edit(&e.changes, e.up),
            O::Run(act) => {
                self.close_palette();
                self.run(act);
            }
            O::Answer(tools::Then::Setting(field), text) => match field.changes(&text) {
                Ok(changes) => self.edit(&changes, field.up),
                Err(e) => {
                    // Still in the field, with what was typed, to put right.
                    if let Some(p) = &mut self.model.palette {
                        let then = Some(tools::Then::Setting(field));
                        p.ask = Some(palette::Ask { label: e.clone(), password: false, then });
                        p.notice = Some((e, false));
                        p.query = text;
                    }
                    self.palette_redraw();
                }
            },
            #[cfg(feature = "wifi")] // the only other text field
            O::Answer(then, text) => {
                self.close_palette();
                tools::run_then(then, text, self.tx.clone());
            }
            O::Load(list) => {
                if let Some(p) = &self.model.palette {
                    tools::load_with(list, p.generation, self.tool_ctx(), self.tx.clone());
                }
                self.palette_redraw();
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
            A::Edit(_) | A::Type { .. } => {}
            A::Confirm(..) | A::Ask { .. } | A::Reply(_) | A::Open(_) | A::Back | A::Load(_) => {}
        }
    }

    fn key(&mut self, e: KeyEvent) {
        let mods = self.mods;
        let Some(p) = &mut self.model.palette else { return self.reply_key(e) };
        let outcome = p.key(e.keysym, e.utf8.as_deref(), mods, &self.cfg.keys);
        self.palette_outcome(outcome);
    }

    /// Typing into a notification's inline reply. tick() lets the keyboard go once it closes.
    fn reply_key(&mut self, e: KeyEvent) {
        match self.model.notes.reply_key(e.keysym, e.utf8.as_deref(), self.mods, &self.cfg.keys) {
            notify::Typed::Nothing => {}
            notify::Typed::Redraw => self.all(false, true),
            notify::Typed::Send(id, text) => {
                if let Some(bus) = &self.bus {
                    bus.note_replied(id, text);
                }
                self.act(Action::NoteDismiss(id), 0.0, None);
            }
        }
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
        if let Some(s) = self.sidebar.as_mut().filter(|_| panel) {
            s.invalidate();
        }
        if compact {
            self.edges_changed();
        }
    }

    fn edges_changed(&mut self) {
        self.edges.iter_mut().for_each(Edges::invalidate);
    }

    fn save(&mut self) {
        self.state.pos = self.islands.iter()
            .filter_map(|i| i.pos.map(|(x, y)| (i.name.clone(), (x * i.scale.ui, y * i.scale.ui))))
            .collect();
        self.state.save();
    }

    /// Sharp on the densest output; the others scale it down.
    fn raster_scale(&self) -> f64 {
        self.islands.iter().map(|i| i.scale.px()).reduce(f64::max).unwrap_or(1.0)
    }

    fn set_art(&mut self) {
        let path = self.model.media.as_ref().and_then(|t| t.art.clone());
        let scale = self.raster_scale();
        let load = |p: &PathBuf, px: i32| icons::load_art(p, (f64::from(px) * scale).round() as i32);
        self.model.art_small = path.as_ref().and_then(|p| load(p, 20)).map(|s| rounded(s, 5.0 * scale));
        // Sized for the sidebar; the panel scales it down.
        self.model.art_large = path.as_ref().and_then(|p| load(p, 104));
    }

    fn set_tray(&mut self) {
        let px = (18.0 * self.raster_scale()).round() as i32;
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
                    // A burst from one app shares a toast; the ones it covers have expired.
                    if let Some(gone) = self.model.notes.add(note) {
                        self.note_gone(gone, 1);
                    }
                    self.all(false, true);
                } else if note.is_ours() {
                    // Someone else is the daemon, so our own messages go through it. Only
                    // ours: relaying one from the bus could bring it straight back.
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
                let dot = self.model.audio.as_ref().is_none_or(|o| o.recording.is_empty() != a.recording.is_empty());
                self.model.audio = Some(a);
                if changed {
                    self.model.flash_power = false;
                    for i in &mut self.islands {
                        i.flash();
                    }
                    self.edges_changed();
                }
                self.all(dot, true);
            }
            Msg::Battery(b) => {
                let replug = self.model.battery.as_ref().is_some_and(|o| o.plugged != b.plugged);
                // The lowest threshold crossed, so starting up at 8% warns once, about 8%.
                let warn = [10u8, 20].into_iter().find(|&at| !b.plugged && b.pct <= at as f64 && self.model.battery_warned > at);
                let title = format!("Battery at {:.0}%", b.pct);
                if b.plugged {
                    self.model.battery_warned = 100;
                }
                self.model.battery = Some(b);
                if replug {
                    self.model.flash_power = true;
                    self.islands.iter_mut().for_each(Island::flash);
                }
                if let Some(at) = warn {
                    self.model.battery_warned = at;
                    let mut note = Note::local("battery", &title, "Plug in soon", notify::NoteImage::Named("battery_alert".into()));
                    note.urgency = if at <= 10 { 2 } else { 1 };
                    self.handle(Msg::Notify(note));
                }
                self.all(true, true);
            }
            Msg::Claude(e, reply) => self.claude(e, reply),
            Msg::Ddc(screens) => {
                self.model.ddc = screens;
                self.all(false, true);
            }
            Msg::Camera(who) => {
                self.model.camera = who;
                self.all(true, false);
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
                self.edges_changed();
            }
            Msg::Radio(on) => {
                self.model.radio = on;
                self.all(false, true);
            }
            Msg::Levels(levels) => {
                // A last frame can arrive from a capture that's just been stopped.
                if self.capture.is_some() {
                    self.set_levels(levels);
                }
            }
            Msg::Calendar(range, events) => {
                if self.calendar_range == Some(range) && self.model.calendar.events != events {
                    self.model.calendar.events = events;
                    self.all(false, true);
                }
            }
            Msg::Bluetooth(b) => {
                self.model.bluetooth = b;
                self.all(false, true);
                self.edges_changed();
            }
            Msg::Tray(items) => {
                self.tray_items = items;
                self.set_tray();
                self.all(false, true);
                self.edges_changed();
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
            "style" => self.choose_style(self.model.style.next()),
            "sidebar" => self.toggle_sidebar(None),
            "reload" => {
                self.configure();
            }
            "settings" => self.open_palette(Some("settings")),
            "wall" => {
                let wall = modules::wallpaper::load();
                if wall != self.model.wall {
                    self.model.wall = wall;
                    self.all(false, true);
                }
            }
            "quit" => self.quit = true,
            "timer" => self.timer("toggle"),
            "nightlight" => self.night_command(""),
            "claude focus" => {
                let worst = self.model.claude.iter().filter(|s| claude::shown(s, Instant::now()))
                    .max_by_key(|s| (s.state == claude::State::Waiting, s.since)).map(|s| s.pids.clone());
                if let Some(w) = worst.and_then(|p| self.window_of(&p)) {
                    self.wm.focus(w);
                }
            }
            "audio" => {
                self.model.audio_open = !self.model.audio_open;
                audio::DETAILED.store(self.model.audio_open, std::sync::atomic::Ordering::Relaxed);
                audio::poke();
                self.all(false, true);
            }
            "" => {} // a second instance checking whether we're alive
            _ => match c.split_once(' ') {
                Some(("open", tool)) => self.open_palette(Some(tool)),
                Some(("timer", what)) => self.timer(what),
                Some(("style", name)) => match style::Style::parse(name) {
                    Some(s) => self.choose_style(s),
                    None => eprintln!("island: no style called {name}"),
                },
                Some(("nightlight", arg)) => self.night_command(arg),
                Some(("set", rest)) => match rest.split_once(' ') {
                    Some((key, value)) => self.choose(&[(key, value)]),
                    None => eprintln!("island: set what to what?"),
                },
                Some(("preset", name)) => match prefs::preset(name) {
                    Some(all) => self.choose(&all),
                    None => eprintln!("island: presets are light and full"),
                },
                Some(("shot", kind)) => {
                    if let Some(task) = tools::shot_task(kind) {
                        self.run(tools::Act::Task(task));
                    }
                }
                _ => eprintln!("island: unknown command {c}"),
            },
        }
    }

    fn choose_style(&mut self, style: style::Style) {
        self.set_style(style);
        self.state.style = Some((style, self.cfg.style));
        self.save();
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
            Action::Brightness => {
                let pct = (frac * 100.0).round() as u32;
                // A drag fires per pixel; only act when the percentage moves.
                if self.model.brightness != Some(pct) {
                    if let Some(cmd) = brightness::set_cmd(&self.cfg.commands, pct) {
                        spawn_sh(&cmd);
                        self.model.brightness = Some(pct);
                        self.all(false, true);
                    }
                }
            }
            Action::Night => self.night_set(!self.model.night.on),
            Action::Warmth => self.night_warmth(frac),
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
            Action::NoteAction(id, key) if self.claude_note(id, &key) => self.act(Action::NoteDismiss(id), 0.0, None),
            Action::NoteAction(id, key) => {
                if let Some(bus) = self.bus.as_ref().filter(|_| id < 0xfff0_0000) {
                    bus.note_action(id, key);
                }
                self.act(Action::NoteDismiss(id), 0.0, None);
            }
            Action::NoteDismiss(id) => {
                // A card closed unanswered: the terminal's prompt decides.
                self.claude_asks.retain(|a| a.0 != id);
                if self.model.notes.remove(id) {
                    self.end_toasts();
                }
                self.note_gone(id, 2);
                self.all(false, true);
            }
            Action::NoteClear => {
                self.model.notes.clear_history();
                self.all(false, true);
            }
            Action::Cmd(c) => self.command(&c),
            Action::Ddc(bus) => {
                let pct = (frac * 100.0).round() as u32;
                if let Some(s) = self.model.ddc.iter_mut().find(|s| s.bus == bus && s.pct != pct) {
                    s.pct = pct;
                    ddc::set(bus, pct);
                    self.all(false, true);
                }
            }
            Action::StreamVolume(index) => {
                let pct = (frac * 100.0).round() as u32;
                let Some(s) = self.model.audio.as_mut().and_then(|a| a.apps.iter_mut().find(|s| s.index == index)) else { return };
                // A drag fires per pixel; only act when the percentage moves.
                if s.volume != pct {
                    s.volume = pct;
                    spawn_sh(&format!("pactl set-sink-input-volume {index} {pct}%"));
                    self.all(false, true);
                }
            }
            Action::StreamMute(index) => spawn_sh(&format!("pactl set-sink-input-mute {index} toggle")),
            Action::AudioDevice { source, name } => {
                let what = if source { "source" } else { "sink" };
                spawn_sh(&format!("pactl set-default-{what} {}", tools::quote(&name)));
            }
            Action::NoteReply(id) => {
                self.model.notes.start_reply(id);
                let on = self.model.notes.replying.is_some();
                for (i, isl) in self.islands.iter_mut().enumerate() {
                    isl.set_typing(on && from == Some(i));
                }
                self.all(false, true);
            }
            Action::NoteGroup(app) => {
                self.model.notes.toggle(&app);
                self.all(false, true);
            }
            Action::NoteClearApp(app) => {
                for id in self.model.notes.ids_from(&app) {
                    self.act(Action::NoteDismiss(id), 0.0, None);
                }
            }
            Action::Dnd => {
                self.model.notes.dnd = !self.model.notes.dnd;
                self.state.dnd = self.model.notes.dnd;
                self.save();
                self.all(false, true);
            }
            Action::Sidebar => self.toggle_sidebar(from),
            Action::CalMonth(by) => {
                let cal = &mut self.model.calendar;
                cal.offset = if by == 0 { 0 } else { (cal.offset + by).clamp(-1200, 1200) };
                cal.selected = None;
                self.load_calendar();
                self.all(false, true);
            }
            Action::CalDay(day) => {
                let cal = &mut self.model.calendar;
                cal.selected = (cal.selected != Some(day)).then_some(day);
                self.all(false, true);
            }
            Action::Bluetooth => {
                if let Some(b) = &self.model.bluetooth {
                    bluetooth::set_power(b.adapter.clone(), !b.powered);
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

    /// The window a process tree runs in: its terminal. Several windows of one
    /// terminal process share a pid, so a title naming Claude breaks the tie.
    fn window_of(&self, pids: &[u32]) -> Option<u64> {
        let mut hits: Vec<&wm::Client> = self.model.clients.iter().filter(|c| c.pid != 0 && pids.contains(&c.pid)).collect();
        hits.sort_by_key(|c| !c.title.as_deref().unwrap_or("").contains("Claude"));
        hits.first().map(|c| c.id)
    }

    fn looking_at(&self, pids: &[u32]) -> bool {
        self.window_of(pids).is_some_and(|w| self.model.clients.iter().any(|c| c.id == w && c.is_focused))
    }

    fn claude(&mut self, e: claude::Event, reply: Option<mpsc::Sender<String>>) {
        let now = Instant::now();
        // Anything else from a session means a card for it was answered in the terminal.
        if e.kind != "PermissionRequest" {
            let gone: Vec<u32> = self.claude_asks.iter().filter(|a| a.1.session == e.session).map(|a| a.0).collect();
            for id in gone {
                self.act(Action::NoteDismiss(id), 0.0, None);
            }
        }
        match e.state() {
            None => self.model.claude.retain(|s| s.id != e.session),
            Some(state) => match self.model.claude.iter_mut().find(|s| s.id == e.session) {
                Some(s) => {
                    if s.state != state {
                        s.state = state;
                        s.since = now;
                    }
                    s.pids = e.pids.clone();
                }
                None => self.model.claude.push(claude::Session { id: e.session.clone(), project: e.project.clone(), state, since: now, pids: e.pids.clone() }),
            },
        }
        let away = !self.looking_at(&e.pids);
        let cfg = self.cfg.claude.clone();
        match e.kind.as_str() {
            "PermissionRequest" if cfg.approve && away => {
                let (title, body) = e.ask_text();
                let mut note = Note::local("Claude Code", &title, &body, notify::NoteImage::None);
                note.sticky = true;
                note.urgency = 2;
                note.actions = vec![("allow".into(), "Allow".into())];
                if e.can_remember() {
                    note.actions.push(("always".into(), "Always allow".into()));
                }
                note.actions.push(("deny".into(), "Deny".into()));
                let id = self.model.notes.next_id();
                note.id = id;
                if let Some(reply) = reply {
                    self.claude_asks.push((id, e, reply));
                }
                self.handle(Msg::Notify(note));
            }
            "Stop" if cfg.notify && away => {
                let mut note = Note::local("Claude Code", &format!("Claude finished · {}", e.project), &e.last_message(), notify::NoteImage::None);
                note.actions = vec![("default".into(), "Show".into())];
                note.id = self.model.notes.next_id();
                self.claude_notes.push((note.id, e.pids.clone()));
                self.handle(Msg::Notify(note));
            }
            "Notification" if cfg.notify && away && e.state() == Some(claude::State::Waiting)
                && !self.claude_asks.iter().any(|a| a.1.session == e.session) => {
                let msg = e.raw.get("message").and_then(serde_json::Value::as_str).unwrap_or("Claude needs your input").to_owned();
                let mut note = Note::local("Claude Code", &format!("Claude needs you · {}", e.project), &msg, notify::NoteImage::None);
                note.actions = vec![("default".into(), "Show".into())];
                note.id = self.model.notes.next_id();
                self.claude_notes.push((note.id, e.pids.clone()));
                self.handle(Msg::Notify(note));
            }
            _ => {}
        }
        self.claude_until = self.model.claude.iter().filter(|s| s.state == claude::State::Done)
            .map(|s| s.since + claude::DONE_FOR).filter(|t| *t > now).min();
        self.all(true, true);
    }

    /// A button on a Claude card or note. True when it was one of ours.
    fn claude_note(&mut self, id: u32, key: &str) -> bool {
        if let Some(i) = self.claude_asks.iter().position(|a| a.0 == id) {
            let (_, e, reply) = self.claude_asks.remove(i);
            if let Some(answer) = e.decision(key) {
                let _ = reply.send(answer);
                // Answered: back to work (or, denied, Claude says so and carries on).
                if let Some(s) = self.model.claude.iter_mut().find(|s| s.id == e.session) {
                    s.state = claude::State::Working;
                    s.since = Instant::now();
                }
                self.all(true, true);
            }
            return true;
        }
        if let Some(i) = self.claude_notes.iter().position(|n| n.0 == id) {
            let (_, pids) = self.claude_notes.remove(i);
            if let Some(w) = self.window_of(&pids) {
                self.wm.focus(w);
            }
            return true;
        }
        false
    }

    /// `island timer [start|toggle|stop|skip|<minutes>]`.
    fn timer(&mut self, what: &str) {
        let now = Instant::now();
        let cfg = &self.cfg.pomodoro;
        match (what, &mut self.model.timer) {
            ("start", _) | ("toggle", None) => self.model.timer = Some(timer::Timer::focus(cfg, now)),
            ("toggle", Some(t)) => t.toggle_pause(now),
            ("stop", _) => self.model.timer = None,
            ("skip", Some(t)) => self.model.timer = t.next(cfg, now),
            (m, _) => match m.trim_end_matches('m').parse::<f64>() {
                Ok(m) if m > 0.0 => {
                    let total = Duration::from_secs_f64(m * 60.0);
                    self.model.timer = Some(timer::Timer::new(timer::Phase::Countdown, 1, total, now));
                }
                _ => eprintln!("island: timer takes start, toggle, stop, skip or minutes"),
            },
        }
        self.all(true, true);
        self.timer_reschedule(now);
    }

    fn timer_reschedule(&mut self, now: Instant) {
        self.timer_fine = self.islands.iter().any(Island::wants_stats);
        self.timer_at = self.model.timer.as_ref().and_then(|t| t.next_change(now, self.timer_fine));
    }

    /// A phase ran out: say so, and start the next one if that's the setting.
    fn timer_tick(&mut self, now: Instant) {
        let Some(t) = &self.model.timer else { return };
        if t.done(now) {
            let next = t.next(&self.cfg.pomodoro, now);
            let (title, body) = match (t.phase, &next) {
                (timer::Phase::Focus, Some(n)) => ("Focus done", format!("{} minute {}", n.total.as_secs() / 60, n.phase.label().to_lowercase())),
                (_, Some(_)) => ("Break's over", "Back to focus".to_owned()),
                (_, None) => ("Time's up", match t.total.as_secs() {
                    s if s < 60 => format!("{s} second timer"),
                    s => format!("{} minute timer", (s as f64 / 60.0).round()),
                }),
            };
            let note = notify::Note::local("timer", title, &body, notify::NoteImage::Named("timer".into()));
            self.model.timer = next.filter(|_| self.cfg.pomodoro.auto);
            self.handle(Msg::Notify(note));
        }
        self.all(true, true);
        self.timer_reschedule(now);
    }

    fn next_deadline(&self) -> Instant {
        let islands = self.islands.iter().filter_map(Island::next_deadline);
        let sidebar = self.sidebar.as_ref().and_then(Sidebar::next_deadline);
        let hovering = self.sidebar_at.as_ref().map(|s| s.0);
        islands.chain(sidebar).chain(hovering).chain(self.capture_retry).chain(self.progress_at).chain(self.next_stats).chain(self.timer_at).chain(self.claude_until).chain(self.model.notes.until).chain([self.next_clock]).min().unwrap_or(self.next_clock)
    }

    fn tick(&mut self) {
        let now = Instant::now();
        for i in &mut self.islands {
            i.tick(now);
        }
        if self.claude_until.is_some_and(|t| now >= t) {
            // A finished session drops off the pill a little while after.
            self.claude_until = None;
            self.all(true, true);
        }
        if self.timer_at.is_some_and(|t| now >= t) {
            self.timer_tick(now);
        } else if self.model.timer.is_some() && self.islands.iter().any(Island::wants_stats) != self.timer_fine {
            // The panel opened or closed: seconds or minutes from here.
            self.timer_reschedule(now);
        }
        if self.islands.iter_mut().fold(false, |due, i| i.take_pin(now) | due) {
            self.act(Action::Pin, 0.0, None);
        }
        if now >= self.next_clock {
            self.next_clock = next_minute();
            self.all(true, true);
            self.night_minute();
        }
        self.pump_notes(now);
        self.sidebar_tick(now);
        // However the reply field closed (sent, Esc, the note went), the keyboard goes back.
        if self.model.notes.replying.is_none() {
            self.islands.iter_mut().for_each(|i| i.set_typing(false));
            if let Some(s) = &mut self.sidebar {
                s.set_typing(false);
            }
        }
        self.sync_capture();
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

    fn sidebar_tick(&mut self, now: Instant) {
        if self.sidebar_at.as_ref().is_some_and(|(t, _)| *t <= now) {
            if let Some((_, name)) = self.sidebar_at.take() {
                let island = self.islands.iter().position(|i| i.name == name);
                let open = self.sidebar.as_ref().is_some_and(Sidebar::is_open);
                if let (Some(i), false) = (island, open) {
                    self.open_sidebar(i, true);
                }
            }
        }
        let Some(s) = &mut self.sidebar else { return };
        s.tick(now);
        if s.finished() {
            return self.drop_sidebar();
        }
        if !s.is_open() && self.model.sidebar_open {
            // It closed itself, on leaving.
            self.model.sidebar_open = false;
            self.all(false, true);
            self.edges_changed();
        }
        let moving = self.model.sidebar_open && self.model.media.as_ref().is_some_and(|t| t.playing && t.length > 0.0);
        match self.progress_at {
            _ if !moving => self.progress_at = None,
            Some(t) if t > now => {}
            due => {
                self.progress_at = Some(now + Duration::from_secs(1));
                if let (Some(_), Some(s)) = (due, &mut self.sidebar) {
                    s.invalidate();
                }
            }
        }
    }

    fn stats(&mut self, now: Instant) {
        let first = self.next_stats.is_none();
        if first && self.cfg.panel.iter().any(|n| n == "calendar") {
            self.load_calendar();
        }
        self.next_stats = Some(now + STATS_EVERY);
        let _ = self.stats.send(sys::Cmd::Sample);
        let _ = self.net_poke.send(());
        // Something else may have changed it; a file read is cheap, a custom command only runs on open.
        if first || brightness::is_cheap(&self.cfg.commands) {
            let now = brightness::read(&self.cfg.commands);
            if now != self.model.brightness {
                self.model.brightness = now;
                self.all(false, true);
            }
        }
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
        self.draw();
        let has_panel = self.islands.iter().any(Island::holds_panel);
        if self.had_panel && !has_panel {
            let _ = self.stats.send(sys::Cmd::Release);
        }
        self.had_panel = has_panel;
    }

    fn draw(&mut self) {
        let App { islands, edges, sidebar, model, icons, cfg, compositor, qh, .. } = self;
        for e in edges {
            e.flush(model, icons, cfg, qh);
        }
        if let Some(s) = sidebar {
            s.flush(model, icons, cfg, compositor, qh);
        }
        for i in islands {
            i.flush(model, icons, cfg, compositor, qh);
        }
    }

    fn island_at(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.islands.iter().position(|i| i.surface() == surface)
    }

    fn edge_at(&self, surface: &wl_surface::WlSurface) -> Option<edges::At> {
        self.edges.iter().enumerate().find_map(|(i, e)| e.find(surface, i))
    }

    fn scroll(&mut self, kind: Scroll, vertical: &smithay_client_toolkit::seat::pointer::AxisScroll) {
        self.scroll += if vertical.value120 != 0 { vertical.value120 as f64 / 120.0 } else { vertical.absolute / 15.0 };
        let notches = self.scroll.trunc();
        self.scroll -= notches;
        if notches == 0.0 {
            return;
        }
        match kind {
            Scroll::Volume => {
                let sign = if notches > 0.0 { '-' } else { '+' };
                spawn_sh(&format!("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ {}%{sign}", 5.0 * notches.abs()));
            }
            Scroll::Tags => self.wm.cycle(notches > 0.0),
            Scroll::Panel => {
                if let Some(i) = self.under_pointer.clone().and_then(|s| self.island_at(&s)) {
                    self.islands[i].scroll_by(notches);
                }
            }
            Scroll::Palette => {
                if let Some(p) = &mut self.model.palette {
                    p.scroll(notches as isize);
                    self.palette_redraw();
                }
            }
        }
    }

    fn sidebar_pointer(&mut self, e: &PointerEvent) {
        let Some(s) = &mut self.sidebar else { return };
        let outcome = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.enter_serial = serial;
                self.cursor = None;
                self.sidebar_at = None;
                s.enter(e.position);
                Outcome::None
            }
            PointerEventKind::Leave { .. } => return s.leave(self.cfg.ui.hover_close_ms),
            PointerEventKind::Motion { .. } => s.motion(e.position),
            PointerEventKind::Press { button, .. } => s.press(button),
            PointerEventKind::Release { .. } => s.release(),
            PointerEventKind::Axis { vertical, .. } => {
                if s.over_volume() {
                    return self.scroll(Scroll::Volume, &vertical);
                }
                let notches = if vertical.value120 != 0 { vertical.value120 as f64 / 120.0 } else { vertical.absolute / 15.0 };
                return s.scroll_by(notches);
            }
        };
        if let Outcome::Act(action, frac) = outcome {
            let island = self.islands.iter().position(|i| i.name == s.name);
            let reply = matches!(action, Action::NoteReply(_));
            // The sidebar has the reply's field, so it takes the keys rather than an island.
            self.act(action, frac, if reply { None } else { island });
            if let (true, Some(s)) = (reply && self.model.notes.replying.is_some(), &mut self.sidebar) {
                s.set_typing(true);
            }
        }
        let over = self.sidebar.as_ref().is_some_and(Sidebar::hovering_target);
        self.set_cursor(if over { Shape::Pointer } else { Shape::Default });
    }

    /// Resting on the middle of the frame's right border.
    fn sidebar_hover(&mut self, at: edges::At, pos: (f64, f64)) {
        let e = &self.edges[at.edges];
        let wanted = self.cfg.ui.hover_sidebar && !self.cfg.sidebar.is_empty() && e.at_sidebar(at, pos)
            && !self.sidebar.as_ref().is_some_and(Sidebar::is_open);
        if !wanted {
            self.sidebar_at = None;
        } else if self.sidebar_at.is_none() {
            self.sidebar_at = Some((Instant::now() + Duration::from_millis(self.cfg.ui.hover_open_ms), e.name.clone()));
        }
    }

    fn edge_pointer(&mut self, at: edges::At, e: &PointerEvent) {
        match e.kind {
            PointerEventKind::Enter { serial } => {
                self.enter_serial = serial;
                self.cursor = None;
                self.edges[at.edges].motion(at, e.position);
                self.sidebar_hover(at, e.position);
            }
            PointerEventKind::Leave { .. } => {
                self.sidebar_at = None;
                if let Some(s) = &mut self.sidebar {
                    s.edge_left(self.cfg.ui.hover_close_ms);
                }
                return self.edges[at.edges].leave(at);
            }
            PointerEventKind::Motion { .. } => {
                self.edges[at.edges].motion(at, e.position);
                self.sidebar_hover(at, e.position);
            }
            PointerEventKind::Press { .. } => {
                self.edges[at.edges].motion(at, e.position);
                self.edges[at.edges].press(at);
            }
            PointerEventKind::Release { button, .. } => {
                let action = self.edges[at.edges].release(at, button);
                let island = self.islands.iter().position(|i| i.name == self.edges[at.edges].name);
                let menu = match action {
                    Action::TrayRight(i) => self.model.tray.get(i).is_some_and(|(item, _)| item.menu.is_some()),
                    Action::TrayClick(i) => self.model.tray.get(i).is_some_and(|(item, _)| item.is_menu && item.menu.is_some()),
                    _ => false,
                };
                // A menu needs the island open to land in.
                if let (true, Some(i)) = (menu, island) {
                    self.islands[i].open_briefly();
                }
                self.act(action, 0.0, island);
                // That can have switched the style, taking these edges with it.
                return;
            }
            PointerEventKind::Axis { vertical, .. } => self.scroll(self.edges[at.edges].scroll_kind(at), &vertical),
        }
        let shape = if self.edges[at.edges].hovering_target(at) { Shape::Pointer } else { Shape::Default };
        self.set_cursor(shape);
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
    pen::rounded_rect(&cr, 0.0, 0.0, w, h, r);
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
        self.edges.iter_mut().for_each(|e| e.frame_done(surface));
        if let Some(s) = self.sidebar.as_mut().filter(|s| s.surface() == surface) {
            s.frame_done();
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
        self.night_apply();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.model.night.drop_output(&output);
        self.islands.retain(|i| i.output != output);
        self.edges.retain(|e| e.output != output);
        if self.sidebar.as_ref().is_some_and(|s| s.output == output) {
            self.drop_sidebar();
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.islands.retain(|i| &i.layer != layer);
        if self.sidebar.as_ref().is_some_and(|s| &s.layer == layer) {
            self.drop_sidebar();
        }
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(i) = self.islands.iter_mut().find(|i| &i.layer == layer) {
            i.configured();
        }
        self.edges.iter_mut().for_each(|e| e.configured(layer));
        if let Some(s) = self.sidebar.as_mut().filter(|s| &s.layer == layer) {
            s.configured();
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
        for raw in events {
            let mut e = raw.clone();
            e.position = (e.position.0 / self.cfg.ui.scale, e.position.1 / self.cfg.ui.scale);
            let e = &e;
            let Some(idx) = self.island_at(&e.surface) else {
                if self.sidebar.as_ref().is_some_and(|s| s.surface() == &e.surface) {
                    self.sidebar_pointer(e);
                } else if let Some(at) = self.edge_at(&e.surface) {
                    self.edge_pointer(at, e);
                }
                continue;
            };
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    self.enter_serial = serial;
                    self.cursor = None;
                    self.under_pointer = Some(e.surface.clone());
                    self.islands[idx].enter(e.position, &self.cfg.ui);
                }
                PointerEventKind::Leave { .. } => {
                    self.under_pointer = None;
                    self.islands[idx].leave(&self.cfg.ui);
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
                    self.scroll(self.islands[idx].scroll_kind(), &vertical);
                }
            }
            // An action can take the island away: a new style, or the output going.
            let Some(isl) = self.islands.get(idx) else { continue };
            let shape = if isl.dragging() { Shape::Grabbing } else if isl.hovering_target() { Shape::Pointer } else { Shape::Default };
            self.set_cursor(shape);
        }
    }
}

impl RelativePointerHandler for App {
    fn relative_pointer_motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ZwpRelativePointerV1,
                               _: &wl_pointer::WlPointer, e: RelativeMotionEvent) {
        for i in &mut self.islands {
            i.relative(i.scale.units(e.delta));
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

pub fn socket_path() -> PathBuf {
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
                if let Some(json) = line.strip_prefix("claude\t") {
                    let Some(event) = claude::Event::parse(json) else { return };
                    if event.kind != "PermissionRequest" {
                        let _ = tx.send(Msg::Claude(event, None));
                        return;
                    }
                    // The hook waits on this connection for the card's answer.
                    let (reply, answer) = mpsc::channel();
                    let _ = tx.send(Msg::Claude(event, Some(reply)));
                    if let Ok(text) = answer.recv() {
                        let _ = writeln!(out, "{text}");
                    }
                    return;
                }
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
            "claude-hook" => claude::hook(),
            "claude-setup" => {
                let bin = env::current_exe().map_or("island".into(), |p| p.to_string_lossy().into_owned());
                match claude::setup(&bin, args.get(1).is_some_and(|a| a == "--undo")) {
                    Ok(m) => { println!("{m}"); std::process::exit(0) }
                    Err(e) => { eprintln!("island: {e}"); std::process::exit(1) }
                }
            }
            "doctor" => doctor::run(),
            "set" if !args.get(1).zip(args.get(2)).is_some_and(|(k, v)| args.len() == 3 && prefs::put(&mut Config::default(), k, v)) => {
                let parts: Vec<&str> = prefs::LOOKS.iter().map(|l| l.0).chain(prefs::PARTS.iter().map(|p| p.0)).collect();
                eprintln!("island: set <key> <value>, the keys being {}", parts.join(", "));
                std::process::exit(2);
            }
            "preset" if args.get(1).is_none_or(|n| prefs::preset(n).is_none()) => {
                eprintln!("island: presets are light and full");
                std::process::exit(2);
            }
            "style" if args.get(1).is_some_and(|a| style::Style::parse(a).is_none()) => {
                eprintln!("island: styles are {}", style::Style::ALL.map(style::Style::name).join(", "));
                std::process::exit(2);
            }
            "pin" | "toggle" | "tuck" | "reset" | "reload" | "wall" | "quit" | "dnd" | "open" | "close" | "shot" | "timer" | "audio" | "nightlight" | "style" | "sidebar" | "settings" | "set" | "preset" => run_client(&args.join(" ")),
            _ => {}
        }
        eprintln!("usage: island [pin|tuck|reset|reload|style [name]|settings|set <key> <value>|preset <light,full>|sidebar|wall|quit|dnd|close|open [tool]|shot <kind>|timer [start|toggle|stop|skip|<minutes>]|nightlight [on|off|toggle]|audio|doctor|claude-setup [--undo]|dmenu [-p prompt]]");
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

    let state = State::load();
    let cfg = Config::load(&state);
    let net_poke = net::spawn(tx.clone());

    let wm = wm::detect();
    wm.watch(tx.clone());
    modules::custom::spawn(&cfg.custom, tx.clone());
    let cfg_notifications = cfg.notifications;
    watch_theme(&cfg.palette, tx.clone());
    let night = nightlight::Night::new(&globals, &qh, &cfg.nightlight, state.night, state.warmth);
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
        edges: Vec::new(),
        sidebar: None,
        sidebar_at: None,
        progress_at: None,
        capture: None,
        capture_retry: None,
        had_panel: false,
        calendar_range: None,
        under_pointer: None,
        model: Model { monitors: Vec::new(), clients: Vec::new(), custom: HashMap::new(), sys: sys::Snapshot::default(),
                       audio: None, brightness: brightness::read(&cfg.commands), media: None, art_small: None, art_large: None, net: net::State::Unknown, bluetooth: None,
                       radio: None, levels: [0.0; viz::BANDS], calendar: Default::default(), sidebar_open: false,
                       can_capture: !cfg.visualizer.is_empty() && tools::on_path("parec"),
                       tray: Vec::new(), pal: theme::load(&cfg.palette), style: state.style(cfg.style), palette: None,
                       notes: notify::Center::new(state.dnd),
                       wall: modules::wallpaper::load(), timer: None,
                       camera: Vec::new(), audio_open: false, battery: None, flash_power: false, battery_warned: 100,
                       claude: Vec::new(), ddc: Vec::new(), night },
        icons: Icons::new(),
        tray_items: Vec::new(),
        cfg,
        state,
        stats: sys::spawn(tx.clone()),
        bus: bus::spawn(tx.clone(), cfg_notifications),
        wm,
        net_poke,
        feeds: Feeds::default(),
        tx: tx.clone(),
        next_stats: None,
        next_clock: next_minute(),
        timer_at: None,
        timer_fine: false,
        claude_asks: Vec::new(),
        claude_notes: Vec::new(),
        claude_until: None,
        scroll: 0.0,
        quit: false,
    };



    app.start_feeds();

    while !app.quit {
        let wait = app.next_deadline().saturating_duration_since(Instant::now());
        if let Err(e) = event_loop.dispatch(Some(wait), &mut app) {
            eprintln!("island: event loop: {e}");
            break;
        }
        app.tick();
        app.flush();
    }
    app.capture = None; // ends parec
    let _ = std::fs::remove_file(socket_path());
    let _ = conn.flush();
}
