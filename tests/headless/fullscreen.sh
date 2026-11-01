#!/bin/bash
# Frame callbacks stop while a fullscreen window covers the island. Whatever
# changed meanwhile has to appear when it's uncovered, with no stall.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
BG="42 74 90"
rm -f $R/island/state; $H/isl; $V move 1500 900; sleep .6
mmsg dispatch togglefullscreen >/dev/null; sleep .8
grim $H/run/n1.png
ok "covered: pill hidden" "$(px $H/run/n1.png 960 8)" "15 20 23"
$BIN pin; sleep .4
mmsg dispatch togglefullscreen >/dev/null; sleep 1.4
grim $H/run/n2.png
ok "uncovered: a pin made while covered shows up" "$(px $H/run/n2.png 690 30 opaque)" yes
$BIN pin; sleep 1.2; grim $H/run/n3.png
ok "and it closes again, so frames resumed" "$(px $H/run/n3.png 690 30)" "$BG"
kill -0 $(cat $R/island.pid) && echo "ok   alive"
