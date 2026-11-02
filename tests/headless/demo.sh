#!/bin/bash
# Records docs/demo.gif and docs/demo.mp4 in a private headless Mango: the owner's wallpapers
# (stills from ~/.cache/wallpapers) and matugen palette, the real app list, but neutral window
# titles, a mock player, sample clipboard text and no network name, so nothing personal ends up
# in it. Animations run at a tenth of real speed and the capture is sped back up, because grim
# is far slower than the spring. Needs ffmpeg, awww and matugen.
H=$(dirname "$(readlink -f "$0")")
ROOT=$(readlink -f "$H/../..")
$H/fresh.sh 1 >/dev/null; rm -f $H/run/island/state
source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR

# The owner's two wallpapers: their cached stills and thumbnails stand in for the videos.
WC=${WALL_CACHE:-$HOME/.cache/wallpapers}
A=arona-sunset-flower-blue-archive-moewalls-com.mp4; B=azure-horizon.1920x1080.mp4
mkdir -p $R/walls $R/thumbs $R/cfg/island
for f in $A $B; do : > $R/walls/$f; cp $WC/$f.thumb.jpg $R/thumbs/$f.thumb.jpg; cp $WC/$f.still.png $R/$f.png; done
# matugen as the real theme does it: same template, private output.
cat > $R/matugen.toml <<E
[config]
[templates.island]
input_path = "$HOME/.config/theme/templates/colors.toml.tmpl"
output_path = "$R/pal.toml"
E
cat > $R/setwall.sh <<E
#!/bin/sh
f=\$(basename "\$1")
awww img $R/\$f.png --transition-type fade --transition-duration 8 >/dev/null 2>&1
matugen -c $R/matugen.toml image $R/\$f.png --prefer saturation --mode dark -q
E
chmod +x $R/setwall.sh
matugen -c $R/matugen.toml image $R/$A.png --prefer saturation --mode dark -q
echo 70 > $R/bright
# A stand-in for the wallpaper engine's state, so the picker's Settings has something to show.
mkdir -p $R/island
echo '{"scope":"all","busy":false,"now":{"name":"'$A'","fit":"fill","speed":1.0,"fps":"60","quality":"max"}}' > $R/island/wallpaper.json
cat > $R/cfg/island/config.toml <<E
palette = "$R/pal.toml"
[commands]
wallpaper_opt = "true"
brightness_get = "cat $R/bright"
brightness_set = "echo {} > $R/bright"
[wallpaper]
dir = "$R/walls"
thumb = "$R/thumbs/{file}.thumb.jpg"
set = "$R/setwall.sh {path}"
E
export XDG_CONFIG_HOME=$R/cfg

# Replace the fixture's windows with apps that show off the glyphs.
mmsg get all-clients | python3 -c "import json,sys;[print(c['id']) for c in json.load(sys.stdin)['clients']]" > $R/ids
while read -r id; do mmsg dispatch "killclient client,$id" >/dev/null; done < $R/ids
sleep .6
win(){ mmsg dispatch "view,$1,0" >/dev/null; sleep .2; (nohup setsid kitty -c $R/kc/k.conf --class "$2" -e sh -c "$3" >/dev/null 2>&1 &); sleep .7; }
printf 'background_opacity 0.55\ncursor_shape beam\n' >> $R/kc/k.conf
win 2 discord "sleep 999"
win 3 spotify "sleep 999"
win 1 steam "sleep 999"
win 1 kitty "sleep 999"
win 1 zen "sleep 999"

nohup setsid awww-daemon >/dev/null 2>&1 & echo $! > $R/awww.pid
sleep .5; awww img $R/$A.png --transition-type none >/dev/null 2>&1

# Private history and app list: nothing from this machine's user can show up.
export XDG_CACHE_HOME=$R/cache XDG_DATA_HOME=$R/data
mkdir -p $XDG_CACHE_HOME $XDG_DATA_HOME
for t in 'git clone https://github.com/nurexa/Rust-Dynamic-Island-for-Mango' 'cargo build --release' 'Meeting moved to 3pm'; do
  printf '%s' "$t" | cliphist store
done
export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent ISLAND_TIME_SCALE=0.1
$H/full.sh
BIN=${ISLAND_BIN:-$ROOT/target/release/island}
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
sleep .5; $V move 1400 620; sleep .5

F=$R/frames; rm -rf $F; mkdir -p $F
(n=0; while :; do grim -c -t ppm $F/$(printf %05d $n).ppm 2>/dev/null; n=$((n+1)); done) &
CAP=$!
START=$(date +%s.%N)
SL_X0=1130; SL_X1=1330; VOL_Y=0; BRI_Y=0   # slider track ends, measured from a screenshot
pos(){ read -r _ _ X Y < <(grep pos $R/island/state); X=${X%.*}; Y=${Y%.*}; }

sleep 5                                   # the pill, windows running behind it
$V path 1400 620 1000 24 24 12; sleep 9   # hover: the panel, with its volume and brightness sliders
$V path 1000 24 $SL_X0 $VOL_Y 14 12; sleep 1
$V drag $((SL_X1-SL_X0)) 0 20 25; sleep 3
$V path $SL_X1 $VOL_Y $SL_X1 $BRI_Y 8 12; $V drag $((SL_X0-SL_X1)) 0 20 25; sleep 4
$V path $SL_X0 $BRI_Y 1400 620 20 12; sleep 7   # leave: it closes
$BIN open apps; sleep 7                   # the launcher
for k in s t e; do $V type $k; sleep 1.2; done
sleep 3; $V key Escape; sleep 7
$BIN open clipboard; sleep 7              # clipboard search
for k in m e e; do $V type $k; sleep 1.2; done
sleep 3; $V key Escape; sleep 7
$BIN open wallpaper; sleep 7              # wallpapers, with their settings nested inside
$V key Return; sleep 8; $V key Escape; sleep 5
$V key Down; sleep 2; $V key Down; sleep 4   # arona, then azure horizon
$V key Return; sleep 20                   # the wallpaper and the whole palette follow
notify-send -t 8000 -i "$R/art.jpg" "New message" "Are we still on for tonight?"; sleep 12
$V path 1400 620 1010 22 20 10; sleep .1
$V drag 300 420 60 34; sleep 2            # drag it out of the way
sleep 1.5; pos
$V move $((X+150)) $((Y+90)); sleep 1
$V path $((X+150)) $((Y+90)) $((X+120)) $((Y+16)) 10 14
$V dbl $((X+120)) $((Y+16)); sleep 8      # double-click: home
$V path $((X+120)) $((Y+16)) 1400 620 14 10; sleep 3

kill $CAP; sleep .3
SECS=$(python3 -c "import time;print(time.time()-$START)")
N=$(ls $F | wc -l); echo "$N frames"
# A second of capture is a tenth of a second on screen.
FPS=$(python3 -c "print($N/($SECS/10))")
ffmpeg -nostdin -v error -y -framerate "$FPS" -pattern_type glob -i "$F/*.ppm" \
  -vf "fps=22,crop=1240:760:340:0,scale=880:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
  "$ROOT/docs/demo.gif"
ls -la --block-size=K "$ROOT/docs/demo.gif" | awk '{print $5}'
for f in $R/*pid; do kill $(cat "$f") 2>/dev/null; done
