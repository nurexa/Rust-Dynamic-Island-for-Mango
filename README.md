# island

An Apple-style Dynamic Island for [MangoWC](https://mangowm.github.io), written in
Rust. It replaces Waybar: a small pill at the top of each monitor that shows your
workspaces and the apps on them, and springs open into a panel when you hover it.

## Install

From a terminal inside your Mango session:

```sh
./install.sh          # build, install to ~/.local/bin, switch from Waybar to the island
./install.sh --undo   # put Waybar back
```

The installer changes two files, and keeps the originals in
`~/.local/share/island-backup`:

- `~/.config/mango/scripts/bar`, so your existing autostart line runs the island
- a marked block at the end of `~/.config/mango/config.conf`, with one layer rule
  and three key bindings

It checks the new config with Mango's parser and rolls back on its own if the island
doesn't come up. Waybar stays installed until you remove it with
`sudo pacman -Rns waybar`.

Keep `mangobar` running. Nothing draws from it any more, but it's what pauses the
wallpaper under fullscreen windows.

Builds need `rustup` or `cargo`, plus `cairo` and `pango`. At runtime it uses
`pactl`, `wpctl`, `playerctl` (with `playerctld`), `curl` and `adwaita-icon-theme`.
All of them are already on this system.

## Using it

| Do this | To get this |
|---|---|
| Hover the pill | Open the panel; move away to close it |
| Click the pill background, or `Super+I` | Keep the panel open (pin) |
| Drag the pill | Move it anywhere on that monitor |
| Drop it near the top centre | Dock it again |
| Double-click the background, or `Super+Alt+I` | Send it home |
| Middle-click the background, or `Super+Shift+I` | Tuck it into a thin sliver, and hover the sliver to bring the panel up |
| Scroll on the pill | Move through occupied workspaces |

The double-click goes on empty pill background such as the clock, because the
workspace chips have their own clicks.

While it's docked, the pill reserves the same 40px Waybar did, so tiled windows stay
where they were. Moved or tucked, it gives that space back. Pin, tuck and position
survive restarts, and each monitor remembers its own position.

Inside the panel:

| Control | Left | Right | Middle | Scroll |
|---|---|---|---|---|
| Workspace chip | Switch to it | Send the focused window there | | |
| App icon | Focus that window, switching workspace if needed | Send the focused window to that workspace | | |
| Volume slider | Set volume, or drag | `pavucontrol` | | ±5% |
| Speaker icon | Mute | `pavucontrol` | | |
| Network | `wifi-menu` | `nmtui` | | |
| Wallpaper button | Pick | Random | Pause or resume | |
| Tray icon | Activate | App's menu | Secondary activate | |
| Album art on the pill | Play or pause | Next | Previous | |

## Configuration

`~/.config/island/config` is optional:

```ini
font = Adwaita Sans
clock = %I:%M %p
reserve = true      # false: float over windows instead of reserving space
```

Colors need no setup. The island reads `~/.config/theme/generated/colors.toml`, the
palette matugen already writes, and repaints when it changes. Accent colors go only
on active items, and every color is adjusted to keep readable contrast on light and
dark palettes.

## How it works

| File | Job |
|---|---|
| `src/main.rs` | Event loop, Wayland handlers, actions |
| `src/island.rs` | One island per output: states, springs, frames, input |
| `src/draw.rs` | Layout of the pill and panel |
| `src/mango.rs` | Mango IPC |
| `src/icons.rs` | App id to `.desktop` entry to icon file to raster |
| `src/bus.rs` | Media and the system tray, including the tray watcher |
| `src/net.rs` `src/audio.rs` `src/sys.rs` | Network, volume, hardware stats |
| `src/theme.rs` `src/settings.rs` | Palette, config, saved state |

Why it doesn't use the obvious approach in a few places:

- **The surface never resizes.** A layer surface resize takes a round trip per frame,
  so one fixed transparent surface holds the morph, and the input region follows the
  visible shape so clicks pass through the rest.
- **Rendering is CPU only.** Cairo draws into shared memory with no GL context. The
  pill and panel render once into cached surfaces that only change with their
  content, so animation frames just composite those.
- **Nothing runs while idle.** There are no animation loops, and the only timer is
  the clock's, once a minute. Mango, audio, media, network and theme all arrive as
  events. Stats are read only while a panel is open.
- **Drags use relative pointer motion.** Surface coordinates race the island's own
  position updates and jitter.
- **NVML lives in a child process.** It uses tens of MB, and each init and shutdown
  leaks a file descriptor inside the driver library. The `--gpu-sampler` child runs
  only while a panel is open.
- **The island owns the tray watcher.** Waybar used to, so the island claims the name
  and adopts apps already on the bus.

## Tests

```sh
cargo test                 # unit tests
tests/headless/all.sh      # end-to-end
```

The end-to-end suite runs a private headless Mango and a private session bus, drives
a virtual pointer, and checks results through Mango's IPC and screenshots. Audio is
shimmed so it never touches your real volume. It covers clicks, drag and dock, tuck,
pin, hotplug, two monitors, media, tray menus, theme changes and the install cutover.

Never start a test Mango with `DBUS_SESSION_BUS_ADDRESS` set: on exit it stops
`graphical-session.target`, which takes the live desktop down. `start.sh` unsets it.

## Limits

- Fractional scales render at the next whole scale and the compositor scales down.
- Clicking a chip on another monitor moves the cursor there, because of Mango's
  `warpcursor`, which is on by default.
- GPU stats are NVIDIA only. Other GPUs simply have no GPU tile.
- Each pill stays on its own monitor; it can't be dragged across to another.
- Long tray menus are cut off at the panel's maximum height. They don't scroll.
