#!/bin/bash
# The notification daemon, tested with the real notify-send on a private bus.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
WIN="40 8 48"
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
sleep .9; grim $H/run/n5.png
$V move 1100 96; sleep .4; grim $H/run/n5h.png
ok "hover: the card lights down beside its buttons, not just above them" "$([ "$(px $H/run/n5h.png 1100 96)" != "$(px $H/run/n5.png 1100 96)" ] && echo lit)" lit
$V click 790 86; wait $N
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

# Every signal the daemon sends, from here on
: > $R/sigs; nohup setsid python3 -I $H/sigs.py $R/sigs >/dev/null 2>&1 & echo $! > $R/sigs.pid; sleep .5
about(){ [ $(($1 - $2)) -ge -3 ] && [ $(($1 - $2)) -le 3 ] && echo $2 || echo $1; }

# A burst from one app shares one toast with a count, instead of queueing
for i in 1 2 3; do notify-send -a burst -t 1500 "Build $i" "Step $i"; done; sleep .5; grim $H/run/n14.png
ok "burst: one toast counts the rest" "$(magick $H/run/n14.png -crop 30x16+1100+23 -format '%[fx:maxima>0.5?1:0]' info:)" 1
sleep 2.2; grim $H/run/n15.png
ok "burst: gone on the newest one's timeout" "$(px $H/run/n15.png 757 40)" "$WIN"
ok "burst: the covered ones report expired" "$(grep -c 'NotificationClosed [0-9]* 1' $R/sigs)" 3

# Inline replies, the way KDE and GNOME apps ask for them
call(){ gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.$1 "${@:2}"; }
ask(){ call Notify chat 0 '' Ana "$1" "['inline-reply', 'Reply']" "{'x-kde-reply-placeholder-text': <'Answer Ana'>}" $2 | sed 's/.* \([0-9]*\),)/\1/'; }
ok "inline reply: advertised" "$(call GetCapabilities | grep -o inline-reply)" inline-reply
id=$(ask "Lunch at noon?" 8000); sleep .9
$V click 790 86; sleep .4; $V type hi there; $V key Return; sleep .6
ok "inline reply: Enter sends the text" "$(grep "NotificationReplied $id " $R/sigs)" "NotificationReplied $id hi there"
ok "inline reply: and closes it as dismissed" "$(grep "NotificationClosed $id " $R/sigs)" "NotificationClosed $id 2"
$V move 1500 900; sleep .9
id=$(ask "Still on?" 1500); sleep .9
$V click 790 86; sleep .3; $V type nope; $V key Escape; $V move 1500 900; sleep 2.4; grim $H/run/n16.png
ok "inline reply: Esc sends nothing" "$(grep -c "NotificationReplied $id " $R/sigs)" 0
ok "inline reply: and the toast times out again" "$(px $H/run/n16.png 757 40)" "$WIN"

# History groups by app, newest app first. Fresh history, and a panel of only that.
C=$R/cfg; mkdir -p $C/island; export XDG_CONFIG_HOME=$C
echo 'panel = ["notifications"]' > $C/island/config.toml
$H/isl; $BIN dnd; sleep .3; : > $R/sigs
notify-send -a mail Mail Inbox; for i in 1 2 3; do notify-send -a chat "Chat $i" "Message $i"; done
height(){ px $H/run/$1.png 686 60 height; }
$BIN pin; sleep 1.2; grim $H/run/n17.png
$V click 720 56; $V move 1500 900; sleep 1.2; grim $H/run/n18.png
ok "groups: the header shows the whole app" "$(about $(($(height n18) - $(height n17))) 80)" 80
$V click 720 56; $V move 1500 900; sleep 1.2; grim $H/run/n19.png
ok "groups: and collapses it to the newest" "$(about $(height n19) $(height n17))" "$(height n17)"
$V click 1213 56; $V move 1500 900; sleep 1.2; grim $H/run/n20.png
ok "groups: clearing one app leaves the others" "$(about $(($(height n17) - $(height n20))) 68)" 68
ok "groups: each cleared note reports dismissed" "$(grep -cE '^NotificationClosed [234] 2$' $R/sigs)" 3
id=$(ask "Dinner?" 0); sleep .6
$V click 1196 87; sleep .5; grim $H/run/n21.png
ok "history reply: the entry turns into a field" "$(px $H/run/n21.png 1100 134 | awk '{print ($1 < 14 ? "well" : "card")}')" well
$V type on my way; $V key Return; $V move 1500 900; sleep .6
ok "history reply: Enter sends it" "$(grep "NotificationReplied $id " $R/sigs)" "NotificationReplied $id on my way"
$BIN pin; $BIN dnd; kill $(cat $R/sigs.pid)
rm $C/island/config.toml; unset XDG_CONFIG_HOME; $BIN reload; sleep .9
# Switched while running. Nothing is sent while it's off: a regression would relay it to itself.
$BIN set notifications off; sleep .6
ok "switched off: the name is let go" "$(owner)" ""
nohup setsid python3 -I $R/fakenotify.py >/dev/null 2>&1 & F=$!; sleep .6
ok "switched off: another daemon can serve" "$(owner)" python3
kill $F; sleep .6
ok "switched off: not taken back" "$(owner)" ""
$H/isl; sleep .3
ok "switched off: holds through a restart" "$(owner)" ""
$BIN set notifications on; sleep .6
ok "switched on without a restart: serving" "$(owner) $(call GetServerInformation | cut -d"'" -f2)" "island island"
tail -3 $R/island.log | grep -v 'another notification'
kill -0 $(cat $R/island.pid) && echo "ok   alive after every notification"
