#!/usr/bin/env bash
set -euo pipefail

HOST="0.0.0.0"
PORT="8088"
ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

echo "Stopping PencariMovie Server on $HOST:$PORT..."

TUNNEL_PID_FILE="$ROOT_DIR/storage/tunnel/cloudflared.pid"
TUNNEL_CONFIG="$ROOT_DIR/storage/tunnel/config.yml"
if [ -f "$TUNNEL_PID_FILE" ]; then
  TUNNEL_PID="$(cat "$TUNNEL_PID_FILE" 2>/dev/null || true)"
  if [ -n "${TUNNEL_PID:-}" ] && kill -0 "$TUNNEL_PID" 2>/dev/null; then
    echo "Stopping Cloudflare tunnel PID $TUNNEL_PID"
    kill "$TUNNEL_PID" 2>/dev/null || true
    sleep 0.2
    kill -9 "$TUNNEL_PID" 2>/dev/null || true
  fi
  rm -f "$TUNNEL_PID_FILE" 2>/dev/null || true
fi
if command -v pkill >/dev/null 2>&1 && [ -n "$TUNNEL_CONFIG" ]; then
  pkill -f "$TUNNEL_CONFIG" 2>/dev/null || true
fi

for PID_FILE in "$ROOT_DIR/.frankenphp.pid" "$ROOT_DIR/.php-server.pid"; do
  if [ -f "$PID_FILE" ]; then
    PID="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "${PID:-}" ] && kill -0 "$PID" 2>/dev/null; then
      echo "Killing process from $(basename "$PID_FILE") PID $PID"
      kill "$PID" 2>/dev/null || true
      sleep 0.2
      kill -9 "$PID" 2>/dev/null || true
    fi
    rm -f "$PID_FILE" 2>/dev/null || true
  fi
done

if command -v pkill >/dev/null 2>&1; then
  pkill -9 -f "frankenphp.*Caddyfile" 2>/dev/null || true
  pkill -9 -f "frankenphp.*php-server" 2>/dev/null || true
fi

if command -v lsof >/dev/null 2>&1; then
  PIDS="$(lsof -ti tcp:"$PORT" -sTCP:LISTEN || true)"
elif command -v fuser >/dev/null 2>&1; then
  PIDS="$(fuser "$PORT"/tcp 2>/dev/null || true)"
fi

if [ -z "${PIDS:-}" ]; then
  echo "No process is listening on $HOST:$PORT."
  exit 0
fi

echo "$PIDS" | tr ' ' '\n' | while read -r PID; do
  [ -z "$PID" ] && continue
  echo "Killing process PID $PID"
  kill "$PID" 2>/dev/null || true
done

# The MadelineProto IPC worker is a DETACHED `php entry.php madeline-ipc <sessionDir>`
# process. Killing only the port listener left it holding the session lock and the `ipc`
# endpoint, so the next start's worker could never boot: the log filled with
#   Serialization: Still waiting for exclusive session lock...
#   APIWrapper:   Saved session!
#   Could not connect to MadelineProto ... in API.php:324   (from entry.php)
# and the browser saw "The operation was cancelled". A restart did NOT clear it, which is
# what made it look like "downloads are broken".
if command -v pkill >/dev/null 2>&1; then
  if pkill -f "madeline-ipc" 2>/dev/null; then
    echo "Killing MadelineProto IPC worker(s)"
  fi
fi
# Drop the endpoints so a stale socket can never be reused.
if [ -d "$ROOT_DIR/storage/sessions" ]; then
  find "$ROOT_DIR/storage/sessions" -maxdepth 2 -type f \
    \( -name ipc -o -name callback.ipc -o -name ipcState.php \) -delete 2>/dev/null || true
fi

echo "Stop command completed."
