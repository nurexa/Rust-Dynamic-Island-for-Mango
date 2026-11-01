# Rust Dynamic Island for Mango

An Apple-style Dynamic Island for [MangoWC](https://mangowm.github.io), written in
Rust. It replaces Waybar with one pill that floats over your windows, shows your
workspaces and the apps on them, and springs open into a panel when you hover it.

**[Project page](https://nurexa.github.io/Rust-Dynamic-Island-for-Mango/)** ·
[Modding guide](MODDING.md)

![The pill opens on hover, switches workspace, drags away and docks again](docs/demo.gif)

## Waybar vs island

Measured with a daily-driver Waybar config on the same compositor, with the same
windows and apps. Each number is the median of 3 runs, and child processes count.

| | Waybar 0.15 | Island |
|---|---|---|
| Real memory (PSS) | 37.3 MB | **10.9 MB** |
| Resident memory (RSS) | 77.1 MB | **34.7 MB** |
| CPU per workspace switch | 45.75 ms | **2.5 ms** |
| Startup | 162 ms | **69 ms** |
| Linked shared libraries | 117 | **39** |
| Idle CPU, across 12 runs | 0–2.2% | 0–0.1% |
| Binary size | **2.5 MB** | 3.1 MB |

Idle is effectively a tie, since both sit near zero and Waybar moves with audio
server activity. Waybar's switch cost includes the helper commands its custom
workspace modules run, but not the separate script that feeds them. Run
`bench/run.py` to reproduce this; raw results are in `bench/results.json`.

## Install

From a terminal inside your Mango session:

```sh
git clone https://github.com/nurexa/Rust-Dynamic-Island-for-Mango
cd Rust-Dynamic-Island-for-Mango
./install.sh          # build, install to ~/.local/bin, switch from Waybar to the island
./install.sh --undo   # put Waybar back
```

The installer changes two files and keeps the originals in
`~/.local/share/island-backup`:

- `~/.config/mango/scripts/bar`, the autostart loop, which it creates if you don't
  have one
- a marked block at the end of `~/.config/mango/config.conf`, with one layer rule
  and three key bindings

It checks the new config with Mango's parser and rolls back on its own if the island
doesn't come up. Waybar stays installed until you remove it. If something other than
that script starts your Waybar, turn that off yourself.

Builds need Rust, `cairo` and `pango`. At runtime the island uses `pactl`, `wpctl`,
`playerctl` (with `playerctld`), `curl`, `adwaita-icon-theme` and any Nerd Font.

## Using it

| Do this | To get this |
|---|---|
| Hover the pill | Open the panel; move away to close it |
| Click the pill background, or `Super+I` | Keep the panel open (pin) |
| Drag the pill | Move it anywhere on that monitor |
| Drop it near the top centre | Dock it again |
| Double-click the background, or `Super+Alt+I` | Send it home |
| Middle-click the background, or `Super+Shift+I` | Tuck it into a thin sliver |
| Scroll on the pill | Move through occupied workspaces |

The island always floats over your windows. It never reserves space or moves them.
It stays under fullscreen windows, and pin, tuck and position survive restarts, with
a position per monitor.

Inside the panel:

| Control | Left | Right | Middle | Scroll |
|---|---|---|---|---|
| Workspace chip | Switch to it | Send the focused window there | | |
| App icon | Focus that window | Send the focused window to that workspace | | |
| Volume slider | Set volume, or drag | Open the mixer | | ±5% |
| Speaker icon | Mute | Open the mixer | | |
| Network | Run `wifi` | Run `wifi_alt` | | |
| Wallpaper button | Pick | Random | Pause or resume | |
| Tray icon | Activate | App's menu | Secondary activate | |
| Album art on the pill | Play or pause | Next | Previous | |

## Make it yours

Everything is in `~/.config/island/config.toml`, and `island reload` applies changes
without a restart. [`examples/config.toml`](examples/config.toml) lists every option.

```toml
pill = ["media", "workspaces", "custom:weather", "clock"]
icon_style = "glyph"          # two-tone Nerd Font glyphs, or "color" for real app icons

[[custom]]                    # any shell command is a module
name = "weather"
exec = "curl -s 'wttr.in/?format=%t'"
interval = 600

[commands]
wifi = "nm-connection-editor"
wallpaper_pick = "my-wallpaper-script"
```

Colors come from a small token file, which [matugen](examples/matugen-colors.toml.tmpl)
can generate from your wallpaper. The island repaints when the file changes.

New modules are one file each, and compositor support sits behind a five-method
trait. [MODDING.md](MODDING.md) covers both, with a worked example.

## How it works

| Path | Job |
|---|---|
| `src/modules/` | One file per pill and panel module, and the list that orders them |
| `src/wm/` | Compositor backends, and the trait they implement |
| `src/island.rs` | One island per output: states, springs, frames, input |
| `src/pen.rs` | The drawing kit modules share, and the glyph table |
| `src/bus.rs` | Media and the system tray, including the tray watcher |
| `src/net.rs` `src/audio.rs` `src/sys.rs` | Network, volume, hardware stats |
| `src/theme.rs` `src/icons.rs` `src/settings.rs` | Palette, icons, config, saved state |

Why it doesn't use the obvious approach in a few places:

- **The surface never resizes.** A layer surface resize takes a round trip per frame,
  so one fixed transparent surface holds the morph, and the input region follows the
  visible shape so clicks pass through the rest.
- **Rendering is CPU only.** Cairo draws into shared memory with no GL context. The
  pill and panel render once into cached surfaces, so animation frames just composite
  those.
- **Nothing runs while idle.** There are no animation loops, and Mango, audio, media,
  network and theme all arrive as events. Stats are read only while a panel is open.
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
shimmed so it never touches your real volume.

Never start a test Mango with `DBUS_SESSION_BUS_ADDRESS` set: on exit it stops
`graphical-session.target`, which takes the live desktop down. `start.sh` unsets it.

## Limits

- Workspaces need a compositor backend, and Mango's is the only one so far. On other
  layer-shell compositors everything else should work; see [MODDING.md](MODDING.md).
- Fractional scales render at the next whole scale and the compositor scales down.
- Clicking a chip on another monitor moves the cursor there, because of Mango's
  `warpcursor`, which is on by default.
- GPU stats are NVIDIA only. Other GPUs simply have no GPU tile.
- Each pill stays on its own monitor; it can't be dragged across to another.
- Long tray menus are cut off at the panel's maximum height. They don't scroll.

## Star history

[![Star history](https://api.star-history.com/svg?repos=nurexa/Rust-Dynamic-Island-for-Mango&type=Date)](https://star-history.com/#nurexa/Rust-Dynamic-Island-for-Mango&Date)

## License

GPL-3.0. See [LICENSE](LICENSE).
