#!/usr/bin/env bash
set -euo pipefail

print_banner() {
  [ -n "${PENCARIMOVIE_NO_BANNER:-}" ] && return 0
  local orange="" reset=""
  if [ -t 1 ]; then
    orange="$(printf '\033[38;5;208m')"
    reset="$(printf '\033[0m')"
  fi
  printf '%s' "$orange"
  cat <<'EOF'

 ========================================
          PencariMovie Server
 ========================================

EOF
  printf '%s' "$reset"
}

print_banner

# Fixed installation path under $HOME (like 9router) unless already running inside the project root
# Fixed installation path under $HOME (like 9router).
# Only treat as a developer in-place checkout if .git exists in the current directory.
if [ -f "./backend.php" ] && [ -f "./start.sh" ] && [ -d "./.git" ]; then
  APP_DIR="."
  OLD_APP_DIR="pencarimovie-downloader"
else
  APP_DIR="${HOME:-/root}/pencarimovie-server"
  OLD_APP_DIR="${HOME:-/root}/pencarimovie-downloader"
fi
PORT="${PORT:-8088}"
HOST="${HOST:-0.0.0.0}"
REPO="aiskendi/pencarimovie-server"
FALLBACK_TAG="v1.0.0"

detect_target() {
  local arch os
  arch="$(uname -m)"
  os="$(uname -s)"
  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64)  echo "linux-x86_64" ;;
        aarch64|arm64) echo "linux-aarch64" ;;
        armv7*|armv8l|armhf|arm)
          # 32-bit userland. Many Android TV boxes / Xiaomi Mi Box devices report
          # armv7l on top of a 64-bit ARM kernel, so accept it only when the kernel
          # actually advertises a 64-bit ARM ABI. PencariMovie Server is 64-bit only
          # (MadelineProto builds MTProto message IDs as time() << 32, which overflows
          # 32-bit PHP) and no 32-bit FrankenPHP build exists.
          local abis=""
          if command -v getprop >/dev/null 2>&1; then
            abis="$(getprop ro.product.cpu.abilist64 2>/dev/null || true)"
            [ -z "$abis" ] && abis="$(getprop ro.product.cpu.abilist 2>/dev/null || true)"
          fi
          if echo "$abis" | grep -qi "arm64"; then
            echo "linux-aarch64"
          else
            echo "PencariMovie Server is 64-bit only and this device exposes no 64-bit ARM ABI (arch=$arch, abilist64='$abis'). Use a 64-bit OS or run it under an aarch64 proot." >&2
            exit 1
          fi
          ;;
        i586|i686|i386)
          echo "PencariMovie Server is 64-bit only; 32-bit x86 ($arch) is not supported. Use a linux-x86_64 system." >&2
          exit 1
          ;;
        *)
          echo "Unsupported architecture: $arch. PencariMovie Server is 64-bit only (linux-x86_64 or linux-aarch64)." >&2
          exit 1
          ;;
      esac
      ;;
    Darwin)
      # These MUST match the published artifact names
      # (`pencarimovie-server-<os>-<arch>.tar.gz`): macos-aarch64 / macos-x86_64.
      # They used to be mac-arm64 / mac-x86_64, which 404'd on every Mac.
      case "$arch" in
        arm64|aarch64) echo "macos-aarch64" ;;
        x86_64|amd64)  echo "macos-x86_64" ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    *) echo "Unsupported OS: $os. PencariMovie Server supports Linux, macOS, Android (Termux/APK), and Windows." >&2; exit 1 ;;
  esac
}

usage() {
  echo "Usage: $0 [start|stop|restart|tunnel [token]|autostart|password|reset-password|token|uninstall]"
  exit 1
}

get_lan_ip() {
  local ip=""
  if [ "$(uname -s)" = "Darwin" ]; then
    ip="$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || true)"
  fi
  if [ -z "$ip" ] && command -v ip >/dev/null 2>&1; then
    ip="$(ip route get 8.8.8.8 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") {print $(i+1); exit}}')"
  fi
  if [ -z "$ip" ] && command -v hostname >/dev/null 2>&1; then
    ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
  fi
  echo "$ip"
}

print_urls() {
  local lan_ip
  lan_ip="$(get_lan_ip)"
  echo "  Local:    http://127.0.0.1:$PORT"
  [ -n "$lan_ip" ] && echo "  Network:  http://$lan_ip:$PORT"
  echo "  CLI:      pms [start|stop|restart|tunnel|autostart|uninstall]"
  echo "  Stop:     pms stop"
  echo "  Restart:  pms restart"
  echo "  Tunnel:   pms tunnel"
  echo "  Autostart: pms autostart [on|off]"
}

port_in_use() {
  if command -v curl >/dev/null 2>&1; then
    curl -s -o /dev/null http://127.0.0.1:"$PORT" 2>/dev/null && return 0
  fi
  if command -v wget >/dev/null 2>&1; then
    wget -q -O /dev/null http://127.0.0.1:"$PORT" 2>/dev/null && return 0
  fi
  return 1
}

do_stop() {
  echo "Stopping PencariMovie Server..."

  local pid=""
  for pid_file in "$APP_DIR/.frankenphp.pid" "$APP_DIR/.php-server.pid"; do
    if [ -f "$pid_file" ]; then
      pid="$(cat "$pid_file" 2>/dev/null || true)"
      if [ -n "$pid" ]; then
        kill "$pid" 2>/dev/null || true
        kill -9 "$pid" 2>/dev/null || true
      fi
      rm -f "$pid_file" 2>/dev/null || true
    fi
  done

  pkill -9 -f "frankenphp.*Caddyfile" 2>/dev/null || true
  pkill -9 -f "frankenphp.*php-server" 2>/dev/null || true
  pkill -9 -f "php.*router\.php" 2>/dev/null || true
  # The published engine is the Rust binary, which none of the patterns above
  # match. Without this, `pms stop` left it running and `pms restart` raced it
  # for the port.
  pkill -9 -f "$APP_DIR/pencarimovie-server" 2>/dev/null || true
  pkill -9 -f "\./pencarimovie-server --port" 2>/dev/null || true

  if command -v lsof >/dev/null 2>&1; then
    local pids_port
    pids_port="$(lsof -ti tcp:"$PORT" -sTCP:LISTEN 2>/dev/null || true)"
    [ -n "$pids_port" ] && kill -9 $pids_port 2>/dev/null || true
  fi

  if command -v fuser >/dev/null 2>&1; then
    fuser -k -9 "$PORT"/tcp 2>/dev/null || true
  fi

  rm -f "$APP_DIR/.frankenphp.pid"
  echo "Server stopped."
}

download_file() {
  local url="$1" dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -L --fail -o "$dest" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$dest" "$url"
  else
    echo "Need curl or wget."; exit 1
  fi
}

# Query GitHub releases API (or redirect location) and return the tag (e.g. v1.6.0).
fetch_latest_tag() {
  local tag=""
  # Method 1: GitHub REST API with 5s timeout to prevent hanging on network stalls
  if command -v curl >/dev/null 2>&1; then
    tag="$(curl -fsSL --connect-timeout 4 --max-time 6 -H "User-Agent: pencarimovie-server" "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)"
  elif command -v wget >/dev/null 2>&1; then
    tag="$(wget -qO- -T 6 -t 1 --header="User-Agent: pencarimovie-server" "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)"
  fi

  # Method 2: Redirect follow via curl %{url_effective}
  if [ -z "$tag" ] && command -v curl >/dev/null 2>&1; then
    local loc=""
    loc="$(curl -fsSL --connect-timeout 4 --max-time 6 -A "Mozilla/5.0" -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null || true)"
    loc="${loc%$'\r'}"
    loc="${loc%/}"
    tag="${loc##*/}"
  fi

  # Method 3: wget fallback
  if [ -z "$tag" ] && command -v wget >/dev/null 2>&1; then
    local loc=""
    loc="$(wget -q -T 6 -t 1 --max-redirect=0 --server-response "https://github.com/$REPO/releases/latest" -O /dev/null 2>&1 \
      | awk 'BEGIN{IGNORECASE=1} /^  Location:/{print $2; exit}' | tr -d '\r' || true)"
    loc="${loc%$'\r'}"
    loc="${loc%/}"
    tag="${loc##*/}"
  fi

  case "$tag" in
    v[0-9]*) echo "$tag" ;;
    *) return 1 ;;
  esac
}

current_tag() {
  if [ -f "$APP_DIR/.release-tag" ]; then
    tr -d '\r\n' < "$APP_DIR/.release-tag"
  fi
}

find_release_root() {
  local extract_dir="$1"
  if [ -f "$extract_dir/backend.php" ] || [ -f "$extract_dir/start.sh" ]; then
    echo "$extract_dir"
    return
  fi
  local found=""
  found="$(find "$extract_dir" -maxdepth 2 -type f \( -name backend.php -o -name start.sh \) 2>/dev/null | head -1 || true)"
  if [ -n "$found" ]; then
    dirname "$found"
    return
  fi
  echo "$extract_dir"
}

# Copy a extracted release into APP_DIR without touching existing storage/.
migrate_legacy_dir() {
  if [ -d "$OLD_APP_DIR" ] && [ "$OLD_APP_DIR" != "$APP_DIR" ]; then
    if [ -d "$OLD_APP_DIR/storage" ] && [ ! -d "$APP_DIR/storage" ]; then
      mkdir -p "$APP_DIR"
      cp -R "$OLD_APP_DIR/storage" "$APP_DIR/storage"
    fi
    rm -rf "$OLD_APP_DIR"
  fi
}

copy_release_into_app() {
  local src="$1" item name
  mkdir -p "$APP_DIR"
  for item in "$src"/*; do
    [ -e "$item" ] || continue
    name="$(basename "$item")"
    if [ "$name" = "storage" ]; then
      mkdir -p "$APP_DIR/storage"
      continue
    fi
    if [ "$name" = "bin" ]; then
      mkdir -p "$APP_DIR/bin"
      # Preserve existing binaries (like bin/frankenphp) when updating from pencarimovie-server.tar.gz
      cp -R "$item"/* "$APP_DIR/bin/" 2>/dev/null || true
      continue
    fi
    rm -rf "$APP_DIR/$name"
    cp -R "$item" "$APP_DIR/$name"
  done
}

strip_crlf() {
  local dir="${1:-.}" f
  for f in "$dir"/*.sh; do
    [ -f "$f" ] || continue
    tr -d '\r' < "$f" > "$f.tmp" && mv "$f.tmp" "$f"
  done
}

download_extract() {
  local target="$1" tag="$2"
  # There is exactly ONE artifact shape now: the per-target Rust package
  # `pencarimovie-server-<os>-<arch>.tar.gz`. The universal
  # `pencarimovie-server.tar.gz` (the old PHP product) is no longer published, so
  # a fallback to it only produced a 404 after the real download had already
  # succeeded — keep the two variables so the .sha256 probe loop below still works.
  local url="https://github.com/$REPO/releases/download/$tag/pencarimovie-server-$target.tar.gz"
  local fallback_url=""
  local tmp src

  tmp="${TMPDIR:-/tmp}/pencarimovie-ota-$$"
  rm -rf "$tmp"
  mkdir -p "$tmp/extract"

  echo "Downloading $url"
  if ! download_file "$url" "$tmp/pencarimovie.tar.gz" 2>/dev/null; then
    if [ -n "$fallback_url" ]; then
      echo "Primary package ($target) download failed, trying fallback: $fallback_url"
      if ! download_file "$fallback_url" "$tmp/pencarimovie.tar.gz" 2>/dev/null; then
        echo "Failed to download release archive."
        exit 1
      fi
    else
      echo "Failed to download release archive: $url"
      exit 1
    fi
  fi
  # Integrity: GitHub publishes a .sha256 sidecar next to each artifact. Verify it
  # when the release has one; a mismatch aborts. A release without a sidecar only
  # warns, so old releases keep installing.
  sha_seen=""
  sha_matched=""
  for sidecar_src in "$url" "$fallback_url"; do
    [ -n "$sidecar_src" ] || continue
    download_file "${sidecar_src}.sha256" "$tmp/pkg.sha256" 2>/dev/null || continue
    want="$(grep -oE '[0-9a-fA-F]{64}' "$tmp/pkg.sha256" | head -n1 | tr 'A-F' 'a-f')"
    [ -n "$want" ] || continue
    sha_seen=1
    got="$( { sha256sum "$tmp/pencarimovie.tar.gz" || shasum -a 256 "$tmp/pencarimovie.tar.gz" || openssl dgst -sha256 "$tmp/pencarimovie.tar.gz"; } 2>/dev/null | grep -oE '[0-9a-fA-F]{64}' | head -n1 | tr 'A-F' 'a-f')"
    if [ "$want" = "$got" ]; then sha_matched=1; break; fi
  done
  if [ -n "$sha_seen" ] && [ -z "$sha_matched" ]; then
    echo "Release archive failed integrity check; aborting."
    rm -rf "$tmp"
    exit 1
  fi
  [ -n "$sha_seen" ] || echo "No .sha256 sidecar published for this release; skipping integrity check."
  tar -xzf "$tmp/pencarimovie.tar.gz" -C "$tmp/extract"
  src="$(find_release_root "$tmp/extract")"
  copy_release_into_app "$src"
  strip_crlf "$APP_DIR"
  # On macOS, clear quarantine flags from downloaded binaries
  if [ "$(uname -s)" = "Darwin" ]; then
    xattr -rd com.apple.quarantine "$APP_DIR" 2>/dev/null || true
  fi
  printf '%s\n' "$tag" > "$APP_DIR/.release-tag"
  rm -rf "$tmp"
}

# Returns 0 if files were installed/updated, 1 if already up to date.
install_or_update() {
  local target latest current
  target="$(detect_target)"
  latest="$(fetch_latest_tag || true)"
  current="$(current_tag)"

  local is_installed=0
  # The published package is the Rust engine, shipped as `pencarimovie-server`
  # with its launchers. `backend.php` / `bin/frankenphp` are the OLD PHP markers:
  # keying only on those made a fresh Rust install look "not installed", so every
  # `pms start` re-downloaded the whole tarball.
  if [ -d "$APP_DIR" ] && { [ -x "$APP_DIR/pencarimovie-server" ] || [ -f "$APP_DIR/start.sh" ] || [ -f "$APP_DIR/backend.php" ] || [ -f "$APP_DIR/bin/frankenphp" ]; }; then
    is_installed=1
  fi

  if [ "$is_installed" -eq 1 ] && [ -z "$current" ]; then
    current="$FALLBACK_TAG"
    printf '%s\n' "$current" > "$APP_DIR/.release-tag"
  fi

  if [ "$is_installed" -eq 1 ]; then
    echo "Checking for updates [current: ${current:-unknown}]..."
  else
    echo "Checking for updates..."
  fi

  if [ -z "$latest" ]; then
    if [ "$is_installed" -eq 1 ]; then
      echo "Could not check GitHub for updates; using installed copy."
      return 1
    fi
    latest="$FALLBACK_TAG"
  fi

  if [ "$is_installed" -eq 1 ] && [ "$current" = "$latest" ]; then
    echo "Already up to date [$current]."
    return 1
  fi

  if [ "$is_installed" -eq 0 ]; then
    echo "Downloading PencariMovie Server $latest ($target)..."
    download_extract "$target" "$latest"
  else
    echo "Updating PencariMovie Server ${current:-unknown} -> $latest ($target)..."
    if port_in_use; then
      do_stop
      sleep 1
    fi
    # ALWAYS the per-target package. The old "fast updater" fetched
    # pencarimovie-server.tar.gz, which this release no longer publishes, so an
    # existing install could never update again.
    download_extract "$target" "$latest"
  fi
  register_cli
  return 0
}

register_cli() {
  local bin_dir="${HOME:-/root}/.local/bin"
  mkdir -p "$bin_dir" 2>/dev/null || true

  # Install self-contained wrappers (independent of the OTA installer file)
  local system_bin="/usr/local/bin"
  local home_bin="${HOME:-/root}/bin"
  for cmd in pms pm pencarimovie; do
    cat <<EOF > "$bin_dir/$cmd"
#!/usr/bin/env bash
# PencariMovie Server CLI launcher
APP_DIR="$APP_DIR"
case "\${1:-}" in
  stop|--stop)
    bash "\$APP_DIR/stop.sh"
    ;;
  restart|--restart)
    bash "\$APP_DIR/restart.sh"
    ;;
  tunnel|--tunnel)
    if [ -f "\$APP_DIR/pencarimovie-linux.sh" ]; then
      bash "\$APP_DIR/pencarimovie-linux.sh" tunnel "\${2:-}"
    else
      echo "Enabling Cloudflare Tunnel..."
      if [ -n "\${2:-}" ]; then
        curl -fsSL -X POST "http://127.0.0.1:\${PORT:-8088}/api/tunnel/enable" -H "Content-Type: application/json" -d "{\"tunnel_token\":\"\${2:-}\"}" --max-time 120 2>/dev/null || wget -qO- --header="Content-Type: application/json" --post-data="{\"tunnel_token\":\"\${2:-}\"}" "http://127.0.0.1:\${PORT:-8088}/api/tunnel/enable" --timeout=120 2>/dev/null || true
      else
        curl -fsSL -X POST "http://127.0.0.1:\${PORT:-8088}/api/tunnel/enable" --max-time 120 2>/dev/null || wget -qO- --post-data="" "http://127.0.0.1:\${PORT:-8088}/api/tunnel/enable" --timeout=120 2>/dev/null || true
      fi
    fi
    ;;
  autostart|--autostart)
    if [ -f "\$APP_DIR/pencarimovie-linux.sh" ]; then
      bash "\$APP_DIR/pencarimovie-linux.sh" autostart "\${2:-}"
    else
      echo "Autostart command not found in \$APP_DIR"
    fi
    ;;
  password|--password|reset-password|--reset-password|token|--token)
    # These were MISSING from this wrapper, so `pms password foo` fell through to
    # the default branch and only STARTED the server. They are also implemented
    # here rather than delegated, because the Rust package does not ship
    # pencarimovie-linux.sh inside \$APP_DIR (only the OTA installer has it).
    PMS_PORT="\${PORT:-8088}"
    pms_api() {
      # $1 = path, $2 = JSON body (defaults to an empty object)
      pms_body="\${2}"
      [ -n "\$pms_body" ] || pms_body='{}'
      curl -fsS -m 15 -H 'Content-Type: application/json' -X POST \\
        -d "\$pms_body" "http://127.0.0.1:\$PMS_PORT\$1" 2>/dev/null
    }
    pms_tok() { sed -n 's/.*"token":"\\([^"]*\\)".*/\\1/p' | head -1; }
    case "\${1:-}" in
      token|--token)
        if [ "\${2:-show}" = "rotate" ]; then
          if ! pms_out="\$(pms_api /api/auth/token/rotate)"; then
            echo "Server is not running. Start it once with 'pms start', then retry."
            exit 1
          fi
          printf '%s\\n' "\$pms_out" | pms_tok
          echo "Token rotated. Re-install the addon from #addon on every device."
        else
          curl -fsS -m 8 "http://127.0.0.1:\$PMS_PORT/api/auth/status" 2>/dev/null | pms_tok
        fi
        ;;
      *)
        # Reset to the default over loopback (privileged), then set the requested
        # value using that default as the "current" one.
        if ! pms_api /api/auth/reset >/dev/null; then
          echo "Server is not running. Start it once with 'pms start', then retry."
          exit 1
        fi
        pms_next="123456"
        case "\${1:-}" in
          reset-password|--reset-password) ;;
          *)
            pms_next="\${2:-}"
            if [ -z "\$pms_next" ]; then
              echo "Usage: pms password <new-password>"
              exit 1
            fi
            ;;
        esac
        if pms_api /api/auth/password \\
             "{\\"current\\":\\"123456\\",\\"next\\":\\"\$pms_next\\"}" >/dev/null; then
          echo "Password updated."
        else
          echo "Password was reset to the default, but setting the new one failed."
          exit 1
        fi
        ;;
    esac
    ;;
  uninstall|--uninstall)
    bash "\$APP_DIR/stop.sh" 2>/dev/null || true
    rm -f "\$HOME/.local/bin/pms" "\$HOME/.local/bin/pm" "\$HOME/.local/bin/pencarimovie" 2>/dev/null || true
    rm -f "/usr/local/bin/pms" "/usr/local/bin/pm" "/usr/local/bin/pencarimovie" 2>/dev/null || true
    rm -f "\$HOME/bin/pms" "\$HOME/bin/pm" "\$HOME/bin/pencarimovie" 2>/dev/null || true
    rm -rf "\$APP_DIR"
    echo "PencariMovie Server has been uninstalled."
    ;;
  *)
    # Run the OTA installer so it checks GitHub for updates before starting.
    if [ -f "\$APP_DIR/pencarimovie-linux.sh" ]; then
      exec bash "\$APP_DIR/pencarimovie-linux.sh" "\$@"
    else
      exec bash "\$APP_DIR/start.sh"
    fi
    ;;
esac
EOF
    chmod +x "$bin_dir/$cmd" 2>/dev/null || true

    if [ -w "$system_bin" ]; then
      cp -f "$bin_dir/$cmd" "$system_bin/$cmd" 2>/dev/null || true
    elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
      sudo cp -f "$bin_dir/$cmd" "$system_bin/$cmd" 2>/dev/null || true
    fi
    if [ -d "$home_bin" ] || [[ ":$PATH:" == *":${HOME:-/root}/bin:"* ]]; then
      mkdir -p "$home_bin" 2>/dev/null || true
      ln -sf "$bin_dir/$cmd" "$home_bin/$cmd" 2>/dev/null || cp -f "$bin_dir/$cmd" "$home_bin/$cmd" 2>/dev/null || true
    fi
  done

  # Ensure ~/.local/bin is in PATH in shell profile files if not already
  for rc in "${HOME:-/root}/.bashrc" "${HOME:-/root}/.profile" "${HOME:-/root}/.bash_profile" "${HOME:-/root}/.zshrc"; do
    if [ -f "$rc" ] && ! grep -q '\.local/bin' "$rc" 2>/dev/null; then
      printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc"
    elif [ ! -f "$rc" ] && [ "$(basename "$rc")" = ".zshrc" ] && [ "$(uname -s)" = "Darwin" ]; then
      # macOS default shell is zsh; ensure ~/.zshrc has PATH even if file did not exist
      printf 'export PATH="$HOME/.local/bin:$PATH"\n' > "$rc" 2>/dev/null || true
    fi
  done
  export PATH="${HOME:-/root}/.local/bin:${HOME:-/root}/bin:$PATH"
}

do_start() {
  migrate_legacy_dir

  local had_app=0 updated=0
  [ -d "$APP_DIR" ] && had_app=1

  if install_or_update; then
    updated=1
  fi

  register_cli

  # Enable autostart by default on first run (like 9router)
  if [ ! -f "$APP_DIR/storage/.no_autostart" ]; then
    local _desk="${HOME:-/root}/.config/autostart/pencarimovie.desktop"
    local _sysd="${HOME:-/root}/.config/systemd/user/pencarimovie.service"
    local _plist="${HOME:-/root}/Library/LaunchAgents/com.pencarimovie.server.plist"
    if [ ! -f "$_desk" ] && [ ! -f "$_sysd" ] && [ ! -f "$_plist" ]; then
      do_autostart on >/dev/null 2>&1 || true
    fi
  fi

  if port_in_use; then
    if [ "$had_app" -eq 1 ] && [ "$updated" -eq 0 ]; then
      echo "Server is already running on port $PORT."
      print_urls
      case ":$PATH:" in
        *":${HOME:-/root}/.local/bin:"*|*":/usr/local/bin:"*|*":${HOME:-/root}/bin:"*) ;;
        *)
          echo "  Note: Run 'source ~/.bashrc' or 'export PATH=\"\$HOME/.local/bin:\$PATH\"' to use 'pms'."
          ;;
      esac
      return
    fi
    echo "Port $PORT is already in use; stopping leftover process..."
    do_stop
    sleep 1
  fi

  cd "$APP_DIR"
  strip_crlf "."

  ROOT_DIR="$(pwd)"
  FRANKENPHP_BIN="$ROOT_DIR/bin/frankenphp"

  # Ensure the IPC worker wrapper and runtime are executable. Windows-created
  # tarballs often lose the +x bit, so set it explicitly here.
  for FILE in \
    "$FRANKENPHP_BIN" \
    "$ROOT_DIR/bin/php" \
    "$ROOT_DIR/backend.php" \
    "$ROOT_DIR/index.php" \
    "$ROOT_DIR/router.php" \
    "$ROOT_DIR/start.sh" \
    "$ROOT_DIR/stop.sh" \
    "$ROOT_DIR/restart.sh"
  do
    if [ -f "$FILE" ]; then
      chmod u+x "$FILE" 2>/dev/null || true
    fi
  done

  register_cli
  # Always run start.sh detached in background so interactive terminal commands and piped curl | bash never block on child I/O.
  # PORT is passed explicitly: it is a local variable here, not exported, so a
  # non-default port used to be silently dropped by the launcher.
  nohup bash -c 'cd "'"$APP_DIR"'" && PORT="'"$PORT"'" PENCARIMOVIE_NO_BANNER=1 bash start.sh' </dev/null >/dev/null 2>&1 &
  sleep 1
  print_urls
  echo "PencariMovie Server started in the background."
  if [ "$(uname -s)" = "Darwin" ]; then
    open "http://127.0.0.1:$PORT" 2>/dev/null || true
  fi
  case ":$PATH:" in
    *":${HOME:-/root}/.local/bin:"*|*":/usr/local/bin:"*|*":${HOME:-/root}/bin:"*) ;;
    *)
      echo "  Note: Run 'source ~/.bashrc' or 'export PATH=\"\$HOME/.local/bin:\$PATH\"' to use 'pms'."
      ;;
  esac
}

do_restart() { do_stop; sleep 1; do_start; }

do_tunnel() {
  local token="${1:-}"
  if ! port_in_use; then
    echo "Server is not running. Starting server first..."
    do_start
    sleep 2
  fi

  local resp=""
  local payload=""
  if [ -n "$token" ]; then
    echo "Enabling Cloudflare Named Tunnel with token..."
    payload="{\"tunnel_token\":\"$token\"}"
  else
    echo "Enabling Cloudflare Tunnel..."
  fi

  if command -v curl >/dev/null 2>&1; then
    if [ -n "$payload" ]; then
      resp="$(curl -fsSL -X POST "http://127.0.0.1:$PORT/api/tunnel/enable" -H "Content-Type: application/json" -d "$payload" --max-time 120 2>/dev/null || true)"
    else
      resp="$(curl -fsSL -X POST "http://127.0.0.1:$PORT/api/tunnel/enable" --max-time 120 2>/dev/null || true)"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ -n "$payload" ]; then
      resp="$(wget -qO- --header="Content-Type: application/json" --post-data="$payload" "http://127.0.0.1:$PORT/api/tunnel/enable" --timeout=120 2>/dev/null || true)"
    else
      resp="$(wget -qO- --post-data="" "http://127.0.0.1:$PORT/api/tunnel/enable" --timeout=120 2>/dev/null || true)"
    fi
  fi

  if [ -n "$resp" ] && echo "$resp" | grep -q '"ok": *1'; then
    local pubUrl manUrl msg
    pubUrl="$(echo "$resp" | grep -o '"public_url": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)"
    manUrl="$(echo "$resp" | grep -o '"manifest_url": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)"
    msg="$(echo "$resp" | grep -o '"message": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)"
    echo ""
    echo "Cloudflare Tunnel is LIVE!"
    [ -n "$msg" ] && echo "  Status:       $msg"
    [ -n "$pubUrl" ] && echo "  Public URL:   $pubUrl"
    [ -n "$manUrl" ] && echo "  Manifest URL: $manUrl"
    echo ""
  else
    local errMsg
    errMsg="$(echo "$resp" | grep -o '"message": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)"
    echo "Failed to enable tunnel: ${errMsg:-Check server logs in $APP_DIR/storage/debug.log}"
    return 1
  fi
}

do_autostart() {
  local action="${1:-}"
  local autostart_dir="${HOME:-/root}/.config/autostart"
  local desktop_file="$autostart_dir/pencarimovie.desktop"
  local systemd_user_dir="${HOME:-/root}/.config/systemd/user"
  local service_file="$systemd_user_dir/pencarimovie.service"

  if [ "$action" = "off" ] || [ "$action" = "disable" ] || [ "$action" = "remove" ]; then
    mkdir -p "$APP_DIR/storage" 2>/dev/null || true
    touch "$APP_DIR/storage/.no_autostart" 2>/dev/null || true

    # Disable XDG desktop autostart
    rm -f "$desktop_file" 2>/dev/null || true

    # Disable systemd user service if systemctl is available
    if command -v systemctl >/dev/null 2>&1; then
      systemctl --user disable --now pencarimovie.service 2>/dev/null || true
      rm -f "$service_file" 2>/dev/null || true
      systemctl --user daemon-reload 2>/dev/null || true
    fi

    # Disable launchd on macOS
    if [ "$(uname -s)" = "Darwin" ]; then
      local plist="${HOME:-/root}/Library/LaunchAgents/com.pencarimovie.server.plist"
      launchctl unload "$plist" 2>/dev/null || true
      rm -f "$plist" 2>/dev/null || true
    fi

    echo "Auto-start on boot has been DISABLED."
    return 0
  fi

  rm -f "$APP_DIR/storage/.no_autostart" 2>/dev/null || true

  # Enable autostart
  echo "Enabling auto-start on boot..."
  local start_script="$APP_DIR/pencarimovie-linux.sh"
  [ ! -f "$start_script" ] && start_script="$APP_DIR/start.sh"

  if [ "$(uname -s)" = "Darwin" ]; then
    local agents_dir="${HOME:-/root}/Library/LaunchAgents"
    local plist="$agents_dir/com.pencarimovie.server.plist"
    mkdir -p "$agents_dir" 2>/dev/null || true
    cat <<EOF > "$plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.pencarimovie.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/env</string>
        <string>bash</string>
        <string>$start_script</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
EOF
    launchctl unload "$plist" 2>/dev/null || true
    launchctl load -w "$plist" 2>/dev/null || true
    echo "Auto-start on boot ENABLED via macOS LaunchAgent: $plist"
    return 0
  fi

  # 1. Desktop autostart (XDG Desktop Entry - standard across GNOME, KDE, XFCE, etc. Same as 9router)
  mkdir -p "$autostart_dir" 2>/dev/null || true
  cat <<EOF > "$desktop_file"
[Desktop Entry]
Type=Application
Name=PencariMovie Server
Comment=PencariMovie Local Streaming Downloader
Exec=/usr/bin/env bash -c 'cd "$APP_DIR" && bash "$start_script" start'
Hidden=false
NoDisplay=false
X-GNOME-Autostart-enabled=true
EOF
  chmod +x "$desktop_file" 2>/dev/null || true
  echo "  - Desktop autostart created: $desktop_file"

  # 2. Systemd User Service (for headless/CLI Linux servers & VPS without GUI)
  if command -v systemctl >/dev/null 2>&1; then
    mkdir -p "$systemd_user_dir" 2>/dev/null || true
    cat <<EOF > "$service_file"
[Unit]
Description=PencariMovie Server
After=network.target

[Service]
Type=forking
WorkingDirectory=$APP_DIR
Environment=MALLOC_ARENA_MAX=2
Environment=GODEBUG=madvdontneed=1
Environment=GOGC=80
MemoryHigh=1.5G
MemoryMax=2G
ExecStart=/usr/bin/env bash $start_script start
ExecStop=/usr/bin/env bash $APP_DIR/stop.sh
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable pencarimovie.service 2>/dev/null || true
    echo "  - Systemd user service enabled: $service_file"
  fi

  echo "Auto-start on boot has been ENABLED."
}

# auth.json is written by the ENGINE, not by this script. The old PHP-bundled
# path hashed with password_hash() (bcrypt); the Rust engine verifies its own
# `sha256$salt$hash` (20 000 rounds) and also accepts bcrypt, so hand-writing a
# hash here would either be wrong for the engine or duplicate its KDF. Instead
# drive the engine's own loopback API, which is privileged for 127.0.0.1.
http_post_json() {
  local path="$1" body="${2:-{}}"
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required for this command."
    return 1
  fi
  curl -fsS -m 15 -H 'Content-Type: application/json' \
    -X POST -d "$body" "http://127.0.0.1:$PORT$path" 2>/dev/null
}

auth_api_ready() {
  command -v curl >/dev/null 2>&1 || return 1
  curl -fsS -m 8 -o /dev/null "http://127.0.0.1:$PORT/api/auth/status" 2>/dev/null
}

json_field() {
  sed -n 's/.*"'"$1"'":"\([^"]*\)".*/\1/p' | head -1
}

# Legacy installs (PHP + `bin/php`) keep the old direct-write path so their
# bcrypt hash keeps working; new Rust installs go through the API.
do_password() {
  local newpw="${1:-}"
  local auth_file="$APP_DIR/storage/auth.json"
  mkdir -p "$APP_DIR/storage" 2>/dev/null || true

  if [ -z "$newpw" ]; then
    echo "Usage: pms password <new-password>"
    return 1
  fi

  local php_bin=""
  for cand in "$APP_DIR/bin/php" "$(command -v php 2>/dev/null || true)"; do
    [ -n "$cand" ] && [ -x "$cand" ] && php_bin="$cand" && break
  done

  if [ -n "$php_bin" ] && [ -f "$APP_DIR/backend.php" ]; then
    "$php_bin" -r '
      $f = $_SERVER["argv"][1] ?? "";
      $newpw = $_SERVER["argv"][2] ?? "";
      if ($f === "" || $newpw === "") { exit(1); }
      $d = is_file($f) ? (json_decode((string)@file_get_contents($f), true) ?: []) : [];
      $d["password_hash"] = password_hash($newpw, PASSWORD_DEFAULT);
      if (empty($d["token"])) { $d["token"] = bin2hex(random_bytes(16)); }
      $d["enabled"] = true;
      file_put_contents($f, json_encode($d, JSON_UNESCAPED_SLASHES), LOCK_EX);
    ' "$auth_file" "$newpw"
    echo "Password updated."
    return 0
  fi

  # Rust engine: reset to the default over loopback (privileged), then set the
  # requested password using that default as the "current" one.
  if ! auth_api_ready; then
    echo "Server is not running. Start it once with 'pms start', then retry."
    return 1
  fi
  if ! http_post_json "/api/auth/reset" '{}' >/dev/null; then
    echo "Could not reset the password; the server refused the request."
    return 1
  fi
  if http_post_json "/api/auth/password" \
      "{\"current\":\"123456\",\"next\":\"$newpw\"}" >/dev/null; then
    echo "Password updated."
  else
    echo "Password was reset to the default, but setting the new one failed."
    return 1
  fi
}

do_reset_password() {
  do_password "123456"
  echo "Password reset to the default (123456)."
}

do_token() {
  local action="${1:-show}"
  local auth_file="$APP_DIR/storage/auth.json"
  if [ "$action" = "rotate" ]; then
    local php_bin=""
    for cand in "$APP_DIR/bin/php" "$(command -v php 2>/dev/null || true)"; do
      [ -n "$cand" ] && [ -x "$cand" ] && php_bin="$cand" && break
    done
    if [ -n "$php_bin" ] && [ -f "$APP_DIR/backend.php" ]; then
      "$php_bin" -r '
        $f = $_SERVER["argv"][1] ?? "";
        if ($f === "") { exit(1); }
        $d = is_file($f) ? (json_decode((string)@file_get_contents($f), true) ?: []) : [];
        if (empty($d["password_hash"])) { $d["password_hash"] = password_hash("123456", PASSWORD_DEFAULT); }
        $d["token"] = bin2hex(random_bytes(16));
        $d["enabled"] = true;
        file_put_contents($f, json_encode($d, JSON_UNESCAPED_SLASHES), LOCK_EX);
        echo $d["token"], "\n";
      ' "$auth_file"
      echo "Token rotated. Re-install the addon from #addon on every device."
    else
      # Rust engine: `/api/auth/token/rotate` is privileged for loopback and it
      # is the only writer that refreshes the in-process auth cache.
      if ! auth_api_ready; then
        echo "Server is not running. Start it once with 'pms start', then retry."
        return 1
      fi
      local out tok
      if ! out="$(http_post_json "/api/auth/token/rotate" '{}')"; then
        echo "Could not rotate the token; the server refused the request."
        return 1
      fi
      tok="$(printf '%s' "$out" | json_field token)"
      if [ -z "$tok" ]; then
        echo "Token rotation returned no token."
        return 1
      fi
      echo "$tok"
      echo "Token rotated. Re-install the addon from #addon on every device."
    fi
    return 0
  fi
  if [ -f "$auth_file" ]; then
    grep -o '"token":"[^"]*"' "$auth_file" | head -1 | cut -d'"' -f4
  else
    echo "No token yet. Start the server once."
  fi
}

do_uninstall() {
  echo "Stopping PencariMovie Server..."
  do_stop 2>/dev/null || true

  # Clean up autostart
  do_autostart off 2>/dev/null || true

  # Remove CLI wrappers
  local bin_dir="${HOME:-/root}/.local/bin"
  local system_bin="/usr/local/bin"
  local home_bin="${HOME:-/root}/bin"
  for cmd in pms pm pencarimovie; do
    rm -f "$bin_dir/$cmd" 2>/dev/null || true
    rm -f "$system_bin/$cmd" 2>/dev/null || true
    rm -f "$home_bin/$cmd" 2>/dev/null || true
  done

  # Remove the app directory (keeps nothing; storage sessions are removed too)
  if [ -d "$APP_DIR" ]; then
    echo "Removing $APP_DIR ..."
    rm -rf "$APP_DIR"
  fi

  echo "PencariMovie Server has been uninstalled."
}

case "${1:-}" in
  start|--start|"") do_start ;;
  stop|--stop) do_stop ;;
  restart|--restart) do_restart ;;
  tunnel|--tunnel) do_tunnel "${2:-}" ;;
  autostart|--autostart) do_autostart "${2:-}" ;;
  password|--password) do_password "${2:-}" ;;
  reset-password|--reset-password) do_reset_password ;;
  token|--token) do_token "${2:-show}" ;;
  uninstall|--uninstall) do_uninstall ;;
  *) usage ;;
esac
