#!/bin/bash
# Open and close the palette and show notifications many times: anything that grows
# here grows for weeks on a real desktop.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
$H/isl; sleep .5; P=$(cat $R/island.pid)
sample(){ echo "$(awk '/^Pss_Anon/{print $2}' /proc/$P/smaps_rollup) $(ls /proc/$P/fd | wc -l) $(ls /proc/$P/task | wc -l)"; }
cycle(){ for i in $(seq $1); do
  $BIN open apps; sleep .25; $V type ki; sleep .05; $V key Escape; sleep .2
  notify-send -t 100 "Soak $i" "body"; sleep .35
done; }
cycle 40; sleep 2; read -r a0 f0 t0 < <(sample); echo "     after 40: anon=${a0}kB fds=$f0 threads=$t0"
cycle ${SOAK:-40}; sleep 2; read -r a1 f1 t1 < <(sample); echo "     after more: anon=${a1}kB fds=$f1 threads=$t1"
cycle ${SOAK:-40}; sleep 2; read -r a2 f2 t2 < <(sample); echo "     after more: anon=${a2}kB fds=$f2 threads=$t2"
ok "soak: no file descriptor growth" "$f2" "$f0"
ok "soak: no thread growth" "$t2" "$t0"
# The heap settles in a step early on; what matters is that it stops growing.
ok "soak: memory plateaus" "$(python3 -c "print($a2-$a1<1024)")" True
