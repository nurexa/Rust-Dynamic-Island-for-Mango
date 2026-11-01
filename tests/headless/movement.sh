#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
winy(){ mmsg get all-clients | python3 -c "import json,sys;print([c['y'] for c in json.load(sys.stdin)['clients'] if c['appid']=='kitty'][0])"; }
px(){ python3 $H/px.py "$@"; }
S=$XDG_RUNTIME_DIR/island/state
pos(){ read _ _ X Y < <(grep pos $S); X=${X%.*}; Y=${Y%.*}; }
WIN="15 20 23"; BG="42 74 90"
near(){ python3 -c "import sys;a=list(map(int,'$1'.split()));b=list(map(int,'$2'.split()));print('same' if max(abs(x-y) for x,y in zip(a,b))<=6 else 'diff')"; }
rm -f $S; $H/isl; $V move 1500 900; sleep .7

$V move 960 20; sleep .05; $V drag 400 300 40 6; sleep .7; grim $H/run/e1.png; pos
ok "drag: never expands" "$(near "$(px $H/run/e1.png 1000 30)" "$WIN")" same
ok "drag: zone released" "$(winy)" 12
$H/isl; sleep .3; $V move 1500 900; sleep .5; grim $H/run/e3.png
ok "restart keeps position" "$(px $H/run/e3.png $X $((Y+1)) opaque)" yes

$V move $X $((Y+16)); sleep .05; $V drag -300 650 30 6; sleep .7; pos; echo "     now at $X,$Y"
$V move $X $((Y+16)); sleep 1; grim $H/run/e5.png
ok "low on screen: panel opens upward" "$(px $H/run/e5.png $X $((Y-120)) opaque)" yes
$V move 300 500; sleep .9; grim $H/run/e5b.png
ok "and closes" "$(near "$(px $H/run/e5b.png $X $((Y-120)))" "$WIN")" same

$V move $X $((Y+16)); sleep .05; $V drag $((960-X+20)) $((6-Y+10)) 40 6; sleep 1.4; grim $H/run/e6.png
ok "drop near home: docks" "$(px $H/run/e6.png 960 7 opaque)" yes
ok "docked: zone returns" "$(winy)" 52
ok "docked: no stored position" "$(grep -c pos $S)" 0

$V move 960 20; sleep .05; $V drag 500 400 30 5; sleep .7; pos
$V dbl $((X+40)) $((Y+16)); sleep 1.3
ok "double click: home" "$(winy) $(grep -c pos $S)" "52 0"
ok "double click: not pinned" "$(grep -c pinned $S)" 0

# Drag from the right edge: pill must stay on screen
$V move 960 20; sleep .05; $V drag 1500 100 30 5; sleep .7; pos; echo "     clamped to $X,$Y"
ok "edge: clamped on screen" "$(python3 -c "print($X<=1920-60)")" True
$V dbl $((X+40)) $((Y+16)); sleep 1.3

$V click 1000 22 m; sleep .1; $V move 1500 900; sleep .9; grim $H/run/e8.png
ok "tuck: pill gone, sliver left" "$(px $H/run/e8.png 1000 20) | $(grep -c tucked $S)" "$WIN | 1"
ok "tuck: zone released" "$(winy)" 12
$V move 960 4; sleep 1; grim $H/run/e9.png
ok "tuck: hover opens panel" "$(px $H/run/e9.png 1100 38 opaque)" yes
$V move 1500 900; sleep .9; grim $H/run/e10.png
ok "tuck: leaving returns to sliver" "$(px $H/run/e10.png 1000 20)" "$WIN"
$H/isl; sleep .3; $V move 1500 900; sleep .6
ok "tuck: persists" "$(winy)" 12
$V move 960 4; sleep 1; $V click 1173 38; sleep .1; $V move 1500 900; sleep .9
ok "untuck from panel button" "$(winy) $(grep -c tucked $S)" "52 0"

$V move 960 22; sleep 1; $V click 1000 225; sleep .2; $V move 1500 900; sleep .9; grim $H/run/e11.png
ok "pin: background click keeps it open" "$(px $H/run/e11.png 1100 38 opaque) $(grep -c pinned $S)" "yes 1"
$V move 960 22; sleep .5; $V click 1000 225; sleep .2; $V move 1500 900; sleep .9; grim $H/run/e12.png
ok "unpin" "$(px $H/run/e12.png 1100 38) $(grep -c pinned $S)" "$BG 0"
echo "$(cat $XDG_RUNTIME_DIR/island.log | tail -3)"
