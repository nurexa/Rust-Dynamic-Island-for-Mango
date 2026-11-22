#!/bin/bash
# Fresh headless mango with quiet windows: kitty on tag 1, pavucontrol on tag 3.
H=$(dirname "$(readlink -f "$0")")
[ -f $H/run/env ] && source $H/run/env
# island, pointer, bus, player, mock, compositor: everything the last run started.
# A stale pid file can name someone else's process by now: only kill what carries this runtime dir.
for f in $H/run/*pid; do
  [ -f "$f" ] || continue
  for p in $(cat "$f"); do
    grep -qzxF "XDG_RUNTIME_DIR=$H/run" "/proc/$p/environ" 2>/dev/null && kill "$p"
  done
done
sleep .5
$H/start.sh ${1:-1} || exit 1; source $H/run/env; VP_W=$((1920+1280*(${1:-1}-1))) $H/tui.sh
mkdir -p $XDG_RUNTIME_DIR/kc; printf 'background #0f1417\nconfirm_os_window_close 0\nallow_remote_control no\nclipboard_control no-append\n' > $XDG_RUNTIME_DIR/kc/k.conf
# A window opens on whatever workspace is showing by then, so wait for each one.
opened(){ for i in $(seq 200); do mmsg get all-clients | grep -q "\"appid\":\"$1\"" && return; sleep .1; done; echo "FAIL fixture: $1 never opened" >&2; }
(nohup setsid kitty -c $XDG_RUNTIME_DIR/kc/k.conf -e sleep 9999 >/dev/null 2>&1 &)
opened kitty; sleep 1
mmsg dispatch "view,3,0" >/dev/null; sleep .2; (nohup setsid pavucontrol >/dev/null 2>&1 &)
opened org.pulseaudio.pavucontrol; sleep 1.2; mmsg dispatch "view,1,0" >/dev/null; sleep .4
# The island's config for the session: never yours, and the panel the click tests
# were measured on. Tests of newer rows (bluetooth.sh, audio.sh...) set their own.
mkdir -p $XDG_RUNTIME_DIR/xdg/island
echo 'panel = ["header", "workspaces", "media", "notifications", "stats", "audio", "brightness", "status"]' > $XDG_RUNTIME_DIR/xdg/island/config.toml
