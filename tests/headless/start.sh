#!/bin/bash
H=$(dirname "$(readlink -f "$0")")
# Headless mango for tests. DBUS_SESSION_BUS_ADDRESS must stay unset: see memory note.
N=${1:-1}
if grep -qF " $H/run/" /proc/self/mountinfo; then
  echo "something is still mounted under $H/run: not clearing it" >&2; exit 1
fi
R=$H/run;rm -rf $R; mkdir -p $R; chmod 700 $R
env -u DBUS_SESSION_BUS_ADDRESS -u WAYLAND_DISPLAY -u DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE -u MANGO_INSTANCE_SIGNATURE \
  XDG_RUNTIME_DIR=$R WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_HEADLESS_OUTPUTS=$N WLR_LIBINPUT_NO_DEVICES=1 \
  setsid mango -c $H/mango.conf >$R/mango.log 2>&1 &
echo $! > $R/pid
for i in $(seq 50); do s=$(ls $R/mango-*.sock 2>/dev/null|head -1); [ -n "$s" ] && [ -S "$R/wayland-0" -o -S "$R/wayland-1" ] && break; sleep 0.1; done
w=$(cd $R; ls wayland-? 2>/dev/null | head -1)
if [ -z "$w" ] || ! kill -0 "$(cat $R/pid)" 2>/dev/null; then
  echo "FAIL fixture: mango didn't start" >&2; tail -5 $R/mango.log >&2; exit 1
fi
cat > $R/env <<E
export XDG_RUNTIME_DIR=$R WAYLAND_DISPLAY=$w MANGO_INSTANCE_SIGNATURE=$s
export XDG_STATE_HOME=$R
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent
unset DISPLAY
E
# A bus of its own from the first script on: on yours, a test click reaches your player and tray.
# After mango, which must never see one. Nothing restarts it: kitty exits when its bus does.
(source $R/env; exec dbus-daemon --config-file=$H/bus.conf --nofork --address=unix:path=$R/bus) >/dev/null 2>&1 &
echo $! > $R/dbus.pid
for i in $(seq 50); do [ -S $R/bus ] && break; sleep 0.1; done
echo up $w $s
