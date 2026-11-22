#!/bin/bash
# Fresh headless mango with quiet windows: kitty on tag 1, pavucontrol on tag 3.
H=$(dirname "$(readlink -f "$0")")
[ -f $H/run/env ] && source $H/run/env
# island, pointer, bus, player, mock, compositor: everything the last run started.
for f in $H/run/*pid; do [ -f "$f" ] && kill $(cat "$f") 2>/dev/null; done
sleep .5
$H/start.sh ${1:-1} || exit 1; source $H/run/env; VP_W=$((1920+1280*(${1:-1}-1))) $H/tui.sh
mkdir -p $XDG_RUNTIME_DIR/kc; printf 'background #0f1417\nconfirm_os_window_close 0\nallow_remote_control no\nclipboard_control no-append\n' > $XDG_RUNTIME_DIR/kc/k.conf
(nohup setsid kitty -c $XDG_RUNTIME_DIR/kc/k.conf -e sleep 9999 >/dev/null 2>&1 &)
sleep 1
mmsg dispatch "view,3,0" >/dev/null; sleep .2; (nohup setsid pavucontrol >/dev/null 2>&1 &); sleep 1.2; mmsg dispatch "view,1,0" >/dev/null; sleep .4
# The island's config for the session: never yours, and the panel the click tests
# were measured on. Tests of newer rows (bluetooth.sh, audio.sh...) set their own.
mkdir -p $XDG_RUNTIME_DIR/xdg/island
echo 'panel = ["header", "workspaces", "media", "notifications", "stats", "audio", "brightness", "status"]' > $XDG_RUNTIME_DIR/xdg/island/config.toml
