#!/bin/sh
# Start PencariMovie Server when this user logs in (freedesktop autostart).
#
# Writes ~/.config/autostart/pencarimovie-server.desktop, which every GNOME /
# KDE / XFCE / Cinnamon session honours. No root needed.
#
# If `yad` is installed the entry starts the tray (icon + menu); otherwise the
# server is started in the background and you reach it by opening the URL.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
PORT="${PORT:-8088}"
dir="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
mkdir -p "$dir"

# start-tray.sh is the right entry either way: it starts the server detached and
# then draws a tray icon only if yad exists.
exec_line="$here/start-tray.sh"

cat > "$dir/pencarimovie-server.desktop" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=PencariMovie Server
Comment=Local Telegram downloader / Stremio add-on server
Exec=env PORT=$PORT $exec_line
Icon=$here/pencarimovie.png
Terminal=false
X-GNOME-Autostart-enabled=true
Categories=Network;AudioVideo;
EOF
chmod +x "$dir/pencarimovie-server.desktop"
echo "Installed: $dir/pencarimovie-server.desktop"
echo "  starts: $exec_line (port $PORT)"
echo "  remove: ./autostart-off.sh"
