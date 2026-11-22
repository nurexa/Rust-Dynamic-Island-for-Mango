#!/bin/bash
# Full end-to-end run. Needs mango, grim, kitty, pavucontrol, waybar, ffmpeg, python-dbus, busctl.
# Everything runs in a private headless compositor on a private bus, with audio shimmed
# and state in run/, so nothing here can reach your island's.
# Deep checkouts: a Wayland socket path must be under 108 bytes, so point ISLAND_HEADLESS at a short
# symlink to this directory (ln -s $PWD ~/.cache/ih) and run the scripts through it.
H=${ISLAND_HEADLESS:-$(dirname "$(readlink -f "$0")")}
(cd $(readlink -f $H/../..) && cargo build --release --bins --examples 2>&1 | tail -1)
mkdir -p $H/run
export ISLAND_BIN=$(readlink -f $H/../../target/release/island)
# Not even an island install.sh starts may reach the real BlueZ; bluetooth.sh brings a fake.
export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent
$H/fresh.sh 1 >/dev/null; $H/clicks.sh; $H/movement.sh; $H/fullscreen.sh; $H/hotplug.sh
$H/fresh.sh 2 >/dev/null; $H/monitors.sh
$H/fresh.sh 1 >/dev/null; $H/full.sh; $H/controls.sh; $H/audio.sh; $H/battery.sh; $H/claude.sh; $H/theme.sh
$H/fresh.sh 1 >/dev/null; $H/full.sh; $H/tools.sh; $H/notifications.sh; $H/palette-soak.sh
$H/fresh.sh 1 >/dev/null; $H/takeover.sh
$H/fresh.sh 1 >/dev/null; $H/modules.sh; $H/grid.sh; $H/bare.sh
$H/fresh.sh 1 >/dev/null; $H/wallpaper.sh
$H/fresh.sh 1 >/dev/null; $H/full.sh; $H/bluetooth.sh
$H/fresh.sh 1 >/dev/null; $H/full.sh; $H/styles.sh
$H/fresh.sh 1 >/dev/null; $H/full.sh; $H/sidebar.sh
$H/fresh.sh 1 >/dev/null; $H/full.sh; $H/isl stop; $H/cutover.sh
# Only what carries this runtime dir: a stale pid file can name someone else's process.
for f in $H/run/*pid; do
  [ -f "$f" ] || continue
  for p in $(cat "$f"); do
    grep -qzxF "XDG_RUNTIME_DIR=$H/run" "/proc/$p/environ" 2>/dev/null && kill "$p"
  done
done
