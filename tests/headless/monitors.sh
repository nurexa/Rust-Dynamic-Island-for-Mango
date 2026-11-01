#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
mons(){ mmsg get all-monitors | python3 -c "import json,sys;print(' '.join(f\"{m['name']}{'*' if m['active'] else ''}:{m['active_tags']}\" for m in json.load(sys.stdin)['monitors']))"; }
rm -f $XDG_RUNTIME_DIR/island/state; $H/isl
mmsg dispatch focusmon,HEADLESS-2 >/dev/null; sleep .3; (nohup setsid kitty -c $XDG_RUNTIME_DIR/kc/k.conf -e sleep 9999 >/dev/null 2>&1 &); sleep 1.2
$V move 1500 900; sleep .3; mmsg dispatch focusmon,HEADLESS-1 >/dev/null; sleep .4; $V move 1500 900; sleep .3
$V move 960 22; sleep 1.2; grim $H/run/m1.png
ok "hover leaves focus alone" "$(mons)" "HEADLESS-1*:[1] HEADLESS-2:[1]"
ok "panel lists both monitors" "$(python3 $H/px.py $H/run/m1.png 730 176 opaque)" no
$V click 786 176; sleep .7
ok "chip on the other monitor switches and focuses that monitor" "$(mons)" "HEADLESS-1:[1] HEADLESS-2*:[2]"
$V move 2560 22; sleep 1.2; grim $H/run/m2.png
ok "pointer on monitor 2 opens that island" "$(python3 $H/px.py $H/run/m2.png 2292 60 opaque)" yes
ok "monitor 1's island closed" "$(python3 $H/px.py $H/run/m2.png 750 30)" "15 20 23"
