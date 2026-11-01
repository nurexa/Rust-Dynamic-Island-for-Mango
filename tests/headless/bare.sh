#!/bin/bash
# Stands in for a compositor with no backend: Mango's IPC is hidden, so all the
# island has is what any wlr-layer-shell compositor would give it.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sorted(l['name'] for l in json.load(sys.stdin)['layers']))"; }
$H/isl stop; rm -f $R/island/state
S=$MANGO_INSTANCE_SIGNATURE; unset MANGO_INSTANCE_SIGNATURE
XDG_STATE_HOME=$R nohup setsid env -u MANGO_INSTANCE_SIGNATURE ${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)} > $R/island.log 2>&1 &
echo $! > $R/island.pid; sleep 1
export MANGO_INSTANCE_SIGNATURE=$S
ok "starts with no compositor backend" "$(layers)" "['island']"
grep -q 'no supported compositor' $R/island.log && echo "ok   says why workspaces are empty" || echo "FAIL says why workspaces are empty"
$V move 1500 900; sleep .5; $V move 960 22; sleep 1.2; grim $H/run/b1.png
ok "panel still opens" "$(px $H/run/b1.png 690 30 opaque)" yes
$V click 1207 38; sleep .1; $V move 1500 900; sleep .9; grim $H/run/b2.png
ok "pin still works" "$(px $H/run/b2.png 690 30 opaque)" yes
cp $H/run/b1.png $H/run/bare.png
kill $(cat $R/island.pid)
