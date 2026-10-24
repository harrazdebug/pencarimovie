#!/bin/sh
# Apply a downloaded update (started by POST /api/update/apply).
#
#   ./apply-update.sh <archive.tar.gz>
#
# `storage/` (sessions, auth.json, caches) is never inside the archive, so
# copying over this folder keeps the user's state.
set -eu
ARCHIVE="${1:?usage: apply-update.sh <archive.tar.gz>}"
cd "$(dirname "$0")"

pkill -f "pencarimovie-server --port" 2>/dev/null || true
sleep 1

TMP="storage/updates/unpack"
rm -rf "$TMP"
mkdir -p "$TMP"
tar -xzf "$ARCHIVE" -C "$TMP"

INNER=$(find "$TMP" -mindepth 1 -maxdepth 1 -type d | head -1)
[ -n "$INNER" ] || { echo "archive has no folder" >&2; exit 1; }

for entry in "$INNER"/*; do
  base=$(basename "$entry")
  [ "$base" = "storage" ] && continue
  cp -Rf "$entry" ./
done
chmod +x pencarimovie-server start.command autostart-on.sh autostart-off.sh apply-update.sh 2>/dev/null || true
rm -rf "$TMP" "$ARCHIVE"
xattr -dr com.apple.quarantine ./pencarimovie-server 2>/dev/null || true

echo "Update applied. Restarting PencariMovie Server..."
nohup ./pencarimovie-server --port "${PORT:-8088}" >storage/server.log 2>&1 &
