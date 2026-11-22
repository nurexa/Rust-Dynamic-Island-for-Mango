#!/bin/bash
# The panel's tile grid: tiles of one row share a top and a height, a tile with nothing to
# show hands its share to its neighbour, a calendar day inside the grid takes the click,
# and nothing fetches the weather until it's asked to. curl is the shim's. Needs fresh.sh 1.
H=${ISLAND_HEADLESS:-$(dirname "$(readlink -f "$0")")}; source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
export PATH=$H/shim:$PATH XDG_CONFIG_HOME=$R/cfggrid
mkdir -p $XDG_CONFIG_HOME/island; rm -f $R/forecast.json
CFG=$XDG_CONFIG_HOME/island/config.toml
echo 'panel = ["header", "clock", "calendar", "stats", "weather", "about"]' > $CFG
: > $R/shim.log
$H/isl; sleep 1
$V move 960 22; sleep 1.3; grim $R/gr1.png; $V move 1500 900
# Panel x 680..1240. Row one: clock, calendar and stats, left edges 688, 812, 1074 (+6 to stay on the face).
a=$(px $R/gr1.png 694 200 span); b=$(px $R/gr1.png 818 200 span); c=$(px $R/gr1.png 1080 200 span)
ok "row: its tiles share a top and a height" "$a|$b" "$b|$a"
ok "row: so does the third" "$c" "$a"
ok "row: and they're a real card, not a sliver" "$([ "${a#* }" -gt "$(( ${a% *} + 200 ))" ] && echo tall)" tall
# Row two is the about tile alone: it reaches under where weather would have been.
GAP=$(px $R/gr1.png 916 360)
ok "no weather configured: about takes the whole row" "$([ "$GAP" = "$(px $R/gr1.png 1100 360)" ] && [ "$GAP" = "$(px $R/gr1.png 700 360)" ] && echo whole)" whole
ok "no weather configured: nothing fetched" "$(grep -c '^curl' $R/shim.log)" 0
sleep 3; ok "and still nothing a few seconds on" "$(grep -c '^curl' $R/shim.log)" 0

# The calendar, in a column that isn't the first, on a row that isn't the top.
$V move 960 22; sleep 1.3; grim $R/gr2.png
ok "calendar: day 9 starts unselected" "$(px $R/gr2.png 862 193)" "$(px $R/gr2.png 1000 270)"
$V click 873 193; sleep .6; grim $R/gr3.png
ok "calendar: a click on day 9 in the grid selects it" "$([ "$(px $R/gr3.png 862 193)" != "$(px $R/gr2.png 862 193)" ] && echo selected)" selected
ok "calendar: and its neighbour doesn't" "$(px $R/gr3.png 895 193)" "$(px $R/gr2.png 895 193)"
$V click 873 193; sleep .3; $V move 1000 140; sleep .4; grim $R/gr4.png
ok "calendar: a second click clears it" "$(px $R/gr4.png 862 193)" "$(px $R/gr2.png 862 193)"
$V move 1500 900; sleep 1

# With a location the tile appears, takes its share, and the fetch is the shim's.
printf '[weather]\nlocation = "1,2"\n' >> $CFG
echo '{"current":{"temperature_2m":4.6,"weather_code":61,"is_day":1},"daily":{"temperature_2m_max":[7.1],"temperature_2m_min":[1.2]}}' > $R/forecast.json
$BIN reload; sleep 2.5
$V move 960 22; sleep 1.3; grim $R/gr5.png; $V move 1500 900
ok "weather configured: it asked open-meteo, through the shim" "$(grep -c '^curl .*api.open-meteo.com' $R/shim.log)" 1
ok "weather configured: the tile shares row two with about" "$([ "$(px $R/gr5.png 916 360)" != "$(px $R/gr5.png 700 360)" ] && [ "$(px $R/gr5.png 700 360)" = "$(px $R/gr5.png 1100 360)" ] && echo shared)" shared
w=$(px $R/gr5.png 694 400 span); a=$(px $R/gr5.png 927 400 span)
ok "weather configured: and the two share a top and a height" "$w" "$a"
rm -f $R/forecast.json
unset XDG_CONFIG_HOME; $H/isl
