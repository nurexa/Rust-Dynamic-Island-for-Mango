#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
export ISLAND_BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
[ -f $R/dbus.pid ] && kill $(cat $R/dbus.pid) 2>/dev/null; sleep .2
rm -f $R/bus; dbus-daemon --session --nofork --address=unix:path=$R/bus >/dev/null 2>&1 & echo $! > $R/dbus.pid; sleep .3
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
[ -f $R/art.jpg ] || ffmpeg -nostdin -v error -y -f lavfi -i "gradients=s=600x400:c0=0x8a2be2:c1=0xff7f50:x0=0:y0=0:x1=600:y1=400:n=2" -frames:v 1 $R/art.jpg
echo '{"mute":false,"vol":48}' > $R/sink; : > $R/events
owner(){ busctl --user status org.kde.StatusNotifierWatcher 2>/dev/null | awk -F= '/^Comm=/{print $2}'; }
items(){ busctl --user get-property org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems 2>/dev/null | awk '{print $2}'; }

# Waybar-like watcher already owns the name, and an app is on the bus
nohup setsid python3 -I $H/fakew.py >/dev/null 2>&1 & FW=$!; sleep .5
nohup setsid $H/../../target/release/examples/mock $H/run/art.jpg > $R/mock.log 2>&1 & echo $! > $R/mock.pid; sleep 1
$H/isl; sleep .3
ok "host-only while someone else owns the watcher" "$(owner)" "python3"
PA=$(ps --ppid $(cat $R/island.pid) -o pid= | head -1)
kill $FW; sleep 1
ok "takes the watcher when it's released" "$(owner)" "island"
ok "adopts the app that registered with the old watcher" "$(items)" 1
$V move 960 22; sleep 1.1; grim $H/run/j1.png; $V move 1500 900; sleep .6
$H/isl stop; sleep .3
ok "pactl child dies with the island" "$(kill -0 $PA 2>/dev/null && echo alive || echo gone)" gone
# Second instance is a clean no-op, so the restart loop can't spin
$H/isl; $ISLAND_BIN; ok "second instance exits 0" "$?" 0
# Scroll switches workspaces
mmsg dispatch view,1,0 >/dev/null; sleep .3
$V move 960 22; sleep .1; $V scroll 1; sleep .5
ok "wheel down on pill: next occupied workspace" "$(mmsg get all-monitors | python3 -c "import json,sys;print(json.load(sys.stdin)['monitors'][0]['active_tags'])")" "[3]"
$V scroll -1; sleep .5; $V move 1500 900; sleep .6
# HiDPI
wlr-randr --output HEADLESS-1 --scale 2; sleep 1
$V move 480 11; sleep 1.2; grim $H/run/j2.png; $V move 800 400; sleep .7
wlr-randr --output HEADLESS-1 --scale 1.5; sleep 1; grim $H/run/j3.png
wlr-randr --output HEADLESS-1 --scale 1; sleep 1; grim $H/run/j4.png
kill -0 $(cat $R/island.pid) && echo "ok   alive through scale changes"
tail -2 $R/island.log
