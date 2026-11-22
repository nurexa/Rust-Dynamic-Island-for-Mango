#!/bin/bash
# Wallpaper engine and panel: settings reach the cached video and the panel shows them.
# Needs gslapper, awww, ffmpeg. Runs in the private headless session; touches no real config.
H=${ISLAND_HEADLESS:-$(dirname "$(readlink -f "$0")")}; source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
WALL=${WALL:-$(readlink -f $H/../../wall/wall)}
python3 $WALL --selftest >/dev/null && echo "ok   wall selftest" || echo "FAIL wall selftest"
mkdir -p $R/bin $R/walls $R/cfg/island; ln -sf $BIN $R/bin/island
# A 21:9 clip: the case black bars exist for.
ffmpeg -nostdin -y -v error -f lavfi -i "testsrc2=s=2560x1080:r=30:d=2" -pix_fmt yuv420p $R/walls/wide.mp4
export PATH=$R/bin:$PATH XDG_CONFIG_HOME=$R/cfg XDG_STATE_HOME=$R XDG_CACHE_HOME=$R/cache
printf 'panel = ["header", "wallpaper"]\n[wallpaper]\ndir = "%s/walls"\n[commands]\nwallpaper_opt = "%s opt"\n' $R $WALL > $R/cfg/island/config.toml
$H/isl; sleep .3
$WALL set $R/walls/wide.mp4
rate(){ ffprobe -v error -show_entries stream=width,height,r_frame_rate -of csv=p=0 "$1" | tr , ' '; }
ok "cached copy is monitor size at the source rate" "$(rate $R/cache/wallpapers/wide.mp4.*-fill-1x-source-high.mp4)" "1920 1080 30/1"
$WALL opt fit fit; $WALL opt speed 2
f=$(ls $R/cache/wallpapers/wide.mp4.*-fit-2x-source-high.mp4)
ok "speed 2 halves the duration" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 $f | cut -c1-3)" "1.0"
# The poster is the first frame as shown: bars top and bottom, picture between.
bars=$(python3 -c "
from PIL import Image; im = Image.open('${f%.mp4}.jpg').convert('L')
print(max(im.getpixel((x, 20)) for x in range(0, 1920, 10)) < 8 and max(im.getpixel((x, 540)) for x in range(0, 1920, 10)) > 40)")
ok "black bars: wide clip is letterboxed" "$bars" "True"
echo "$($WALL opt | tr -d ' \n')" | grep -q '"fit":"fit"' && echo "ok   opt prints the live settings" || echo "FAIL opt output"
$WALL opt fit bogus 2>/dev/null; ok "bad value is refused" "$?" "1"
# The panel draws a row of chips for it.
island pin; sleep 1.2; grim $R/wall-b.png
ok "panel grows to hold the wallpaper section" "$(px $R/wall-b.png 700 200 opaque)" "yes"
island pin

# A still reaches the screen as it was drawn: thin colored diagonals keep their color and
# aren't torn into steps on the way through the player (4:4:4 JPEG was, by jpegdec).
for e in png jpg; do
  magick -size 3840x2160 xc:white -fill none -stroke '#e02040' -strokewidth 3 \
    -draw "circle 1920,1180 2720,1180" -quality 94 -sampling-factor 1x1 $R/walls/edge-$e.$e
  magick $R/walls/edge-$e.$e -filter Lanczos -resize 1920x1080 $R/edge-want.png
  mmsg dispatch "view,5,0" >/dev/null; $V move 1500 1050; $WALL set $R/walls/edge-$e.$e; sleep 2; grim $R/edge-got.png
  for f in want got; do magick $R/edge-$f.png -crop 900x800+510+190 +repage -alpha off $R/edge-$f-crop.png; done
  db=$(magick compare -metric PSNR $R/edge-got-crop.png $R/edge-want-crop.png null: 2>&1 | cut -d' ' -f1)
  ok "still ($e): edges reach the screen clean" "$(awk -v d="$db" 'BEGIN{print (d >= 40) ? "clean" : "torn at " d " dB"}')" clean
done
magick -size 800x450 xc:teal $R/walls/later.png
$WALL prep
ok "prep caches a wallpaper that was never set" "$(ls $R/cache/wallpapers/later.png.1920x1080-*-lossless.png 2>/dev/null | wc -l)" 1
mmsg dispatch "view,1,0" >/dev/null
