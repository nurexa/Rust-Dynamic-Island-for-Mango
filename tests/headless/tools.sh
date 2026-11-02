#!/bin/bash
# Every palette tool, driven by a virtual keyboard. systemctl and nmcli are shims, so
# nothing here can touch the real machine, and clipboard history is a private copy.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
export XDG_CACHE_HOME=$R/cache XDG_DATA_HOME=$R/data XDG_CONFIG_HOME=$R/cfgt
mkdir -p $XDG_CACHE_HOME $XDG_DATA_HOME/applications $XDG_CONFIG_HOME/island $R/walls
LOG=$R/shim.log; : > $LOG
layer(){ mmsg get all-layers | python3 -c "import json,sys;print([l['layer'] for l in json.load(sys.stdin)['layers'] if l['name']=='island'][0])"; }
focused(){ mmsg get focusing-client | python3 -c "import json,sys;print(json.load(sys.stdin).get('appid'))"; }
tags(){ mmsg get all-monitors | python3 -c "import json,sys;print(json.load(sys.stdin)['monitors'][0]['active_tags'])"; }
logged(){ grep -c -- "$1" $LOG; }

printf '#!/bin/sh\necho "islandtest $*" >> %s\n' "$LOG" > $H/shim/islandtest; chmod +x $H/shim/islandtest
printf '[Desktop Entry]\nType=Application\nName=Zztest\nExec=islandtest launched %%U\nIcon=zztest\n' > $XDG_DATA_HOME/applications/zztest.desktop
ffmpeg -nostdin -v error -y -f lavfi -i color=c=red:s=64x64 -frames:v 1 "$R/walls/it's red.png"
cat > $XDG_CONFIG_HOME/island/config.toml <<E
screenshot_dir = "$R/shots"
[commands]
lock = "islandtest lock"
[wallpaper]
dir = "$R/walls"
set = "islandtest wallpaper {path}"
[[tool]]
name = "fruit"
list = "printf 'apple\\\\nbanana\\\\n'"
run = "islandtest fruit {}"
E
sed -i 's/^tools = .*//' $XDG_CONFIG_HOME/island/config.toml
sed -i '1i tools = ["apps", "run", "windows", "clipboard", "screenshot", "wifi", "wallpaper", "power", "fruit"]' $XDG_CONFIG_HOME/island/config.toml
$H/isl; sleep .3; $V move 1500 900; sleep .4
ok "starts on the top layer" "$(layer)" top

# Keyboard focus is only held while open, and goes back where it was
$BIN open apps; sleep 1
ok "palette: raised to overlay" "$(layer)" overlay
$V key Escape; sleep .9
ok "palette: Escape closes, layer back to top" "$(layer)" top
ok "palette: window focus comes back" "$(focused)" kitty
$BIN open apps; sleep .6; $BIN open apps; sleep .9
ok "palette: the same key toggles it shut" "$(layer)" top

# Apps
$BIN open apps; sleep .9; $V type zzt; sleep .3; $V key Return; sleep .8
ok "apps: Enter launches, field codes removed" "$(grep -c '^islandtest launched$' $LOG)" 1
ok "apps: launch counted" "$(grep -c '1 zztest' $XDG_STATE_HOME/island/launches 2>/dev/null || grep -c '1 zztest' $R/island/launches)" 1

# Run
$BIN open run; sleep .9; $V type islandtest typed here; sleep .3; $V key Return; sleep .7
ok "run: Enter runs what was typed" "$(logged 'islandtest typed here')" 1

# Windows
$BIN open windows; sleep .9; $V type pav; sleep .3; $V key Return; sleep .8
ok "windows: jumps to another workspace" "$(focused) $(tags)" "org.pulseaudio.pavucontrol [3]"
mmsg dispatch view,1,0 >/dev/null; sleep .5

# Clipboard: content a stranger chose, which must stay inert
printf 'plain note' | cliphist store
printf 'evil $(islandtest pwned) `islandtest pwned2`' | cliphist store
$BIN open clipboard; sleep 1; $V type evil; sleep .3; $V key Return; sleep .9
ok "clipboard: restores the exact text" "$(wl-paste -n)" 'evil $(islandtest pwned) `islandtest pwned2`'
ok "clipboard: hostile content never ran" "$(logged pwned)" 0

# Screenshots
$BIN shot output; sleep 1.6
ok "screenshot: saved as a PNG" "$(file -b --mime-type $R/shots/*.png 2>/dev/null | head -1)" image/png
ok "screenshot: copied to the clipboard" "$(wl-paste -l | grep -c image/png)" 1
$BIN shot window; sleep 1.5
ok "screenshot: window shot saved too" "$(ls $R/shots | wc -l)" 2
sleep 5.5

# Power: nothing destructive on one Enter
$BIN open power; sleep .9; $V type reboot; sleep .3; $V key Return; sleep .4
ok "power: one Enter doesn't reboot" "$(logged 'systemctl reboot')" 0
grim $H/run/t1.png
$V key Return; sleep .9
ok "power: the second Enter does" "$(logged 'systemctl reboot')" 1
$BIN open power; sleep .9; $V type shut; sleep .3; $V key Return; sleep .3; $V key BackSpace; sleep .2; $V key Return; sleep .4; $V key Escape; sleep .8
ok "power: editing the search cancels the confirmation" "$(logged 'systemctl poweroff')" 0
$BIN open power; sleep .9; $V type lock; sleep .3; $V key Return; sleep .8
ok "power: lock uses the configured command" "$(logged 'islandtest lock')" 1

# Wi-Fi
$BIN open wifi; sleep 1; $V type cafe; sleep .3; $V key Return; sleep .4; $V type secret1; sleep .3; grim $H/run/t2.png; $V key Return; sleep .9
ok "wifi: the password goes in on stdin" "$(cat $R/wifi.password 2>/dev/null)" secret1
ok "wifi: and never into argv" "$(logged secret1)" 0

# Wallpaper: filename with a quote in it
$BIN open wallpaper; sleep .9; $V type red; sleep .3; grim $H/run/t3.png; $V key Return; sleep .8
ok "wallpaper: runs the set command with the path intact" "$(logged "islandtest wallpaper $R/walls/it's red.png")" 1

# A tool made of two shell commands
$BIN open fruit; sleep .9; $V type ban; sleep .3; $V key Return; sleep .8
ok "custom tool: list and run from config" "$(logged 'islandtest fruit banana')" 1

# Tab moves between tools
$BIN open apps; sleep .8; $V key Tab; sleep .5; grim $H/run/t4.png; $V key shift+Tab; sleep .4; $V key Escape; sleep .8

# Scripts written for rofi
printf 'one\ntwo\nthree\n' | $BIN dmenu -i -p Pick > $R/dm1 & D=$!
sleep .8; $V key Down; sleep .2; $V key Return; wait $D
ok "dmenu: prints the chosen line" "$? $(cat $R/dm1)" "0 two"
printf 'one\ntwo\nthree\n' | $BIN dmenu -format i -p Pick > $R/dm2 & D=$!
sleep .8; $V key Down Down; sleep .2; $V key Return; wait $D
ok "dmenu: -format i prints the index" "$(cat $R/dm2)" 2
printf 'one\n' | $BIN dmenu > $R/dm3 & D=$!
sleep .8; $V key Escape; wait $D
ok "dmenu: cancel exits 1 and prints nothing" "$? $(wc -c < $R/dm3)" "1 0"
$BIN dmenu -password -p Secret > $R/dm4 & D=$!
sleep .8; $V type hunter2; sleep .2; $V key Return; wait $D
ok "dmenu: -password returns what was typed" "$(cat $R/dm4)" hunter2
sleep .6

# Above fullscreen, unlike the pill
mmsg dispatch togglefullscreen >/dev/null; sleep .8
$BIN open apps; sleep 1.1; grim $H/run/t5.png
ok "palette: shows over a fullscreen window" "$(px $H/run/t5.png 690 30 opaque)" yes
$V key Escape; sleep .9; grim $H/run/t6.png
ok "palette: gone again, back under it" "$(px $H/run/t6.png 690 30)" "15 20 23"
mmsg dispatch togglefullscreen >/dev/null; sleep .6

kill -0 $(cat $R/island.pid) && echo "ok   alive after every tool"
grep -vE 'no supported|another notification' $R/island.log | tail -3
rm -f $H/shim/islandtest
