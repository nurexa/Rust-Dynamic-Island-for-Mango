#!/bin/bash
# Every style: the space it takes, that it's joined to its edge, what it lets
# through, and that switching and restarting keep it. Needs fresh.sh 1 and full.sh.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; R=$XDG_RUNTIME_DIR; V=$H/vp
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
tags(){ mmsg get all-monitors | python3 -c "import json,sys;print(json.load(sys.stdin)['monitors'][0]['active_tags'])"; }
win(){ mmsg get all-clients | python3 -c "import json,sys;print(next('%d,%d %dx%d'%(c['x'],c['y'],c['width'],c['height']) for c in json.load(sys.stdin)['clients'] if c['appid']=='kitty'))"; }
focused(){ mmsg get focusing-client | python3 -c "import json,sys;print(json.load(sys.stdin).get('appid'))"; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sum(l['name']=='island' for l in json.load(sys.stdin)['layers']))"; }
overlay(){ mmsg get all-layers | python3 -c "import json,sys;print(sum(l['name']=='island' and l['layer']=='overlay' for l in json.load(sys.stdin)['layers']))"; }
style(){ $BIN style $1; sleep 1.5; }
shot(){ grim $R/st.png; }
WIN="15 20 23"

$V move 1500 900; sleep .6
ok "pill: one surface, windows untouched" "$(layers) $(win)" "1 12,12 1896x1056"

style frame
ok "frame: four strips and four reservers under the island" "$(layers)" 9
ok "frame: windows make room for the rail and border" "$(win)" "52,20 1848x1040"
shot
ok "frame: the rail and all three borders are drawn" "$(px $R/st.png 2 500 opaque) $(px $R/st.png 600 3 opaque) $(px $R/st.png 1916 540 opaque) $(px $R/st.png 600 1076 opaque)" "yes yes yes yes"
ok "frame: the inner corner is filled" "$(px $R/st.png 42 10 opaque)" yes
ok "frame: the window shows inside it" "$(px $R/st.png 600 500)" "$WIN"
$V click 20 138 l; sleep .7; ok "frame: a rail workspace switches to it" "$(tags)" "[3]"
$V click 20 84 l; sleep .7; ok "frame: and back" "$(tags) $(focused)" "[1] kitty"
$V click 600 500 l; sleep .4; ok "frame: clicks inside reach the window" "$(focused)" kitty
$V move 976 3; sleep 1.3; shot
ok "frame: hovering the top border grows the panel out of it" "$(px $R/st.png 700 200 opaque)" yes
ok "frame: with a reversed corner where it leaves the border" "$(px $R/st.png 694 9 opaque) $(px $R/st.png 686 24 opaque)" "yes no"
$V move 600 700; sleep 1.3; shot
ok "frame: leaving draws it back in" "$(px $R/st.png 700 200)" "$WIN"
$V click 20 1038 l; sleep 1.3; shot
ok "frame: the rail's power button opens the palette from the bottom" "$(overlay) $(px $R/st.png 700 1040 opaque) $(px $R/st.png 700 200 opaque)" "1 yes no"
$BIN close; sleep 1.3; shot
ok "frame: it goes back into the bottom border" "$(overlay) $(px $R/st.png 700 1040 opaque)" "0 no"
$V move 600 700; $BIN pin; sleep 1.4
$BIN open apps; sleep 1.3; $BIN close; sleep 2; shot
ok "frame: a pinned panel returns to the top after the palette" "$(px $R/st.png 700 200 opaque) $(px $R/st.png 700 1040 opaque)" "yes no"
$BIN pin; sleep 1.3
mmsg dispatch togglefullscreen >/dev/null; sleep .9; shot
ok "frame: fullscreen covers all of it" "$(px $R/st.png 2 500)" "$WIN"
mmsg dispatch togglefullscreen >/dev/null; sleep .9; shot
ok "frame: and it's back afterwards" "$(px $R/st.png 2 500 opaque)" yes
$H/isl; sleep 1.2
ok "frame: a restart keeps it" "$(layers) $(win)" "9 52,20 1848x1040"

style bar
ok "bar: a strip, a reserver and the island" "$(layers)" 3
ok "bar: windows move down by its height" "$(win)" "12,44 1896x1024"
$V move 1500 900; sleep .5; shot
ok "bar: full width, rounded under the corners" "$(px $R/st.png 300 2 opaque) $(px $R/st.png 1 35 opaque) $(px $R/st.png 300 100)" "yes yes $WIN"
$V click 1898 16 l; sleep 1.2
ok "bar: the power button opens the palette" "$(overlay)" 1
$BIN close; sleep 1.2
$V move 960 16; sleep 1.3; shot
ok "bar: it swells down into the panel" "$(px $R/st.png 700 200 opaque)" yes
$V move 1500 900; sleep 1.3

style notch
ok "notch: one surface, windows untouched" "$(layers) $(win)" "1 12,12 1896x1056"
$V move 1500 900; sleep 1; shot
ok "notch: it starts at the very edge" "$(px $R/st.png 960 1 opaque)" yes
$V move 960 10; sleep 1.3; shot
ok "notch: opens with shoulders on the edge" "$(px $R/st.png 676 1 opaque) $(px $R/st.png 700 200 opaque)" "yes yes"
$V move 1500 900; sleep 1.3

for s in split minimal; do
    style $s; $V move 1500 900; sleep .8; shot
    ok "$s: one surface, windows untouched" "$(layers) $(win)" "1 12,12 1896x1056"
    ok "$s: windows show around it" "$(px $R/st.png 700 22)" "$WIN"
done

glassy(){ mmsg get all-layers | python3 -c "import json,sys;print(sum(l['name']=='island-blur' for l in json.load(sys.stdin)['layers']))"; }
style glass; $V move 1500 900; sleep .8; shot
ok "glass: its surfaces take the name the blur rule wants" "$(glassy) $(layers)" "3 0"
ok "glass: windows move down by its height and gap" "$(win)" "12,50 1896x1018"
ok "glass: it floats, short of both ends" "$([ "$(px $R/st.png 600 22)" != "$(px $R/st.png 100 22)" ] && echo yes) $([ "$(px $R/st.png 100 22)" = "$(px $R/st.png 1820 22)" ] && echo yes)" "yes yes"
ok "glass: one layer where the island meets the bar, not two" "$(px $R/st.png 592 34)" "$(px $R/st.png 598 34)"
$V move 960 22; sleep 1.3; shot
ok "glass: it swells down into the panel" "$([ "$(px $R/st.png 700 200)" != "$WIN" ] && echo yes)" yes
$V move 1500 900; sleep 1.3

style pill
$BIN set blur on; sleep 1.5
ok "blur on: any style takes the blur rule's name" "$(glassy)" 1
$BIN set blur auto; sleep 1.5
ok "blur auto: and drops it" "$(glassy) $(layers)" "0 1"
ok "set: a key that means nothing is refused" "$($BIN set nonsense on >/dev/null 2>&1; echo $?)" 2
ok "set: a value out of range is refused" "$($BIN set scale 9 >/dev/null 2>&1; echo $?)" 2
$BIN settings; sleep 1.2
ok "settings: opens on the island" "$(overlay)" 1
$BIN close; sleep 1.2

# The cut-off: at twice the size there's room for half a panel, and the rest scrolls.
for a in one two three; do notify-send -a $a -t 500 "From $a" "History for the panel"; done; sleep 4
$BIN set scale 2; sleep 1.5; $BIN pin; sleep 1.8; $V move 960 600; sleep .6; shot; cp $R/st.png $R/st-top.png
ok "tall panel: it stops short of the screen's edge" "$(px $R/st.png 420 1020 opaque) $([ "$(px $R/st.png 960 1078)" = "$(px $R/st.png 420 1020)" ] || echo clear)" "yes clear"
$V scroll 3; sleep .8; shot
ok "tall panel: the wheel scrolls it, not the workspaces" "$(tags) $(cmp -s $R/st.png $R/st-top.png || echo moved)" "[1] moved"
$BIN pin; sleep 1; $BIN set scale 1; sleep 1.5; $V move 1500 900
ok "scale: back at 1 nothing is saved over the config" "$(grep -c '^set scale' $R/island/state)" 0

ok "an unknown style is refused, with the names" "$($BIN style blob 2>&1; echo " $?")" "island: styles are pill, frame, notch, bar, glass, split, minimal
 2"
style pill
ok "back to pill: every edge surface is gone and the space is given back" "$(layers) $(win)" "1 12,12 1896x1056"
for s in frame bar notch frame split pill frame pill; do $BIN style $s; sleep .15; done; sleep 1.5
ok "switching fast leaves nothing behind" "$(layers) $(win)" "1 12,12 1896x1056"

$BIN style frame; sleep 1.5
mmsg dispatch create_virtual_output >/dev/null; sleep 1.5
ok "frame: a new monitor gets its own" "$(layers)" 18
wlr-randr --output HEADLESS-2 --off; sleep 1.2
ok "frame: and takes it along when it goes" "$(layers)" 9
wlr-randr --output HEADLESS-1 --scale 2; sleep 2; shot
ok "frame: at scale 2 it still meets every edge" "$(px $R/st.png 3 500 opaque) $(px $R/st.png 1200 6 opaque) $(px $R/st.png 1916 540 opaque) $(px $R/st.png 1200 1074 opaque)" "yes yes yes yes"
wlr-randr --output HEADLESS-1 --scale 1; sleep 2
ok "frame: and back at scale 1" "$(win)" "52,20 1848x1040"
style pill
kill -0 $(cat $R/island.pid) && echo "ok   alive"
