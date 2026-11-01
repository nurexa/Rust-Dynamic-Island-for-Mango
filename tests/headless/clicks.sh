#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
tags(){ mmsg get all-monitors | python3 -c "import json,sys;print(json.load(sys.stdin)['monitors'][0]['active_tags'])"; }
focused(){ mmsg get focusing-client | python3 -c "import json,sys;d=json.load(sys.stdin);print(d.get('appid'))"; }
kittytags(){ mmsg get all-clients | python3 -c "import json,sys;print([c['tags'] for c in json.load(sys.stdin)['clients'] if c['appid']=='kitty'])"; }
px(){ python3 $H/px.py "$@"; }
WIN="15 20 23"   # kitty background from the test config, now behind the island
$H/isl
$V move 1500 900; sleep .8; grim $H/run/a.png
ok "idle: window runs behind the pill" "$(px $H/run/a.png 800 22)" "$WIN"
ok "idle: pill is drawn" "$(px $H/run/a.png 950 10 opaque)" "yes"

# click-through: a window under the surface but outside the pill still gets pointer focus
mmsg dispatch togglefloating >/dev/null; sleep .4
$V click 700 400 l; sleep .4; ok "click at transparent surface reaches the window (kitty focused)" "$(focused)" "kitty"
mmsg dispatch togglefloating >/dev/null; sleep .4

$V move 960 22; sleep 1; grim $H/run/b.png
ok "hover opens panel" "$(px $H/run/b.png 700 30 opaque)" "yes"
ok "keyboard focus not stolen by hover" "$(focused)" "kitty"
$V move 1500 900; sleep 1; grim $H/run/c.png
ok "leave closes panel" "$(px $H/run/c.png 800 22)" "$WIN"

# tag click in compact: chip 3 is at x ~ 925 given chips [1 kitty][3 pav]
$V move 960 22; sleep 1
$V click 840 106; sleep .5; ok "panel chip click switches tag" "$(tags)" "[3]"
$V click 730 106; sleep .5; ok "panel chip click switches back" "$(tags)" "[1]"
# app icon on another tag: focuses that window and its tag
$V click 848 106; sleep .6; ok "icon click focuses window on other tag" "$(focused) $(tags)" "org.pulseaudio.pavucontrol [3]"
$V click 738 106; sleep .6; ok "icon click returns" "$(focused) $(tags)" "kitty [1]"
# right click on an empty chip moves focused window there, silently
$V click 786 106 r; sleep .6; ok "right click moves focused window (tagsilent)" "$(kittytags) $(tags)" "[[2]] [1]"
$V click 786 106 l; sleep .4; ok "follow it" "$(tags)" "[2]"
mmsg dispatch "tag,1,0" >/dev/null; sleep .4
mmsg dispatch "view,1,0" >/dev/null; sleep .4
ok "restored" "$(kittytags) $(tags)" "[[1]] [1]"

# fullscreen covers the island
$V move 1500 900; sleep .7
mmsg dispatch togglefullscreen >/dev/null; sleep .8; grim $H/run/d.png
echo "     fullscreen pixel at pill: $(px $H/run/d.png 950 10) (window bg, not pill black)"
mmsg dispatch togglefullscreen >/dev/null; sleep .8
echo "--- layers"; mmsg get all-layers
