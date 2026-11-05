#!/bin/bash
# Per-app volume, picking devices, and the mic dot, against the fake pactl.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
# Any of the mic dot's orange across the pill, wherever the other modules put it.
orange(){ magick "$1" -crop 560x24+680+10 -fx "r>0.85 && g>0.5 && g<0.75 && b<0.25" -format "%[fx:maxima>0?1:0]" info: | sed 's/1/orange/;s/0/no/'; }
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
: > $R/shim.log
# Its own island with only the audio section, so the rows sit where the clicks expect.
export XDG_CONFIG_HOME=$R/cfgau; mkdir -p $XDG_CONFIG_HOME/island
echo 'panel = ["audio"]' > $XDG_CONFIG_HOME/island/config.toml
$H/isl; sleep .5
# Pin is saved across restarts and other tests leave it either way: set it, don't toggle it.
pinned(){ grep -qx pinned $R/island/state 2>/dev/null; }
pin(){ pinned || $BIN pin; }
unpin(){ pinned && $BIN pin; true; }
unpin; sleep .8
echo '[{"index":7,"mute":false,"volume":{"mono":{"value_percent":"64%"}},"properties":{"application.name":"Firefox","application.icon_name":"firefox"}}]' > $R/apps
echo '[{"source":2,"properties":{"application.name":"OBS"}}]' > $R/rec
echo "Event 'new' on source-output #3" >> $R/events; sleep .8; grim $H/run/a0.png
ok "recording the output's monitor isn't the mic" "$(orange $H/run/a0.png)" no
echo '[{"source":1,"properties":{"application.name":"Discord"}}]' > $R/rec
echo "Event 'new' on source-output #4" >> $R/events; sleep .8; grim $H/run/a1.png
ok "an app on the mic lights the orange dot" "$(orange $H/run/a1.png)" orange
pin; sleep 1; $BIN audio; sleep 1; grim -g "560,0 800x620" $H/run/a3.png
$V click 1100 163; sleep .5
ok "an app's slider sets its own volume" "$(grep -c 'pactl set-sink-input-volume 7 ' $R/shim.log)" 1
$V click 800 114; sleep .5
ok "a click on another output makes it the default" "$(grep -c "pactl set-default-sink hdmi" $R/shim.log)" 1
$V click 710 211; sleep .5
ok "and the same for the microphone" "$(grep -c "pactl set-default-source mic" $R/shim.log)" 1
$V click 715 163; sleep .5
ok "the app's icon mutes it" "$(grep -c 'pactl set-sink-input-mute 7 toggle' $R/shim.log)" 1
echo '[]' > $R/rec; echo "Event 'remove' on source-output #4" >> $R/events
$BIN audio; sleep .3; unpin; sleep 1.2; grim $H/run/a2.png
ok "the dot goes when the recording stops" "$(orange $H/run/a2.png)" no
rm -f $R/apps $R/rec
unset XDG_CONFIG_HOME; $H/isl
