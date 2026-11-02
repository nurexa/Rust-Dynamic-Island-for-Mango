#!/bin/bash
# The notification daemon, tested with the real notify-send on a private bus.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
WIN="15 20 23"
owner(){ busctl --user status org.freedesktop.Notifications 2>/dev/null | awk -F= '/^Comm=/{print $2}'; }
# The card's left padding: plain background, wherever the text falls.
open(){ px $H/run/$1.png 757 40 opaque; }

# Taking over from a daemon that was here first
$H/isl stop; sed 's/StatusNotifierWatcher/Notifications/; s/org.kde.Notifications/org.freedesktop.Notifications/' $H/fakew.py > $R/fakenotify.py
nohup setsid python3 -I $R/fakenotify.py >/dev/null 2>&1 & F=$!; sleep .6
$H/isl; sleep .4
ok "leaves another daemon alone at startup" "$(owner)" python3
kill $F; sleep 1
ok "takes over when it stops" "$(owner)" island
$V move 1500 900; sleep .4

notify-send -t 1500 "Hello" "Plain toast"; sleep .9; grim $H/run/n1.png
ok "toast: the pill grows into a card" "$(open n1)" yes
sleep 1.8; grim $H/run/n2.png
ok "toast: expires on the app's timeout" "$(px $H/run/n2.png 757 40)" "$WIN"

# Hover holds it, and it still leaves afterwards
notify-send -t 800 "Stay" "Hovered"; sleep .3; $V move 900 30; sleep 1.6; grim $H/run/n3.png
ok "toast: hovering keeps it up" "$([ "$(px $H/run/n3.png 757 40)" != "$WIN" ] && echo yes)" yes
$V move 1500 900; sleep 2.5; grim $H/run/n4.png
ok "toast: goes once the pointer leaves" "$(px $H/run/n4.png 757 40)" "$WIN"

# Actions reach the app
notify-send --action=reply=Reply --wait "Message" "Answer me" > $R/act & N=$!
sleep .9; grim $H/run/n5.png; $V click 790 86; wait $N
ok "actions: the button reaches the app" "$(cat $R/act)" reply
$V move 1500 900; sleep .9

# Replacing in place
id=$(notify-send -p -t 3000 "Copying" "10%"); sleep .5
notify-send -r "$id" -t 900 "Copying" "90%"; sleep .4; grim $H/run/n6.png
ok "replace: updates the card without a second one" "$(open n6)" yes
sleep 1.4

# An app closing its own
id=$(notify-send -p -t 0 "Sticky" "Until closed"); sleep .9
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications CloseNotification u "$id"; sleep .9; grim $H/run/n7.png
ok "close: an app can take its own back" "$(px $H/run/n7.png 757 40)" "$WIN"

# Critical stays, and shows over fullscreen
mmsg dispatch togglefullscreen >/dev/null; sleep .8
notify-send -t 0 "Quiet" "Normal stays under the game"; sleep .9; grim $H/run/n8.png
ok "fullscreen: normal notifications stay out of the way" "$(px $H/run/n8.png 757 40)" "$WIN"
sleep .2; mmsg dispatch togglefullscreen >/dev/null; sleep .9
$V click 900 26; sleep .9
mmsg dispatch togglefullscreen >/dev/null; sleep .8
notify-send -u critical "Battery" "Critically low"; sleep 1.1; grim $H/run/n9.png
ok "critical: shows over fullscreen" "$(open n9)" yes
sleep 6; grim $H/run/n10.png
ok "critical: never times out" "$(open n10)" yes
$V move 950 30; sleep .2; $V click 950 30; sleep 1; $V move 1500 900; sleep .5; grim $H/run/n11.png
ok "critical: a click dismisses it" "$(px $H/run/n11.png 757 40)" "$WIN"
mmsg dispatch togglefullscreen >/dev/null; sleep .6

# Do not disturb
$BIN dnd; sleep .3; notify-send "Silent" "Goes to history only"; sleep 1; grim $H/run/n12.png
ok "dnd: no toast" "$(px $H/run/n12.png 757 40)" "$WIN"
$BIN dnd; sleep .3
$V move 960 22; sleep 1.2; grim $H/run/n13.png; $V move 1500 900; sleep .9
tail -3 $R/island.log | grep -v 'another notification'
kill -0 $(cat $R/island.pid) && echo "ok   alive after every notification"
