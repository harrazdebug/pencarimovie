#!/bin/sh
# Remove the login entry installed by autostart-on.sh (does not stop a running
# server - use ./stop.sh / pkill for that).
set -eu
dir="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
rm -f "$dir/pencarimovie-server.desktop"
echo "Removed $dir/pencarimovie-server.desktop (if it existed)."
