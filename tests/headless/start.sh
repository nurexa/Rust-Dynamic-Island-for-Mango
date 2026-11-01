#!/bin/bash
H=$(dirname "$(readlink -f "$0")")
# Headless mango for tests. DBUS_SESSION_BUS_ADDRESS must stay unset: see memory note.
N=${1:-1}
R=$H/run; rm -rf $R; mkdir -p $R; chmod 700 $R
env -u DBUS_SESSION_BUS_ADDRESS -u WAYLAND_DISPLAY -u DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE -u MANGO_INSTANCE_SIGNATURE \
  XDG_RUNTIME_DIR=$R WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_HEADLESS_OUTPUTS=$N WLR_LIBINPUT_NO_DEVICES=1 \
  setsid mango -c $H/mango.conf >$R/mango.log 2>&1 &
echo $! > $R/pid
for i in $(seq 50); do s=$(ls $R/mango-*.sock 2>/dev/null|head -1); [ -n "$s" ] && [ -S "$R/wayland-0" -o -S "$R/wayland-1" ] && break; sleep 0.1; done
w=$(cd $R; ls wayland-? | head -1)
cat > $R/env <<E
export XDG_RUNTIME_DIR=$R WAYLAND_DISPLAY=$w MANGO_INSTANCE_SIGNATURE=$s
E
echo up $w $s
