#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sorted(l['monitor'] for l in json.load(sys.stdin)['layers'] if l['name']=='island'))"; }
alive(){ kill -0 $(cat $XDG_RUNTIME_DIR/island.pid) 2>/dev/null && echo alive || echo dead; }
rm -f $XDG_RUNTIME_DIR/island/state; $H/isl
ok "one output" "$(layers)" "['HEADLESS-1']"
mmsg dispatch create_virtual_output >/dev/null; sleep 1
ok "plug in: second island" "$(layers)" "['HEADLESS-1', 'HEADLESS-2']"
wlr-randr --output HEADLESS-2 --off; sleep 1
ok "output off: its island goes, the other stays" "$(layers) $(alive)" "['HEADLESS-1'] alive"
wlr-randr --output HEADLESS-2 --on; sleep 1
ok "output back on: island returns" "$(layers) $(alive)" "['HEADLESS-1', 'HEADLESS-2'] alive"
for i in 1 2 3 4; do wlr-randr --output HEADLESS-2 --off; sleep .25; wlr-randr --output HEADLESS-2 --on; sleep .25; done; sleep .8
ok "rapid off/on" "$(layers) $(alive)" "['HEADLESS-1', 'HEADLESS-2'] alive"
mmsg dispatch destroy_all_virtual_output >/dev/null; sleep 1
ok "all outputs gone: no crash" "$(layers) $(alive)" "[] alive"
mmsg dispatch create_virtual_output >/dev/null; sleep 1.2
mmsg get all-layers
ok "first output after zero: island back" "$(alive)" alive
tail -3 $XDG_RUNTIME_DIR/island.log
