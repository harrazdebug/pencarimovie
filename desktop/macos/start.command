#!/bin/sh
# PencariMovie Server - launcher for macOS (double-click in Finder).
#
#   PORT=9000 ./start.command      different port
#
# The dashboard opens when the server answers. Closing the Terminal window stops
# the server; for login start use ./autostart-on.sh (a LaunchAgent), which runs it
# in the background with no window.
#
# macOS has no shell-level menu-bar API, so there is no tray icon here: the
# LaunchAgent keeps the server running and you open the dashboard from the
# bookmark. (A menu-bar item would require shipping a compiled .app bundle.)
set -eu
cd "$(dirname "$0")"
PORT="${PORT:-8092}"
BIN="./pencarimovie-server"
[ -x "$BIN" ] || { echo "pencarimovie-server not found next to this script"; exit 1; }
ulimit -n 65536 2>/dev/null || true

( sleep 2; open "http://127.0.0.1:$PORT/" >/dev/null 2>&1 || true ) &

echo "PencariMovie Server"
echo "  dashboard : http://127.0.0.1:$PORT/"
echo "  add-on    : http://127.0.0.1:$PORT/manifest.json"
echo "  Ctrl+C to stop"
exec "$BIN" --port "$PORT" "$@"
