#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
export ISLAND_BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)} DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
rm -f $R/island/state; $H/isl; sleep .5; P=$(cat $R/island.pid)
m(){ echo "$1: anon=$(awk '/RssAnon/{print $2}' /proc/$P/status) rss=$(awk '/VmRSS/{print $2}' /proc/$P/status) fds=$(ls /proc/$P/fd | wc -l) threads=$(awk '/Threads/{print $2}' /proc/$P/status)"; }
cycle(){ for i in $(seq $1); do $V move 960 22; sleep .42; mmsg dispatch "view,$((i%2*2+1)),0" >/dev/null; $V move 1500 900; sleep .42; done; }
m start
cycle 10; sleep 2; m "after 10"
cycle 50; sleep 2; m "after 60"
cycle 50; sleep 2; m "after 110"
