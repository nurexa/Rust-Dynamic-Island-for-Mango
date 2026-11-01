# Modding

There are four levels, from a config edit to a new compositor. Start at the top one
that does what you want. After a config change, run `island reload`; nothing needs a
restart.

## 1. Rearrange it in config

Copy [`examples/config.toml`](examples/config.toml) to `~/.config/island/config.toml`.
The pill and the panel are lists of modules, drawn in the order you give them:

```toml
pill = ["media", "workspaces", "network", "clock"]
panel = ["header", "workspaces", "media", "stats", "audio", "status"]
```

Leave a name out and that module is gone. An unknown name is skipped.

| Pill modules | Shows |
|---|---|
| `media` | Album art while something plays |
| `workspaces` | Occupied workspaces and their windows |
| `network` | A warning, only when offline |
| `clock` | The time |

| Panel modules | Shows |
|---|---|
| `header` | Time, date, and the home, tuck and pin buttons |
| `workspaces` | Every monitor and its workspaces |
| `media` | Art, track, progress, playback buttons |
| `stats` | CPU, GPU, RAM, network tiles |
| `audio` | Output device and volume slider |
| `status` | Network, battery, tray |

Icons, fonts and click commands live in the same file:

```toml
icon_style = "glyph"          # or "color" for the apps' own icons
icon_font = "JetBrainsMono Nerd Font Propo"

[icons]                       # app id = any glyph
"org.gnome.nautilus" = "󰉋"

[commands]
wifi = "nm-connection-editor"
wallpaper_pick = "my-wallpaper-script"   # leave empty to hide the wallpaper button
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

## 2. Add a module with a script

Any shell command can be a module, with no Rust involved. Its first line of output
is what shows:

```toml
pill = ["workspaces", "custom:weather", "clock"]

[[custom]]
name = "weather"
exec = "curl -s 'wttr.in/?format=%t'"
interval = 600                 # seconds; 0 runs it once
on_click = "xdg-open https://wttr.in"
```

A custom module works in either list, and it disappears while its output is empty.
Each one is a sleeping thread between runs, so a slow interval costs almost nothing.

Output can include Nerd Font glyphs, as long as your `font` or a fallback has them.

## 3. Add a module in Rust

Write Rust when a script can't do it, such as custom drawing or live state. A module
is one function: it draws at a position and returns the next position.

Say you want the focused window's title on the pill. Create `src/modules/title.rs`:

```rust
//! The focused window's title.
use super::{Ctx, PILL_H};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let focused = c.m.clients.iter().find(|w| w.is_focused);
    let Some(title) = focused.and_then(|w| w.title.as_deref()) else { return x };
    let w = p.text(title, 12.5, Weight::Medium, p.pal.fg, x, PILL_H / 2.0, 200.0);
    p.regions.push(Region::new((x, 4.0, w, 24.0), 8.0, Action::Sh("rofi -show window".into())));
    x + w
}
```

Register it in `src/modules/mod.rs`:

```rust
mod title;

const PILL: &[(&str, PillModule)] = &[
    // ...
    ("title", title::pill),
];
```

Then add `"title"` to `pill` in your config and rebuild. That's the whole interface.

- **Return `x` or `y` unchanged** when there's nothing to show. The module then takes
  no room.
- **Panel modules** have the same shape but work in `y`, and the value they return
  includes their own bottom spacing.
- **`Ctx`** gives you the `Model` (monitors, windows, audio, media, network, tray,
  stats), the config, and the current output.
- **`Pen`** has `text`, `glyph`, `fill`, `symbol`, `image` and `button`, plus the raw
  cairo context as `p.cr` for anything else.
- **`Region`** is a click target. Push one and hover highlights, tooltips and clicks
  all work. `Action` in `src/pen.rs` lists what a click can do.

Modules run only when their content changes, never per animation frame, so a module
can take its time without hurting the animation.

For a new data source, add a variant to `Msg` in `src/main.rs`, send it from a thread
of your own, and store it in `Model`. `src/audio.rs` is a short example. Call
`self.all(true, true)` in the handler so the island redraws.

To tune the animation, run `ISLAND_TIME_SCALE=0.1 island` for slow motion. The spring
constants are at the top of `src/island.rs`.

## 4. Support another compositor

Everything compositor-specific sits behind one trait in `src/wm/mod.rs`:

```rust
pub trait Backend {
    fn watch(&self, tx: Tx);                 // send Msg::Monitors and Msg::Clients on change
    fn view(&self, tag: u32, monitor: &str); // show a workspace
    fn send_to(&self, tag: u32);             // move the focused window there
    fn focus(&self, window: u64);
    fn cycle(&self, forward: bool);          // next or previous occupied workspace
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
and KDE Plasma all have it; GNOME doesn't. Without a backend the island still runs,
with an empty workspaces module and everything else working.

## Tests

```sh
cargo test                 # unit tests
tests/headless/all.sh      # full run in a private headless Mango
bench/run.py               # Waybar vs island numbers
```
