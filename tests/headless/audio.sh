#!/bin/bash
# Per-app volume, picking devices, and the mic dot, against the fake pactl.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
orange(){ px "$@" | awk '{print ($1>200 && $3<80) ? "orange" : "no"}'; }
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
: > $R/shim.log
echo '[{"index":7,"mute":false,"volume":{"mono":{"value_percent":"64%"}},"properties":{"application.name":"Firefox","application.icon_name":"firefox"}}]' > $R/apps
echo '[{"source":2,"properties":{"application.name":"OBS"}}]' > $R/rec
echo "Event 'new' on source-output #3" >> $R/events; sleep .8; grim $H/run/a0.png
ok "recording the output's monitor isn't the mic" "$(orange $H/run/a0.png 971 22)" no
echo '[{"source":1,"properties":{"application.name":"Discord"}}]' > $R/rec
echo "Event 'new' on source-output #4" >> $R/events; sleep .8; grim $H/run/a1.png
ok "an app on the mic lights the orange dot" "$(orange $H/run/a1.png 971 22)" orange
$BIN pin; sleep 1; $BIN audio; sleep 1
$V click 1100 467; sleep .5
ok "an app's slider sets its own volume" "$(grep -c 'pactl set-sink-input-volume 7 ' $R/shim.log)" 1
$V click 800 418; sleep .5
ok "a click on another output makes it the default" "$(grep -c "pactl set-default-sink hdmi" $R/shim.log)" 1
$V click 710 515; sleep .5
ok "and the same for the microphone" "$(grep -c "pactl set-default-source mic" $R/shim.log)" 1
$V click 715 467; sleep .5
ok "the app's icon mutes it" "$(grep -c 'pactl set-sink-input-mute 7 toggle' $R/shim.log)" 1
echo '[]' > $R/rec; echo "Event 'remove' on source-output #4" >> $R/events
$BIN audio; sleep .3; $BIN pin; sleep 1.2; grim $H/run/a2.png
ok "the dot goes when the recording stops" "$(orange $H/run/a2.png 971 22)" no
rm -f $R/apps $R/rec
