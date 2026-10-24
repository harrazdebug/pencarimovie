# `legacy/php/` — the retired PHP + FrankenPHP engine

This is the original PencariMovie Server: a PHP backend (`backend.php`) driving
MadelineProto, served by a bundled FrankenPHP runtime from `bin/`, with a
Windows system-tray helper, POSIX/Termux launchers, a Composer dependency tree and
a shared-hosting installer. It was replaced by the Rust server in
[`server/`](../../server), which embeds the same dashboard (`public/` at the repo
root) into one self-contained binary.

**Nothing here is built, released or deployed any more.**

- `.github/workflows/release.yml` has no PHP job (its header explains why).
- The Android app runs the Rust engine ([`android/pencarimovie-rust`](../../android/pencarimovie-rust)).
- Docker / Heroku / Render deploy the **Rust** image
  ([`Dockerfile`](../../Dockerfile), [`Dockerfile.release`](../../Dockerfile.release),
  [`heroku.yml`](../../heroku.yml), [`render.yaml`](../../render.yaml)) — those files
  deliberately stayed at the repository root because they build the product.

## What is in here

| Path                                     | What it was                                                                                                                                                         |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- | -------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| `backend.php`, `index.php`, `router.php` | The whole HTTP surface: `/api/*`, the Stremio/Eclipse addon, the download streamer.                                                                                 |
| `Caddyfile`                              | FrankenPHP server config (`:8088` HTTP listener).                                                                                                                   |
| `bin/`                                   | Bundled FrankenPHP / PHP runtime wrapper (`bin/php`, `bin/php.ini*`).                                                                                               |
| `patches/`                               | MadelineProto vendor patches (applied by `cweagans/composer-patches`).                                                                                              |
| `composer.json`, `composer.lock`         | The MadelineProto dependency tree.                                                                                                                                  |
| `start\*.sh                              | bat`, `stop.sh                                                                                                                                                      | bat`, `restart\*.sh                     | bat`, `start-hidden.ps1`, `tray.ps1`, `tray.ico`, `tray.png`, `tunnel-spawn.ps1`, `auth-write.ps1` | Windows tray + cross-platform launchers. |
| `install\*.sh                            | bat`, `install.php`                                                                                                                                                 | Dependency / shared-hosting installers. |
| `.htaccess`                              | Apache/cPanel routing + deny rules for `storage/`, `vendor/`, `bin/`.                                                                                               |
| `package.json`                           | `npm start` mapped to `start.bat`.                                                                                                                                  |
| `docker-entrypoint.sh`                   | The FrankenPHP container entrypoint (thread/memory autotune).                                                                                                       |
| `scripts/`                               | The PHP release packagers (`package-unix.sh`, `package-windows.bat`, `build-release.*`, `download-frankenphp.ps1`, `verify-madeline-patch.php`, `release-apk.bat`). |
| `tools/ledger.php`                       | Generator for the port audit ledger.                                                                                                                                |
| `test_kudrat_series.php`                 | The 18-check series-matcher regression harness.                                                                                                                     |
| `SHARED-HOSTING.md`                      | cPanel/ByetHost deployment tutorial.                                                                                                                                |

## Two files that did NOT move: `tray.ico` / `tray.png`

They stayed at the repository root. `release.yml` stages them into **every Rust
release artifact** (`cp tray.ico "stage/$NAME/pencarimovie.ico"`), so they are
product assets, not legacy ones — moving them here silently stripped the tray icon
from all five tarballs (caught by running the packaging path end-to-end).
`legacy/php/tray.ps1` looks for them next to itself and therefore falls back to a
stock icon if you ever run this tree standalone.

## Things that moved here that you might not expect

- **`SHARED-HOSTING.md`** — it documents installing the PHP build on cPanel. It is
  history now, so it travels with the code it describes.
- **`docker-entrypoint.sh`** — PHP/FrankenPHP specific (it exports
  `FRANKENPHP_NUM_THREADS`, `PHP_MEMORY_LIMIT`, …). The Rust image has no entrypoint
  script at all; the binary is its own entrypoint.
- **`package.json`** — its `main` is `router.php` and `start` is `start.bat`.
- **`bin/`** — the bundled FrankenPHP/PHP runtime _wrapper_. The Rust release
  tarballs contain no `bin/`.

## Known limitation: this tree is archived, not runnable in place

`backend.php` resolves `public/`, `storage/`, `vendor/` and `.release-tag` relative
to its own `__DIR__`. Only `bin/` moved with it, so from this location it will not
find the dashboard in `../../public`, the state in `../../storage`, or a Composer
`vendor/` tree (which is untracked and stayed at the repository root).

That is deliberate: fixing it would mean editing ~20 `__DIR__`-relative paths in a
20 000-line file for a tree nothing deploys. **If you ever need to run the PHP engine
again, copy this directory into a standalone checkout** (restoring `public/`,
`storage/`, `vendor/` beside `backend.php`) rather than running it from inside
`legacy/`.

## The installers did NOT move

`pencarimovie-linux.sh`, `pencarimovie-termux.sh`, `pencarimovie-windows.bat`,
`pencarimovie-libwrt.sh`, `pencarimovie-docker.sh` and `update.ps1` stayed at the
repository root on purpose:

1. their **root URLs are a public contract** — `curl -fsSL telegra.my/linux | bash`
   and friends are inlined by [`cloudflare-worker-telegra-my.js`](../../cloudflare-worker-telegra-my.js),
   and the raw `raw.githubusercontent.com/aiskendi/pencarimovie-server/main/<file>`
   paths are documented to users;
2. they are the OS package installers for the **current** product, not for PHP.

They still contain PHP-era fast paths (they will start a bundled `frankenphp`/
`php -S` when one is present in the install directory). That is pre-existing
behaviour and was left untouched by this refactor — see the follow-up note in the
root `README.md` § Repository layout.
