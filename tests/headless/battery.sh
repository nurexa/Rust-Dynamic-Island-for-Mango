#!/bin/bash
# A laptop: a fake UPower on a private system bus. Plugging, unplugging, low warnings.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
rm -f $R/sysbus; dbus-daemon --session --nofork --address=unix:path=$R/sysbus >/dev/null 2>&1 & echo $! > $R/sysbus.pid; sleep .3
echo '{"state":2,"pct":55,"empty":7800}' > $R/battery
python3 -I -W ignore $H/fakeupower.py $R/battery unix:path=$R/sysbus & echo $! > $R/upower.pid; sleep .5
notes(){ busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetCapabilities >/dev/null 2>&1; grep -c "$1" $R/island.log; }
ISLAND_SYSTEM_BUS=unix:path=$R/sysbus $H/isl; sleep 1
grim -g "560,0 800x60" $H/run/b1.png
# Something drawn there: the mean brightness of a small box around x,y.
light(){ magick "$1" -crop 40x16+$(($2-20))+$(($3-8)) -format "%[fx:mean>0.12?1:0]" info: | sed 's/1/light/;s/0/dark/'; }
ok "the pill shows the battery on a laptop" "$(light $H/run/b1.png 407 22)" light
$BIN pin; sleep 1.2; grim -g "560,0 800x520" $H/run/b2.png; $BIN pin; sleep 1
echo '{"state":1,"pct":56,"full":2700}' > $R/battery; kill -USR1 $(cat $R/upower.pid); sleep .4
grim -g "560,0 800x60" $H/run/b3.png
ok "plugging in flashes Charging" "$(light $H/run/b3.png 380 22)" light
echo '{"state":2,"pct":9,"empty":600}' > $R/battery; kill -USR1 $(cat $R/upower.pid); sleep 1
grim -g "560,0 800x160" $H/run/b4.png
ok "low battery warns" "$(light $H/run/b4.png 250 31)" light
echo '{"state":2,"pct":8,"empty":500}' > $R/battery; kill -USR1 $(cat $R/upower.pid); sleep 1
grim -g "560,0 800x160" $H/run/b5.png
kill -0 $(cat $R/island.pid) && echo "ok   alive through power changes"
kill $(cat $R/upower.pid) $(cat $R/sysbus.pid)
