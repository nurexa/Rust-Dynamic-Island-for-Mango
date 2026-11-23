# Modding

The island is a set of small parts, and every one is optional. There are six ways to
change it, from a config line to a new compositor. Start at the first that does what
you want. After a config change, run `island reload`; nothing needs a restart.

## 1. Turn things on and off

Copy [`examples/config.toml`](examples/config.toml) to `~/.config/island/config.toml`.
Leave a name out of a list and that part is gone. The panel lays its tiles out as a grid:
`clock`, `calendar`, `stats`, `weather`, `about` and `timer` are narrow and share a row with their
neighbours in the order given; the rest take a row each. `weather` needs a `[weather] location`.

```toml
pill = ["media", "workspaces", "claude", "timer", "network", "privacy", "battery", "clock"]
panel = ["header", "clock", "calendar", "stats", "weather", "about", "timer", "media", "notifications", "audio", "brightness", "nightlight", "status", "bluetooth"]
tools = ["apps", "run", "windows", "files", "clipboard", "screenshot", "wifi", "bluetooth", "wallpaper", "power"]
sidebar = ["media", "calendar", "controls", "notifications"]   # [] leaves it out
visualizer = ["bars", "wave", "ring"]                           # [] is off
notifications = true          # false keeps your own notification daemon
disable = ["timer", "web"]    # or leave parts out wherever they appear (also "calc", "web", "sidebar", "visualizer")
```

| Pill modules | Shows |
|---|---|
| `media` | Album art while something plays, with the `bars` visualizer |
| `workspaces` | Occupied workspaces and their windows |
| `claude` | Claude Code sessions: working, needs you, done |
| `timer` | The pomodoro's minutes left, while it runs |
| `network` | A warning, only when offline |
| `privacy` | Orange dot while a mic records, green while the camera's open |
| `battery` | Charge, on laptops |
| `clock` | The time |

| Panel modules | Shows |
|---|---|
| `header` | Time, date, and the sidebar, home, tuck and pin buttons |
| `workspaces` | Every monitor and its workspaces |
| `media` | Art, track, progress, playback buttons, `wave` and `ring` |
| `calendar` | The month, and events; off by default, the sidebar has it |
| `timer` | Pomodoro: start, pause, skip, stop |
| `notifications` | Recent notifications, do not disturb |
| `stats` | CPU, GPU, RAM, network tiles |
| `audio` | Volume slider; click the device for outputs, mics and per-app volume |
| `brightness` | Brightness sliders: the backlight, and each monitor with ddcutil |
| `nightlight` | Night light toggle and warmth slider; hidden when the compositor has no gamma control |
| `status` | Network, battery, tray |
| `bluetooth` | On/off switch, connected devices and their battery; hidden without an adapter |
| `wallpaper` | Wallpaper fit, speed, fps, quality (needs `wallpaper_opt`) |

| Sidebar sections | Shows |
|---|---|
| `media` | The player, with `wave` and `ring` |
| `calendar` | The month, event dots, and what's next |
| `controls` | Wi-Fi, Bluetooth, night light and do not disturb, with volume and brightness |
| `notifications` | The whole history; the sidebar scrolls |

A `custom:` module works in the sidebar too.

| Tools | Does | Needs |
|---|---|---|
| `apps` | Searches and launches apps, favourites first | |
| `run` | Runs anything on your PATH, or what you typed | |
| `windows` | Switches to any window on any workspace | |
| `files` | Searches files as you type, opens the one you pick | `plocate` (with `updatedb`) or `fd` |
| `clipboard` | Searches history and restores an entry | `cliphist`, `wl-clipboard` |
| `screenshot` | Region, window, screen, annotate, color picker | `grim`, `slurp`; `satty` to annotate |
| `wifi` | Lists networks and connects, asking for a password | `nmcli` |
| `bluetooth` | Connects paired devices, scans for and pairs new ones, turns it on and off | BlueZ and an adapter |
| `wallpaper` | Picks from a folder, with thumbnails; **Settings ›** nests the engine's options when `wallpaper_opt` is set | `[wallpaper]` in config |
| `power` | Lock, suspend, log out, reboot, shut down | |

A tool whose programs are missing simply doesn't appear. To drop parts from the binary
altogether, build only what you want:

```sh
cargo build --release --no-default-features --features "apps,clipboard,notifications"
```

The features are `apps`, `run`, `windows`, `files`, `calc`, `clipboard`, `screenshot`,
`wifi`, `bluetooth`, `wallpaper`, `power` and `notifications`.

Commands to bind to keys:

```sh
island open                # the palette, on the first tool
island open clipboard      # on a given tool; the same key again closes it
island shot region         # or output, window, annotate, color
island dnd                 # do not disturb
island nightlight          # toggle the night light; or on, off
island timer               # start or pause the pomodoro; stop, skip, or minutes: island timer 10
island audio               # open the panel's list of outputs, mics and apps
island doctor              # what works on this machine, and how to fix what doesn't
island claude-setup        # add Claude Code's hooks (--undo takes them out)
island style               # the next style; or pill, frame, notch, bar, glass, split, minimal
island settings            # change any of this from the island: it edits config.toml, comments kept
island set scale 1.25      # or blur, opacity, icons, hover, or any add-on: on, off
island preset light        # the basics; or full
island sidebar             # open or close the sidebar
```

## 2. Change how it looks and behaves

```toml
style = "pill"                # pill, frame, notch, bar, glass, split or minimal
icon_style = "glyph"          # or "color" for the apps' own icons
icon_font = "JetBrainsMono Nerd Font Propo"
terminal = "kitty"            # for apps that need one
screenshot_dir = "~/Pictures"
web_search = "https://duckduckgo.com/?q={}"   # the search's web row; empty hides it

[icons]                       # app id = any glyph, or an icon's name from src/symbols.rs
"org.gnome.nautilus" = "folder"

[calendar]
dir = "~/.local/share/calendars"   # .ics files, searched four folders deep; empty is a plain month
week_start = "monday"              # sunday (the default), monday or saturday

[notify]                      # seconds on screen
low = 3
normal = 5

[nightlight]                  # warms and dims the screen itself; no wlsunset needed
schedule = "20:00-07:00"      # empty: manual only. A manual toggle holds until the next edge
warmth = 3400                 # Kelvin, 2500-6500; the panel slider's choice is remembered
dim = 0.85                    # brightness while on

[pomodoro]                    # minutes
focus = 25
short = 5
long = 15
rounds = 4                    # focus rounds before the long break
auto = true                   # start the next phase on its own

[claude]
approve = true                # permission prompts become Allow/Deny cards when you're elsewhere
notify = true                 # a note when a session finishes or needs you

[commands]
lock = "hyprlock"
mixer = "pavucontrol"

[wallpaper]
dir = "~/Pictures/walls"
set = "swww img {path}"       # {path} is the chosen file
```

Colors come from `palette`, a small file of tokens that you can write by hand or
generate from your wallpaper. [`examples/matugen-colors.toml.tmpl`](examples/matugen-colors.toml.tmpl)
is a template for matugen. The island repaints as soon as the file changes.

```toml
accent = "#8fcff3"
on_accent = "#003549"
surface = "#0f1417"
on_surface = "#dfe3e7"
muted = "#8a9297"
error = "#ffb4ab"
```

## 3. Add a tool or a module with a script

No Rust needed. A palette tool is two commands: `list` prints one item per line, and
`run` receives the chosen line wherever it says `{}`. The line is shell-quoted for you.

```toml
tools = ["apps", "run", "projects"]

[[tool]]
name = "projects"
glyph = "󰉋"
hint = "Open a project"
list = "ls ~/Projects"
run = "kitty -d ~/Projects/{}"
```

A pill or panel module is one command, and its first line of output is what shows:

```toml
pill = ["workspaces", "custom:weather", "clock"]

[[custom]]
name = "weather"
exec = "curl -s 'wttr.in/?format=%t'"
interval = 600                 # seconds; 0 runs it once
on_click = "xdg-open https://wttr.in"
```

A custom module disappears while its output is empty. Each one is a sleeping thread
between runs, so a slow interval costs almost nothing.

## 4. Use it from your own scripts

`island dmenu` works like `rofi -dmenu`, so scripts written for rofi only need the
command swapped. It reads lines on stdin, prints the choice, and exits 1 on cancel.

```sh
choice=$(printf 'Small\nLarge\n' | island dmenu -p "Size")
password=$(island dmenu -password -p "Password")
```

It understands `-p`, `-password`, `-format i` and rofi's `text\0icon\x1f/path` rows, and
ignores rofi's styling flags. Anything that sends freedesktop notifications, such as
`notify-send`, works without changes.

## 5. Add a module or tool in Rust

Write Rust when a script can't do it, such as custom drawing or live state.

**A panel or pill module** is one function that draws at a position and returns the
next position. For the focused window's title on the pill, create
`src/modules/title.rs`:

```rust
//! The focused window's title.
use super::{Ctx, PILL_H};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let focused = c.m.clients.iter().find(|w| w.is_focused);
    let Some(title) = focused.and_then(|w| w.title.as_deref()) else { return x };
    let w = p.text(title, 12.5, Weight::Medium, p.pal.fg, x, PILL_H / 2.0, 200.0);
    p.regions.push(Region::new((x, 4.0, w, 24.0), 8.0, Action::Open("windows")));
    x + w
}
```

A panel module is the same with y: it draws between `PAD` and `p.w - PAD` from the y it's
given and returns where the next starts. Its tile, and its width in the grid, come from
`seat()` in `src/modules/mod.rs`: give it a share under 1.0 there and it sits beside others.

Register it in `src/modules/mod.rs`, then add `"title"` to `pill` in your config:

```rust
mod title;

const PILL: &[(&str, PillModule)] = &[
    // ...
    ("title", title::pill),
];
```

**A tool** is one function that returns items. Create `src/tools/emoji.rs`:

```rust
//! Pick an emoji.
use super::{Act, Ctx, Icon, Item};

pub fn list(_: &Ctx) -> Vec<Item> {
    [("Grin", "😀"), ("Rocket", "🚀")].map(|(name, e)| {
        Item::new(name, Icon::Glyph(e.into()), Act::Sh(format!("wl-copy {e}")))
    }).to_vec()
}
```

Register it in `src/tools/mod.rs`:

```rust
mod emoji;

pub const TOOLS: &[Tool] = &[
    // ...
    Tool { name: "emoji", glyph: "\u{f0785}", hint: "Pick an emoji", list: emoji::list, typed: None },
];
```

The list runs on a worker thread when the tool opens and is dropped when it closes, so
it can take its time and costs nothing the rest of the time.

- **Return `x` or `y` unchanged** from a module when there's nothing to show. It then
  takes no room.
- **Panel modules** work in `y`, and what they return includes their own bottom spacing.
  Each sits on a card; `seat()` in `src/modules/mod.rs` says which, and how much of that
  spacing is outside it. Fill with `pal.card`, `raised` or `well` rather than a fixed
  color, and the layers hold up under any palette.
- **`Ctx`** in a module gives you the `Model` (monitors, windows, audio, media, network,
  tray, stats, notifications), the config and the current output.
- **`Pen`** has `text`, `wrapped`, `glyph`, `fill`, `symbol`, `image` and `button`, plus
  the raw cairo context as `p.cr` for anything else.
- **`Region`** is a click target. Push one and hover highlights, tooltips and clicks all
  work. `Action` in `src/pen.rs` lists what a click can do.
- **`Act`** in `src/tools/mod.rs` is what a chosen item does. Wrap one in `Act::Confirm`
  to ask for a second Enter, or use `Act::Ask` to prompt for text.

Modules run only when their content changes, never per animation frame.

For a new data source, add a variant to `Msg` in `src/main.rs`, send it from a thread
of your own, and store it in `Model`. `src/audio.rs` is a short example. Call
`self.all(true, true)` in the handler so the island redraws.

To tune the animation, run `ISLAND_TIME_SCALE=0.1 island` for slow motion. The spring
constants are at the top of `src/island.rs`.

## 6. Support another compositor

Everything compositor-specific sits behind one trait in `src/wm/mod.rs`:

```rust
pub trait Backend {
    fn watch(&self, tx: Tx);                 // send Msg::Monitors and Msg::Clients on change
    fn view(&self, tag: u32, monitor: &str); // show a workspace
    fn send_to(&self, tag: u32);             // move the focused window there
    fn focus(&self, window: u64);
    fn cycle(&self, forward: bool);          // next or previous occupied workspace
    fn quit(&self);                          // end the session
}
```

Add `src/wm/yourcompositor.rs` implementing it, using `src/wm/mango.rs` (about 100
lines) as the model, then add a check to `detect()`:

```rust
if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
    return Box::new(hyprland::Hyprland);
}
```

The compositor needs `wlr-layer-shell`. Sway, Hyprland, niri, river, Wayfire, COSMIC
and KDE Plasma all have it; GNOME doesn't. Without a backend the island still runs:
the workspaces module stays empty, and the `windows` tool has nothing to list.

## Tests

```sh
cargo test                 # unit tests
tests/headless/all.sh      # full run in a private headless Mango
bench/run.py               # benchmark numbers
```

The headless suite replaces `systemctl` and `nmcli` with shims, so a test can never
reboot the machine or change its network.
