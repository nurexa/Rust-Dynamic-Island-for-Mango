#!/bin/bash
# Private bus + mock apps + shimmed audio, inside the headless session.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; R=$XDG_RUNTIME_DIR
[ -f $R/art.jpg ] || ffmpeg -nostdin -v error -y -f lavfi -i "gradients=s=600x400:c0=0x8a2be2:c1=0xff7f50:x0=0:y0=0:x1=600:y1=400:n=2" -frames:v 1 $R/art.jpg
echo '{"mute":false,"vol":48}' > $R/sink; : > $R/events; : > $R/audio.log
[ -f $R/dbus.pid ] && kill $(cat $R/dbus.pid) 2>/dev/null
dbus-daemon --session --print-address=1 --nofork --address=unix:path=$R/bus > $R/dbus.addr 2>/dev/null &
echo $! > $R/dbus.pid; sleep .3
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
nohup setsid playerctld > /dev/null 2>&1 & echo $! > $R/pctl.pid
sleep .3; $H/isl; sleep .3
(nohup setsid $H/../../target/release/examples/mock $H/run/art.jpg > $R/mock.log 2>&1 & echo $! > $R/mock.pid)
sleep 1.2
