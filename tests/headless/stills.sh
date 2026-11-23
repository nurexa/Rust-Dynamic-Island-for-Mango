#!/bin/bash
# Writes docs/styles/{pill,frame,notch,bar,glass,split,minimal}.jpg (960x540) in a private headless
# Mango, set up like demo.sh: the site's azure wallpaper still, a matugen palette, neutral
# windows, a mock player, nothing personal (the island is USER=you HOSTNAME=island, with its own
# HOME, config, cache and history). Needs ffmpeg, awww, matugen, grim, kitty.
H=${ISLAND_HEADLESS:-$(dirname "$(readlink -f "$0")")}
ROOT=$(readlink -f "$H/../..")
$H/fresh.sh 1 >/dev/null; rm -f $H/run/island/state
source $H/run/env; R=$XDG_RUNTIME_DIR
stop(){ for f in $R/*pid; do for p in $(cat "$f"); do grep -qzxF "XDG_RUNTIME_DIR=$R" /proc/$p/environ 2>/dev/null && kill $p; done; done; }

# The wallpaper: the still the site already publishes, never anything from your own cache.
B=azure-horizon.1920x1080.mp4
mkdir -p $R/cfg/island
ffmpeg -nostdin -v error -y -i $ROOT/docs/wall/azure.jpg -vf scale=1920:1080:flags=lanczos $R/$B.png
cat > $R/matugen.toml <<E
[config]
[templates.island]
input_path = "$HOME/.config/theme/templates/colors.toml.tmpl"
output_path = "$R/pal.toml"
E
matugen -c $R/matugen.toml image $R/$B.png --prefer saturation --mode dark -q
echo 70 > $R/bright
cat > $R/cfg/island/config.toml <<E
palette = "$R/pal.toml"
[commands]
brightness_get = "cat $R/bright"
brightness_set = "echo {} > $R/bright"
E
export XDG_CONFIG_HOME=$R/cfg

# Neutral windows: tags 1 and 3 get a terminal and a player, tag 2 (the one shown) stays empty.
mmsg get all-clients | python3 -c "import json,sys;[print(c['id']) for c in json.load(sys.stdin)['clients']]" > $R/ids
while read -r id; do mmsg dispatch "killclient client,$id" >/dev/null; done < $R/ids
sleep .6
win(){ mmsg dispatch "view,$1,0" >/dev/null; sleep .2; (nohup setsid kitty -c $R/kc/k.conf --class "$2" -e sh -c "$3" >/dev/null 2>&1 &); sleep .7; }
win 1 kitty "sleep 999"
win 3 spotify "sleep 999"
mmsg dispatch "view,2,0" >/dev/null

# awww with a cache of its own: on yours it would restore, and show, your wallpaper.
export XDG_CACHE_HOME=$R/cache XDG_DATA_HOME=$R/data
mkdir -p $XDG_CACHE_HOME $XDG_DATA_HOME
nohup setsid awww-daemon --no-cache >/dev/null 2>&1 & echo $! > $R/awww.pid
sleep .5; awww img $R/$B.png --transition-type none >/dev/null 2>&1

export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent
$H/full.sh
BIN=${ISLAND_BIN:-$ROOT/target/release/island}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
V=$H/vp
sleep 1; $V move 1500 900; sleep 1

# still NAME FILTER: grab the screen and write a 960x540 JPEG.
still(){ sleep 1; grim $R/still.png
  ffmpeg -nostdin -v error -y -i $R/still.png -vf "$2" -q:v 4 "$ROOT/docs/styles/$1.jpg"; }
FULL="scale=960:540:flags=lanczos"
TOP="crop=1440:810:240:0,scale=960:540:flags=lanczos"   # the top middle, for a panel on its own
PILL="crop=960:540:480:0"                                # the pill itself, at full size
open(){ $BIN style $1; sleep 2; $BIN pin; sleep 3; }    # pinned: the panel stays out for the shot
shut(){ $BIN pin; sleep 1.5; }

open pill;  still pill  "$TOP";  shut
open frame; still frame "$FULL"; shut
open notch; still notch "$TOP";  shut
open bar;   still bar   "$FULL"; shut
open glass; still glass "$FULL"; shut
$BIN style split;   sleep 3; $V move 1500 900; still split   "$PILL"
$BIN style minimal; sleep 3; $V move 1500 900; still minimal "$PILL"
$BIN style pill
stop
ls -la --block-size=K "$ROOT"/docs/styles/*.jpg | awk '{print $5, $9}'
