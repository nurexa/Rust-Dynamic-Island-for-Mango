#!/bin/bash
# Records docs/demo.gif in a private headless Mango, with neutral windows, a mock
# player and no network name, so nothing personal can end up in it. Animations run
# at a tenth of real speed and the capture is sped back up, because grim is far
# slower than the spring. Needs ffmpeg and awww.
H=$(dirname "$(readlink -f "$0")")
ROOT=$(readlink -f "$H/../..")
$H/fresh.sh 1 >/dev/null; rm -f $H/run/island/state
source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR

# Replace the fixture's windows with apps that show off the glyphs.
mmsg get all-clients | python3 -c "import json,sys;[print(c['id']) for c in json.load(sys.stdin)['clients']]" > $R/ids
while read -r id; do mmsg dispatch "killclient client,$id" >/dev/null; done < $R/ids
sleep .6
win(){ mmsg dispatch "view,$1,0" >/dev/null; sleep .2; (nohup setsid kitty -c $R/kc/k.conf --class "$2" -e sh -c "$3" >/dev/null 2>&1 &); sleep .7; }
printf 'background_opacity 0.55\ncursor_shape beam\n' >> $R/kc/k.conf
win 2 discord "sleep 999"
win 3 spotify "sleep 999"
win 1 kitty "sleep 999"
win 1 firefox "sleep 999"

ffmpeg -nostdin -v error -y -f lavfi -i "gradients=s=1920x1080:c0=0x2b3a78:c1=0xb06a9c:x0=0:y0=0:x1=1920:y1=1080:n=2" -frames:v 1 $R/wall.png
nohup setsid awww-daemon >/dev/null 2>&1 & echo $! > $R/awww.pid
sleep .5; awww img $R/wall.png --transition-type none >/dev/null 2>&1

export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent ISLAND_TIME_SCALE=0.1
$H/full.sh
sleep .5; $V move 1500 800; sleep .5

F=$R/frames; rm -rf $F; mkdir -p $F
(n=0; while :; do grim -c -t ppm $F/$(printf %05d $n).ppm 2>/dev/null; n=$((n+1)); done) &
CAP=$!
START=$(date +%s.%N)
pos(){ read -r _ _ X Y < <(grep pos $R/island/state); X=${X%.*}; Y=${Y%.*}; }

sleep 7                                   # the pill on its own, windows behind it
$V path 1400 620 1000 24 24 12; sleep 9   # hover: it opens
$V path 1000 24 806 106 18 16; sleep 2
$V down; sleep .1; $V up; sleep 6         # switch workspace from the panel
$V path 806 106 712 106 12 16; sleep 1.5
$V down; sleep .1; $V up; sleep 5         # and back
$V path 712 106 1400 620 20 12; sleep 8   # leave: it closes
$V path 1400 620 1010 22 20 10; sleep .1
$V drag 300 420 60 34; sleep 2            # drag it out of the way
sleep 1.5; pos
$V move $((X+150)) $((Y+90)); sleep 1
$V path $((X+150)) $((Y+90)) $((X+110)) $((Y+16)) 10 14
$V dbl $((X+110)) $((Y+16)); sleep 8      # double-click: home
$V path $((X+110)) $((Y+16)) 1400 620 14 10; sleep 4

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
