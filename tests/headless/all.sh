#!/bin/bash
# Full end-to-end run. Needs mango, grim, kitty, pavucontrol, waybar, ffmpeg, python-dbus.
# Everything runs in a private headless compositor on a private bus, with audio shimmed.
H=$(dirname "$(readlink -f "$0")")
(cd $H/../.. && cargo build --release --bins --examples 2>&1 | tail -1)
export ISLAND_BIN=$(readlink -f $H/../../target/release/island)
stop(){ [ -f $H/run/vp.pid ] && kill $(cat $H/run/vp.pid) 2>/dev/null; }
stop; $H/fresh.sh 1 >/dev/null; $H/clicks.sh; $H/movement.sh; $H/hotplug.sh
stop; $H/fresh.sh 2 >/dev/null; $H/monitors.sh
stop; $H/fresh.sh 1 >/dev/null; $H/full.sh; $H/controls.sh; $H/theme.sh
stop; $H/fresh.sh 1 >/dev/null; $H/takeover.sh
stop; $H/fresh.sh 1 >/dev/null; $H/full.sh; $H/isl stop; $H/cutover.sh
source $H/run/env; [ -f $XDG_RUNTIME_DIR/dbus.pid ] && kill $(cat $XDG_RUNTIME_DIR/dbus.pid)
stop; kill $(cat $H/run/island.pid 2>/dev/null) 2>/dev/null; kill $(cat $H/run/pid)
