# Install PencariMovie on a normal webhost (cPanel / FTP)

No VPS, no SSH, no terminal, no Composer. You drag **one file** onto your host, open it in a
browser, and it installs itself.

---

## 0. Before you start

You need two things from your hosting control panel:

| What                           | Where to find it                                |
| ------------------------------ | ----------------------------------------------- |
| Your website address           | the panel dashboard, e.g. `http://mysite.rf.gd` |
| FTP host / username / password | "FTP Accounts" or "FTP Details"                 |

**You do not need a bot token.** The app provisions a guest bot for you on first use — see
step 4. A token is only needed if you want to use your own bot.

The host must run **PHP 8.2 or newer, 64-bit**. Every cPanel host does; the installer tells you
if it does not.

---

## 1. Get the two files

Download the release package (one click):

```
https://github.com/aiskendi/pencarimovie-server/releases/latest/download/pencarimovie-shared-hosting.zip
```

Unzip it on your computer. You now have a folder with `install.php`, `index.php`,
`backend.php`, `public/`, `vendor/` and more.

You only need **`install.php`** — keep the rest of the ZIP where it is; the installer will
fetch its own copy on the server.

> That link 404s until a release containing `pencarimovie-shared-hosting.zip` is published
> (the Release workflow builds and uploads it). If it 404s, open the repo's **Releases** page
> and take the latest `pencarimovie-shared-hosting.zip` from there.

---

## 2. Upload it with FTP

1. Install **FileZilla** (free) if you do not have it.
2. Top bar, fill in and press **Quickconnect**:

   | Field    | What to type                               |
   | -------- | ------------------------------------------ |
   | Host     | `ftpupload.net` (or what your panel shows) |
   | Username | your FTP username                          |
   | Password | your FTP password                          |
   | Port     | `21`                                       |

3. On the right side you will see your host's folders. Open the one that holds your website:

   | Panel                              | Folder            |
   | ---------------------------------- | ----------------- |
   | iFastNet / ByetHost / InfinityFree | **`htdocs`**      |
   | cPanel (most hosts)                | **`public_html`** |
   | Plesk                              | **`httpdocs`**    |

   If you are not sure, pick the folder that already contains `index.php` or `index.html`.

4. Drag **`install.php`** from the left side into that folder.
5. Close FileZilla.

> FTP clients cannot unzip an archive, which is why you upload the installer instead of the
> package. The installer downloads the package itself — on the host, over HTTP.

**Alternative, if your panel has a File Manager** (cPanel → File Manager): upload
`pencarimovie-shared-hosting.zip` and press **Extract**. That skips the installer entirely —
the site is ready when the extract finishes.

**Alternative, pure FTP, no installer:** drag the whole unzipped folder into `htdocs`. It is
4 209 files and takes a while, but it needs nothing from the host beyond plain FTP. Upload
`.htaccess` last (FileZilla hides dot-files: enable _Server → Force showing hidden files_).

---

## 3. Run the installer in your browser

Open:

```
https://yourdomain.com/install.php
```

- It lists PHP version, extensions and writability. All green → press **Start**.
- It downloads the app (~12 MB), unpacks it, writes `.htaccess` and a version stamp, and
  deletes itself. A second or two on a decent host.
- If the host cuts the download, press **Continue download** — it resumes, it does not start
  over.

When it says **Installed**, the site is live.

---

## 4. First run (1 minute)

1. Open `https://yourdomain.com/`.
2. The **password is `123456`** — change it in Settings right now.
3. **Nothing else to type.** The app provisions a guest bot by itself the first time it needs
   one; that handshake is the **~15 s** wait (measured 14.6 s and 14.9 s on the two hosts
   tested). A bot name appears in the corner when it is ready.
   - Shared hosting cannot start a background process, so the guest bot is created on your
     **first play**, not on page load.
   - If you see _"Connecting guest bot in progress. Please refresh or try again in a few
     seconds."_, wait ~15 s and press play again. Nothing is broken.
4. Open a movie → **Stream** plays in the browser, **Download** saves it.

**Optional:** to use your own bot instead of the guest one, put a token from
[@BotFather](https://t.me/BotFather) in Settings → Connect.

---

## 5. Optional: the Stremio / Nuvio addon

Dashboard → **`#addon`** → copy the URL (it looks like
`https://yourdomain.com/8f3a…/manifest.json`) → paste into Stremio → Install.

**Free hosts cannot do this part.** See below.

---

## Which host to use

| Host                                        | Website works | Stremio addon works |
| ------------------------------------------- | ------------- | ------------------- |
| Your own VPS, Raspberry Pi, Docker          | ✅            | ✅                  |
| **Paid** cPanel host                        | ✅            | ✅                  |
| **Free** iFastNet / ByetHost / InfinityFree | ✅            | ❌                  |

Free hosts put a "please enable JavaScript" bot check in front of PHP. A browser passes it; the
Stremio/Nuvio addon cannot, so installing the addon shows
`unexpected json token at offset 21`. Nothing in the app can fix that — browsing and pressing
Stream/Download in a browser still work fine.

A Cloudflare Worker cannot rescue it either: Cloudflare's requests to a free iFastNet host are
refused (`HTTP 520`), because those hosts only accept residential IPs. (The `__test` cookie
itself is solvable — AES-128-CBC on the three constants in the challenge page — but the Worker
still has to reach the origin, and it cannot.)

---

## What it looks like in practice

Measured on two free hosts (PHP 8.4, Apache, all shell/IPC functions disabled):

| Step                             | Time                            |
| -------------------------------- | ------------------------------- |
| Installer unpack (`4 208` files) | **1.8 s**                       |
| Dashboard loads                  | 0.6 s                           |
| First bot connect                | **14.6–14.9 s**                 |
| Stream list for a title          | 2.4–4.0 s                       |
| First video chunk (256 KB)       | 13–18 s (includes the bot boot) |
| Second chunk (2 MB)              | 4.2–4.5 s → **~465–495 KB/s**   |

That speed is enough for 1080p; 4K will buffer. Every video request reconnects the bot, which is
why the first chunk is slow — that is the price of a host without background processes.

---

## If something goes wrong

| What you see                                            | What it means                            | Fix                                                            |
| ------------------------------------------------------- | ---------------------------------------- | -------------------------------------------------------------- |
| `unexpected json token at offset 21`                    | host's bot wall, not the app             | use a paid host for the addon, or browse in a browser          |
| `Update required (vX < vY)` on every title              | the version stamp is missing             | re-run the installer                                           |
| `Password required`                                     | you opened a plain URL                   | use the `/<token>/` URL from `#addon`                          |
| `Could not create IPC server: … stream_socket_server()` | the package was not built from this repo | use the official release ZIP                                   |
| `Call to undefined function curl_multi_exec()`          | same — old `backend.php`                 | use the official release ZIP                                   |
| "Streams Filtered Out" on a title that has files        | your stream filters hide them            | `#addon` → Streams Configuration → clear **required keywords** |
| "No Streams Found"                                      | that title/episode has no matching file  | try another title                                              |
| Page is blank / 500                                     | a PHP error is hidden                    | add `?debug=1` to an API URL, or read `storage/debug.log`      |

---

## Updating later

Upload `install.php` again and open it — it sees the app already installed and offers
**Delete this installer**. To upgrade, re-upload the new ZIP and Extract over the top, but keep
`storage/`: that is your bot session. Delete it and the app simply provisions a fresh guest bot
on the next play.

---

<details>
<summary>Manual route (only if the installer cannot reach the internet)</summary>

1. Upload [`storage/_host_upload.php`](storage/_host_upload.php) to the same folder and rename it
   `uploader.php`.
2. From a computer with the ZIP:
   `curl --data-binary @pencarimovie-shared-hosting.zip "https://yourdomain.com/uploader.php?part=0&file=pm_install.zip"`
   (max ~19 MB per POST — split with `part=1`, `part=2` if needed).
3. Upload `install.php`, open `?step=extract&from=0`, then `?step=finish`.
4. Delete `uploader.php` and `install.php`.

[`storage/_host_probe.php`](storage/_host_probe.php) is a one-screen report of what the host
allows (PHP version, limits, disabled functions, Telegram reachability). Upload it as
`probe.php`, open it once, delete it.

</details>
