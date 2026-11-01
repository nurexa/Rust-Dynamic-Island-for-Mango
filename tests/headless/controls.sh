#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
vol(){ python3 -c "import json;d=json.load(open('$R/sink'));print(d['vol'],d['mute'])"; }
last(){ tail -1 $R/mock.log; }
px(){ python3 $H/px.py "$@"; }
$V move 960 22; sleep 1.2
$V click 1118 166; sleep .4; ok "media: previous" "$(last)" "media: previous"
$V click 1203 166; sleep .4; ok "media: next" "$(last)" "media: next"
$V click 1160 166; sleep .4; ok "media: play-pause" "$(last)" "media: play-pause"
sleep .4; grim $H/run/h1.png; magick $H/run/h1.png -crop 640x360+640+0 $H/run/h1s.png
$V click 1160 166; sleep .5
$V click 1045 297; sleep .5; ok "slider click sets volume" "$(vol)" "50 False"
$V click 708 297; sleep .5; ok "mute button" "$(vol)" "50 True"
$V click 708 297; sleep .5; ok "unmute" "$(vol)" "50 False"
$V move 1100 297; sleep .15; $V scroll -1; sleep .5; ok "scroll on slider: +5" "$(vol)" "55 False"
$V scroll 1; sleep .5; ok "scroll on slider: -5" "$(vol)" "50 False"
# drag along the slider without moving the island
$V move 1000 297; sleep .1; $V down; sleep .05; $V path 1000 297 1110 297 12 8; $V up; sleep .5
echo "     slider drag -> $(vol), last cmd: $(tail -1 $R/audio.log)"
ok "slider drag did not undock" "$(grep -c pos $R/island/state 2>/dev/null || echo 0)" 0
# tray
$V click 1205 329; sleep .4; ok "tray left: Activate" "$(last)" "tray: activate"
$V click 1205 329 m; sleep .4; ok "tray middle: SecondaryActivate" "$(last)" "tray: secondary"
$V click 1205 329 r; sleep .7; grim $H/run/h2.png; magick $H/run/h2.png -crop 640x300+640+0 $H/run/h2s.png
$V move 1000 200
# menu: submenu, back, click (mock logs the event)
$V click 900 117; sleep .6; grim $H/run/h3.png; magick $H/run/h3.png -crop 640x200+640+0 $H/run/h3s.png
$V click 710 36; sleep .6; grim $H/run/h4.png
ok "menu back: from submenu to root" "$(px $H/run/h4.png 780 148 opaque)" "yes"
$V click 900 76; sleep .5; ok "menu item fires dbusmenu Event" "$(last)" "menu: clicked 1"
sleep .5; grim $H/run/h5.png
ok "menu click returns to main view" "$(px $H/run/h5.png 1160 166 opaque)" "no"
# compact flash on volume change
$V move 1500 900; sleep 1; echo "Event 'change' on server" >> /dev/null
$H/shim/wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 30%; sleep .6; grim $H/run/h6.png; magick $H/run/h6.png -crop 400x44+760+0 $H/run/h6s.png
sleep 1.6; grim $H/run/h7.png; magick $H/run/h7.png -crop 400x44+760+0 $H/run/h7s.png
# media gone
kill $(cat $R/mock.pid); sleep 1.2; grim $H/run/h8.png; magick $H/run/h8.png -crop 400x44+760+0 $H/run/h8s.png
$V move 960 22; sleep 1.2; grim $H/run/h9.png; magick $H/run/h9.png -crop 640x330+640+0 $H/run/h9s.png
echo "     items after app quit: $(busctl --user get-property org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems)"
$V move 1500 900
