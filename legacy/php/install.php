<?php
/**
 * PencariMovie Server - one-file installer for shared hosting (cPanel / Apache).
 *
 * Upload this single file into public_html and open it in a browser:
 *   https://yourdomain.com/install.php
 * It checks the runtime, downloads the deployment ZIP, extracts it in resumable
 * chunks (shared hosts cap max_execution_time at ~60s), writes .htaccess and
 * .release-tag, then deletes itself.
 *
 * You can also upload the ZIP yourself with File Manager -> Extract and skip
 * this script entirely; see SHARED-HOSTING.md.
 *
 * ASCII only on purpose: Windows PowerShell 5.1 misreads UTF-8 .php/.ps1 files
 * without a BOM.
 */

// Change this to the release asset you publish. It must contain index.php,
// backend.php, public/ and vendor/.
const PM_DEFAULT_ARCHIVE = 'https://github.com/aiskendi/pencarimovie-server/releases/latest/download/pencarimovie-shared-hosting.zip';
const PM_ARCHIVE_NAME = 'pm_install.zip';
const PM_DL_SECONDS = 40;

@ini_set('display_errors', '1');
error_reporting(E_ALL);
set_time_limit(0);

header('Content-Type: text/html; charset=utf-8');
$root = __DIR__;
$step = (string) ($_GET['step'] ?? '');

function pm_h($s): string
{
    return htmlspecialchars((string) $s, ENT_QUOTES, 'UTF-8');
}

function pm_page(string $title, string $body): void
{
    echo "<!doctype html><meta charset=utf-8><title>" . pm_h($title) . "</title>";
    echo "<style>body{font:14px/1.55 system-ui,Segoe UI,Arial;background:#141414;color:#ddd;margin:0;padding:28px}"
        . "main{max-width:820px;margin:0 auto}h1{font-size:20px;margin:0 0 14px;color:#ff6b35}"
        . "code,pre{background:#000;padding:2px 5px;border-radius:4px;color:#9fd}"
        . "pre{padding:12px;overflow:auto}a.b{display:inline-block;background:#ff6b35;color:#111;padding:10px 18px;"
        . "border-radius:8px;text-decoration:none;font-weight:600;margin:6px 8px 6px 0}"
        . "table{border-collapse:collapse}td{padding:3px 12px 3px 0}li{margin:2px 0}"
        . ".ok{color:#6c6}.bad{color:#f66}</style><main>" . $body . "</main>";
    exit;
}

function pm_requirements(): array
{
    $r = [];
    $r[] = ['PHP ' . PHP_VERSION, PHP_VERSION_ID >= 80200, 'PHP 8.2 or newer is required'];
    $r[] = [(PHP_INT_SIZE * 8) . '-bit PHP', PHP_INT_SIZE >= 8, 'The Telegram protocol needs 64-bit PHP'];
    foreach (['curl', 'zip', 'openssl', 'mbstring', 'fileinfo'] as $ext) {
        $r[] = ['extension ' . $ext, extension_loaded($ext), 'Enable the php-' . $ext . ' extension'];
    }
    $r[] = ['stream_socket_client', function_exists('stream_socket_client'), 'Needed to reach the Telegram datacenters'];
    $r[] = ['writable directory', is_writable(__DIR__), 'Make the document root writable'];
    return $r;
}

function pm_zip_open(string $path)
{
    if (!is_file($path)) {
        return null;
    }
    $z = new ZipArchive();
    return $z->open($path) === true ? $z : null;
}

function pm_archive_url(): string
{
    $url = trim((string) ($_GET['url'] ?? ''));
    return $url !== '' ? $url : PM_DEFAULT_ARCHIVE;
}

// ---------------------------------------------------------------- step: home
if ($step === '') {
    $rows = '';
    $ok = true;
    foreach (pm_requirements() as [$label, $pass, $fix]) {
        $ok = $ok && $pass;
        $rows .= '<tr><td>' . pm_h($label) . '</td><td class="' . ($pass ? 'ok' : 'bad') . '">'
            . ($pass ? 'OK' : 'FAIL') . '</td><td>' . ($pass ? '' : pm_h($fix)) . '</td></tr>';
    }
    $installed = is_file($root . '/vendor/autoload.php') && is_file($root . '/backend.php');
    $body = '<h1>PencariMovie Server - installer</h1>'
        . '<table>' . $rows . '</table><br>';
    if (!$ok) {
        $body .= '<p class="bad">Fix the failures above with your host before continuing.</p>';
    } elseif ($installed) {
        $body .= '<p class="ok">The app is already installed here.</p>'
            . '<a class="b" href="/">Open the dashboard</a>'
            . '<a class="b" href="?step=delete">Delete this installer</a>';
    } else {
        $body .= '<p>Archive to install:</p><pre>' . pm_h(pm_archive_url()) . '</pre>'
            . '<p>Paste a different URL below if you published your own ZIP.</p>'
            . '<form method="get" action="?"><input name="url" style="width:100%;padding:8px;background:#000;'
            . 'color:#ddd;border:1px solid #333;border-radius:6px" placeholder="' . pm_h(pm_archive_url()) . '">'
            . '<p><a class="b" href="?step=download">Start</a></p></form>';
    }
    pm_page('PencariMovie installer', $body);
}

// ------------------------------------------------------------ step: download
if ($step === 'download') {
    $url = pm_archive_url();
    $zipPath = $root . '/' . PM_ARCHIVE_NAME;
    $started = microtime(true);

    $fh = @fopen($zipPath, 'ab');
    if ($fh === false) {
        pm_page('Download', '<h1>Cannot write here</h1><p>' . pm_h($zipPath) . ' is not writable.</p>');
    }
    $have = (int) @filesize($zipPath);
    $ch = curl_init($url);
    $headers = $have > 0 ? ['Range: bytes=' . $have . '-'] : [];
    curl_setopt_array($ch, [
        CURLOPT_FILE => $fh,
        CURLOPT_FOLLOWLOCATION => true,
        CURLOPT_MAXREDIRS => 5,
        CURLOPT_HTTPHEADER => $headers,
        CURLOPT_TIMEOUT => 600,
        CURLOPT_CONNECTTIMEOUT => 15,
        CURLOPT_LOW_SPEED_LIMIT => 1024,
        CURLOPT_LOW_SPEED_TIME => PM_DL_SECONDS,
        CURLOPT_USERAGENT => 'pencarimovie-installer',
    ]);
    $ok = curl_exec($ch);
    $code = (int) curl_getinfo($ch, CURLINFO_HTTP_CODE);
    $err = curl_error($ch);
    $size = (int) curl_getinfo($ch, CURLINFO_CONTENT_LENGTH_DOWNLOAD);
    $elapsed = round(microtime(true) - $started, 1);
    unset($ch);
    fclose($fh);

    clearstatcache(true, $zipPath);
    $now = (int) @filesize($zipPath);
    $z = pm_zip_open($zipPath);
    if ($z === null) {
        $body = '<h1>Download incomplete</h1>'
            . '<p>Got ' . number_format($now) . ' bytes in ' . $elapsed . 's (HTTP ' . pm_h($code) . ').</p>'
            . ($err !== '' ? '<pre>' . pm_h($err) . '</pre>' : '')
            . '<p>The host cut the transfer (its time or size limit). Click Continue - it resumes where it '
            . 'stopped instead of starting over.</p>'
            . '<a class="b" href="?step=download&url=' . rawurlencode($url) . '">Continue download</a>'
            . '<a class="b" href="?">Back</a>';
        pm_page('Download', $body);
    }
    $total = $z->numFiles;
    $z->close();
    pm_page('Download', '<h1>Archive ready</h1><p>' . number_format($now) . ' bytes, '
        . number_format($total) . ' entries, ' . $elapsed . 's.</p>'
        . '<a class="b" href="?step=extract&from=0&url=' . rawurlencode($url) . '">Extract</a>');
}

// ------------------------------------------------------------- step: extract
if ($step === 'extract') {
    $zipPath = $root . '/' . PM_ARCHIVE_NAME;
    $z = pm_zip_open($zipPath);
    if ($z === null) {
        pm_page('Extract', '<h1>Archive missing</h1><a class="b" href="?step=download">Download first</a>');
    }
    $total = $z->numFiles;
    $from = max(0, (int) ($_GET['from'] ?? 0));
    $ok = 0;
    $fail = [];
    $t0 = microtime(true);
    $i = $from;
    // One pass is enough on this class of host (4 200 files took ~5s), but the
    // budget below still hands back a Continue link if a slow disk runs long.
    for (; $i < $total; $i++) {
        if ((microtime(true) - $t0) > 35) {
            break;
        }
        $raw = $z->getNameIndex($i);
        if ($raw === false || $raw === '' || str_contains($raw, '..')) {
            continue;
        }
        $name = ltrim(str_replace('\\', '/', $raw), '/');
        if (str_ends_with($name, '/')) {
            @mkdir($root . '/' . $name, 0755, true);
            continue;
        }
        $dir = $root . '/' . dirname($name);
        if (!is_dir($dir)) {
            @mkdir($dir, 0755, true);
        }
        if (@$z->extractTo($root, $raw)) {
            $ok++;
        } else {
            $fail[] = $name;
        }
    }
    $z->close();
    $body = '<h1>Extracting</h1><p>' . number_format($ok) . ' of ' . number_format($total)
        . ' entries written (' . number_format($from) . ' - ' . number_format($i) . ') in '
        . round(microtime(true) - $t0, 1) . 's.</p>';
    if ($fail) {
        $body .= '<p class="bad">' . count($fail) . ' failed, first: ' . pm_h($fail[0]) . '</p>';
    }
    if ($i >= $total) {
        $body .= '<a class="b" href="?step=finish">Finish</a>';
    } else {
        $body .= '<a class="b" href="?step=extract&from=' . $i . '">Continue</a>';
    }
    pm_page('Extracting', $body);
}

// -------------------------------------------------------------- step: finish
if ($step === 'finish') {
    // The chunked pass can stop on its 35s budget; one native bulk extract here
    // guarantees nothing is missing before the archive is deleted.
    $zipPath = $root . '/' . PM_ARCHIVE_NAME;
    $z = pm_zip_open($zipPath);
    if ($z !== null) {
        @$z->extractTo($root);
        $z->close();
    }
    foreach (['storage', 'storage/cache', 'storage/sessions', 'storage/reports', 'storage/bin'] as $d) {
        @mkdir($root . '/' . $d, 0755, true);
    }
    $tag = 'v2.7.9';
    @file_put_contents($root . '/.release-tag', $tag);

    $htaccess = <<<'HT'
<IfModule mod_rewrite.c>
    RewriteEngine On
    RewriteRule ^(storage|vendor|bin|patches|scripts|src|state|plans|android|fastlane|fdroid)(/|$) - [F,L]
    RewriteRule (^|/)\.(git|github)(/|$) - [F,L]
    RewriteRule ^public/ - [L]
    RewriteRule ^index\.php$ - [L]
    RewriteRule ^ index.php [L]
</IfModule>

<FilesMatch "^(backend\.php|router\.php|install\.php|composer\.(json|lock)|package(-lock)?\.json|AGENTS\.md|CHANGELOG\.md|MadelineProto\.log)$">
    Require all denied
</FilesMatch>
HT;
    @file_put_contents($root . '/.htaccess', $htaccess);
    @unlink($root . '/' . PM_ARCHIVE_NAME);

    $ready = is_file($root . '/vendor/autoload.php') && is_file($root . '/index.php');
    $body = '<h1>' . ($ready ? 'Installed' : 'Almost there') . '</h1>'
        . '<p>' . ($ready ? 'The app is in place.' : 'vendor/ or index.php is missing - the archive was incomplete.') . '</p>'
        . '<ol><li>Open <a href="/">the dashboard</a>.</li>'
        . '<li>Change the server password (default <code>123456</code>) in Settings.</li>'
        . '<li>Paste your bot token and press Connect - the first connect takes 15-30s.</li>'
        . '<li>Open <code>#addon</code> to install the Stremio/Nuvio addon.</li></ol>'
        . '<p>The installer and its archive are deleted now. If they are still listed in File Manager, '
        . 'delete <code>install.php</code> and <code>' . pm_h(PM_ARCHIVE_NAME) . '</code> yourself - '
        . '.htaccess already blocks both from the web.</p>';
    // Some hosts run the document root read-only for PHP even though it accepts
    // file writes, so this is best-effort; .htaccess is the real guard.
    @unlink(__FILE__);
    @unlink($root . '/' . PM_ARCHIVE_NAME);
    pm_page('Installed', $body);
}

// -------------------------------------------------------------- step: delete
if ($step === 'delete') {
    @unlink(__FILE__);
    @unlink($root . '/' . PM_ARCHIVE_NAME);
    pm_page('Removed', '<h1>Installer removed</h1><a class="b" href="/">Open the dashboard</a>');
}

pm_page('Installer', '<h1>Unknown step</h1><a class="b" href="?">Start over</a>');
