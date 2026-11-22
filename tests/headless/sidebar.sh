#!/bin/bash
# The sidebar, the calendar in it, and capture following playback and visibility.
# parec is the shim's, so nothing is recorded. Needs fresh.sh 1 and full.sh.
H=${ISLAND_HEADLESS:-$(dirname "$(readlink -f "$0")")}; source $H/run/env; R=$XDG_RUNTIME_DIR; V=$H/vp
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sum(l['name']=='island' for l in json.load(sys.stdin)['layers']))"; }
shot(){ grim $R/sb.png; }
# Children of every thread, by command line: the shim runs as python3.
capturing(){
    local pid n=0 c
    pid=$(cat $R/island.pid)
    for c in $(cat /proc/$pid/task/*/children 2>/dev/null); do
        tr '\0' ' ' < /proc/$c/cmdline 2>/dev/null | grep -q parec && n=$((n+1))
    done
    echo $n
}
WIN="40 8 48"
CFG=$R/xdg/island/config.toml
cp $CFG $R/sidebar.config

$V move 1200 700; sleep 1.5
ok "playing, bars on the pill: capturing" "$(capturing)" 1
playerctl play-pause; sleep 1.2
ok "paused: capture stops" "$(capturing)" 0
playerctl play-pause; sleep 1.2
ok "playing again: capturing" "$(capturing)" 1
mmsg dispatch togglefullscreen >/dev/null; sleep 1.2
ok "under fullscreen: capture stops" "$(capturing)" 0
mmsg dispatch togglefullscreen >/dev/null; sleep 1.5
ok "uncovered: capturing" "$(capturing)" 1
$BIN tuck; sleep 1.5
ok "tucked away: capture stops" "$(capturing)" 0
$BIN tuck; sleep 1.5
ok "back out: capturing" "$(capturing)" 1
for c in $(cat /proc/$(cat $R/island.pid)/task/*/children); do
    tr '\0' ' ' < /proc/$c/cmdline | grep -q parec && kill $c
done
sleep 8
ok "capture that died comes back" "$(capturing)" 1

ok "closed, the sidebar has no surface" "$(layers)" 1
$BIN sidebar; sleep 1.6; shot
ok "open: one more surface" "$(layers)" 2
ok "open: it hangs off the right edge" "$(px $R/sb.png 1918 540 opaque)" yes
$BIN sidebar; sleep 1.8; shot
ok "closed again: the surface is gone" "$(layers) $(px $R/sb.png 1700 540)" "1 $WIN"

# A known height, so its buttons are where this says.
{ echo 'sidebar = ["calendar"]'; cat $R/sidebar.config; } > $CFG
$BIN reload; sleep .8
$BIN sidebar; sleep 1.8; shot
ok "calendar only: centred on the edge" "$(px $R/sb.png 1900 380 opaque) $(px $R/sb.png 1900 200)" "yes $WIN"
$V move 1870 424; sleep .3; $V click 1890 406 l; sleep 1.8
ok "its close button closes it" "$(layers)" 1

$BIN style frame; sleep 1.6
$V move 1916 540; sleep 1.8
ok "frame: resting on the right border opens it" "$(layers)" 10
$V move 1700 540; sleep 1
ok "frame: moving into it keeps it open" "$(layers)" 10
$V move 900 700; sleep 2
ok "frame: leaving closes it" "$(layers)" 9
$BIN style pill; sleep 1.5

{ echo 'disable = ["sidebar", "visualizer"]'; cat $R/sidebar.config; } > $CFG
$BIN reload; sleep 1.2
ok "visualizers off: nothing captures while playing" "$(capturing)" 0
$BIN sidebar; sleep 1
ok "sidebar off: the command does nothing" "$(layers)" 1
cp $R/sidebar.config $CFG; $BIN reload; sleep .8
kill -0 $(cat $R/island.pid) && echo "ok   alive"
