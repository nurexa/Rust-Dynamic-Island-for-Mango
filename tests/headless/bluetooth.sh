#!/bin/bash
# Bluetooth against a fake BlueZ (fakebt.py) on a private system bus, so the real
# adapter and whatever headphones are paired to it are never touched. Notes go out
# through a notify-send shim, so their exact text can be checked. Run after full.sh.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH ISLAND_SYSTEM_BUS=unix:path=$R/sysbus
export XDG_CONFIG_HOME=$R/cfgbt; mkdir -p $XDG_CONFIG_HOME/island
LOG=$R/bt.log; : > $LOG
layer(){ mmsg get all-layers | python3 -c "import json,sys;print([l['layer'] for l in json.load(sys.stdin)['layers'] if l['name']=='island'][0])"; }
logged(){ grep -c -- "$1" $LOG; }
ctl(){ DBUS_SYSTEM_BUS_ADDRESS=$ISLAND_SYSTEM_BUS busctl --system call org.bluez / test.Control "$@"; }
# Context switches of the island's main thread: how often the UI woke up.
woke(){ awk '/^voluntary_ctxt_switches/{print $2}' /proc/$1/task/$1/status; }
ticks(){ awk '{print $14+$15}' /proc/$1/stat; }

printf '#!/bin/sh\necho "note $*" >> %s\n' "$LOG" > $H/shim/notify-send; chmod +x $H/shim/notify-send
echo 'notifications = false' > $XDG_CONFIG_HOME/island/config.toml
for f in sysbus fakebt; do [ -f $R/$f.pid ] && kill $(cat $R/$f.pid) 2>/dev/null; done
dbus-daemon --session --nofork --address=$ISLAND_SYSTEM_BUS >/dev/null 2>&1 & echo $! > $R/sysbus.pid; sleep .3
DBUS_SYSTEM_BUS_ADDRESS=$ISLAND_SYSTEM_BUS nohup setsid python3 -I $H/fakebt.py $LOG > $R/fakebt.log 2>&1 &
echo $! > $R/fakebt.pid; sleep .8
$H/isl; sleep .3; $V move 1500 900; sleep .4
P=$(cat $R/island.pid)

# The panel: switch on, the connected headset and its battery
ACC="255 182 146"; on(){ [ "$(px $1 1190 371)" = "$ACC" ] && echo on || echo off; }
$V move 960 22; sleep 1.2; grim $H/run/bt1.png
ok "panel: the switch shows on" "$(on $H/run/bt1.png)" on
ok "panel: a row for the connected headset" "$(px $H/run/bt1.png 700 410 opaque)" yes
$V click 1202 371; sleep .6
ok "switch: turns the adapter off" "$(logged 'set /org/bluez/hci0 Powered False')" 1
grim $H/run/bt2.png
ok "switch: shows off, and the headset row goes with it" "$(on $H/run/bt2.png) $(px $H/run/bt2.png 700 410 opaque)" "off no"
$V click 760 371; sleep 1
ok "row: opens the Bluetooth tool" "$(layer)" overlay
$V key Return; sleep .9
ok "tool: off, so Enter turns it back on" "$(logged 'set /org/bluez/hci0 Powered True')" 1
$V move 1500 900; sleep .8

# Connect, a failure, disconnect, each with the note it should leave
pick(){ $BIN open bluetooth; sleep 1; $V type "$1"; sleep .3; $V key Return; sleep .9; }
pick mouse
ok "tool: Enter connects a paired device" "$(logged 'connect /org/bluez/hci0/dev_MS')" 1
ok "tool: and says so" "$(logged 'note -a bluetooth Bluetooth Connected to Test Mouse')" 1
pick broken
ok "tool: a failed connect says so" "$(logged "note -a bluetooth Bluetooth Couldn't connect to Broken Pad")" 1
pick headset
$V move 960 22; sleep 1.2; grim $H/run/bt3.png; $V move 1500 900; sleep .8
ok "panel: lists both connected devices" "$(px $H/run/bt3.png 700 434 opaque)" yes

# Low battery: one note, not one per change, and again only after it charged
for pct in 12 10 18 60 9; do ctl SetBattery oy /org/bluez/hci0/dev_HS $pct >/dev/null; sleep .3; done
ok "battery: warned twice, at 12% and, after charging, at 9%" "$(logged 'battery low')" 2
ok "battery: says which device" "$(logged 'note -a bluetooth Bluetooth Test Headset battery low: 12%')" 1
pick headset
ok "tool: Enter on a connected device disconnects it" "$(logged 'disconnect /org/bluez/hci0/dev_HS')" 1

# Scan: the palette waits on it, signal-strength chatter doesn't wake the UI
$BIN open bluetooth; sleep 1; $V type scan; sleep .3; $V key Return; sleep 1.5
ok "scan: discovery started" "$(logged 'discovery start')" 1
ok "scan: the palette stays open while it runs" "$(layer)" overlay
w0=$(woke $P); sleep 4; w1=$(woke $P)
ok "scan: chatter doesn't wake the UI" "$([ $((w1-w0)) -le 3 ] && echo quiet || echo "$((w1-w0)) wakeups")" quiet
sleep 10.5; grim $H/run/bt4.png
ok "scan: stops after 15 seconds" "$(logged 'discovery stop')" 1
$V key Return; sleep 1
ok "scan: Enter on a new device pairs it" "$(logged 'pair /org/bluez/hci0/dev_SPK')" 1
ok "scan: trusts it" "$(logged 'set /org/bluez/hci0/dev_SPK Trusted True')" 1
ok "scan: and connects" "$(logged 'note -a bluetooth Bluetooth Paired and connected to Test Speaker')" 1

# No adapter, no row; one plugged back in comes back
$V move 960 22; sleep 1.2; grim $H/run/bt5.png; $V move 1500 900; sleep .8
ctl Adapter b false >/dev/null; sleep .5
$V move 960 22; sleep 1.2; grim $H/run/bt6.png; $V move 1500 900; sleep .8
ok "adapter gone: the panel ends above where the row was" "$(px $H/run/bt5.png 700 380 opaque) $(px $H/run/bt6.png 700 380 opaque)" "yes no"
ctl Adapter b true >/dev/null; sleep .5
$V move 960 22; sleep 1.2; grim $H/run/bt7.png; $V move 1500 900; sleep .8
ok "adapter back: so is the row" "$(px $H/run/bt7.png 700 380 opaque)" yes

# Idle: nothing runs while nothing changes
sleep 1; t0=$(ticks $P); w0=$(woke $P); sleep 5; t1=$(ticks $P); w1=$(woke $P)
ok "idle: no CPU, no wakeups" "$((t1-t0)) $([ $((w1-w0)) -le 2 ] && echo quiet || echo "$((w1-w0)) wakeups")" "0 quiet"
kill -0 $P && echo "ok   alive after Bluetooth"
grep -vE 'no supported|another notification' $R/island.log | tail -3
rm -f $H/shim/notify-send
for f in sysbus fakebt; do kill $(cat $R/$f.pid) 2>/dev/null; done
unset XDG_CONFIG_HOME ISLAND_SYSTEM_BUS
$H/isl
