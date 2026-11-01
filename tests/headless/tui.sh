#!/bin/bash
# Start the helper pointer in the headless session.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env
rm -f $XDG_RUNTIME_DIR/vp.fifo; mkfifo $XDG_RUNTIME_DIR/vp.fifo
nohup setsid python3 $H/vp.py $XDG_RUNTIME_DIR/vp.fifo >$XDG_RUNTIME_DIR/vp.log 2>&1 &
echo $! > $XDG_RUNTIME_DIR/vp.pid
sleep 0.3
