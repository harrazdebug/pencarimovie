<?php
// Post-upgrade verification for MadelineProto 8.8.8 + local patches.
error_reporting(E_ALL);
$base = dirname(__DIR__);
require $base . '/vendor/autoload.php';

$fail = 0;
function chk(string $label, bool $ok, string $extra = ''): void
{
    global $fail;
    if (!$ok) {
        $fail++;
    }
    printf("%-58s %s%s\n", $label, $ok ? 'OK' : 'FAIL', $extra !== '' ? '  ' . $extra : '');
}

// 1. Package + class availability
chk('danog\\MadelineProto\\API class loads', class_exists(\danog\MadelineProto\API::class));
// Derive the expected version from composer.lock instead of hardcoding it, so a
// version bump cannot leave this checker (or the release workflow) comparing
// against a stale constant. The floor is 8.8.0: the patch shape below assumes
// the `getSlow()` field order and the IpcState/entry.php layout of 8.8+.
$lockedMp = '';
$lockRaw = (string) @file_get_contents(__DIR__ . '/../composer.lock');
if ($lockRaw !== '') {
    $lock = json_decode($lockRaw, true);
    foreach (($lock['packages'] ?? []) as $pkg) {
        if (($pkg['name'] ?? '') === 'danog/madelineproto') {
            $lockedMp = ltrim((string) ($pkg['version'] ?? ''), 'v');
            break;
        }
    }
}
$ver = \Composer\InstalledVersions::getPrettyVersion('danog/madelineproto') ?? '?';
chk('madelineproto installed == composer.lock', $lockedMp !== '' && $ver === $lockedMp, "installed=$ver lock=" . ($lockedMp !== '' ? $lockedMp : 'MISSING'));
chk('madelineproto >= 8.8 (patch baseline)', version_compare($ver, '8.8.0', '>='), "got $ver");
$psec = \Composer\InstalledVersions::getPrettyVersion('phpseclib/phpseclib') ?? '?';
chk('phpseclib 4.x installed', str_starts_with($psec, '4.'), "got $psec");

// 2. Critical patch: Ipc::getSlow() must NOT force full boot on Windows
$ipc = new \danog\MadelineProto\Settings\Ipc();
chk('Settings\\Ipc::getSlow() false on Windows (patch)', $ipc->getSlow() === false);
$GLOBALS['FD_FORCE_FULL_BOOT'] = true;
$ipc2 = new \danog\MadelineProto\Settings\Ipc();
chk('Settings\\Ipc::getSlow() true with FD_FORCE_FULL_BOOT', $ipc2->getSlow() === true);
unset($GLOBALS['FD_FORCE_FULL_BOOT']);

// 3. Patch presence checks (source markers)
$markers = [
    'src/Ipc/Runner/ProcessRunner.php' => 'data/data/com.pencarimovie.downloader',
    'src/Ipc/Runner/WebRunner.php' => 'Return false so MadelineProto',
    'src/Ipc/Runner/entry.php' => 'MADELINE_WORKER_MAX_MEMORY',
    'src/MTProtoSession/MsgIdHandler.php' => 'Auto-adjusting session time_delta',
    'src/MTProtoTools/ReferenceDatabase.php' => "is_array(\$data)",
    'src/DoHWrapper.php' => 'resolv.conf',
    'src/Exception.php' => 'operation completed successfully',
    'src/Shutdown.php' => 'API::finalize();',
    'src/Logger.php' => 'Ignore driver suspension',
];
foreach ($markers as $file => $needle) {
    $path = $base . '/vendor/danog/madelineproto/' . $file;
    $src = is_file($path) ? (string) file_get_contents($path) : '';
    chk('patch marker ' . $file, $src !== '' && str_contains($src, $needle));
}

// 4. Sibling package patches
$orm = $base . '/vendor/danog/async-orm/src/Driver/MemoryArray.php';
chk('async-orm MemoryArray cap patch', is_file($orm) && str_contains((string) file_get_contents($orm), '>= 2000'));
$ipcsrv = $base . '/vendor/danog/ipc/lib/IpcServer.php';
$isrc = is_file($ipcsrv) ? (string) file_get_contents($ipcsrv) : '';
chk('danog/ipc TYPE_TCP-first patch', str_contains($isrc, '$types = [self::TYPE_TCP];'));

// 5. Public API surface used by backend.php must still exist
foreach (
    [
        'downloadToBrowser',
        'downloadToCallable',
        'downloadToFile',
        'downloadToDir',
        'getSelf',
        'botLogin',
        'getDownloadInfo',
    ] as $m
) {
    chk('method ' . $m . '()', method_exists(\danog\MadelineProto\API::class, $m) || method_exists(\danog\MadelineProto\MTProtoTools\FilesLogic::class, $m));
}
$conn = new \danog\MadelineProto\Settings\Connection();
$conn->setIpv6(false)->setTimeout(15.0)->setObfuscated(true)->setUseDoH(false);
chk('Connection setters used by fd_boot_madeline()', $conn->getIpv6() === false && $conn->getUseDoH() === false && $conn->getObfuscated() === true);
// 8.9.0 sandboxes browser-served files by default (CSP sandbox + nosniff + normalized mime).
// The opt-out must exist in case a client ever needs the raw behaviour.
chk('Files::setSandboxDownloads() escape hatch', method_exists(\danog\MadelineProto\Settings\Files::class, 'setSandboxDownloads'));

$st = new \danog\MadelineProto\Settings();
$st->getPeer()->setFullInfoCacheTime(300)->setFullFetch(false)->setCacheAllPeersOnStartup(false);
$st->getRpc()->setRpcDropTimeout(180)->setRpcResendTimeout(12);
$st->getFiles()->setDownloadParallelChunks(4);
$st->getLogger()->setMaxSize(10 * 1024 * 1024);
$st->getAppInfo()->setApiId(1)->setApiHash('x');
chk('full Settings pipeline used by fd_boot_madeline()', true);

echo $fail === 0 ? "ALL CHECKS PASSED\n" : "$fail CHECK(S) FAILED\n";
exit($fail === 0 ? 0 : 1);
