#!/bin/sh
# Start PencariMovie Server when this user logs in (macOS LaunchAgent).
#
#   ./autostart-on.sh          install + load now
#   ./autostart-off.sh         remove it
#
# The server runs with no window; open http://127.0.0.1:8088/ when you need the
# dashboard. Logs: storage/server.log. No sudo needed (per-user LaunchAgent).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
PORT="${PORT:-8088}"
label="com.pencarimovie.server"
plist="$HOME/Library/LaunchAgents/$label.plist"
mkdir -p "$HOME/Library/LaunchAgents" "$here/storage"

cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$here/pencarimovie-server</string>
    <string>--port</string><string>$PORT</string>
  </array>
  <key>WorkingDirectory</key><string>$here</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>StandardOutPath</key><string>$here/storage/server.log</string>
  <key>StandardErrorPath</key><string>$here/storage/server.log</string>
</dict>
</plist>
EOF

launchctl unload "$plist" >/dev/null 2>&1 || true
launchctl load "$plist"
echo "Installed: $plist"
echo "  running on http://127.0.0.1:$PORT/  (log: $here/storage/server.log)"
echo "  remove: ./autostart-off.sh"
