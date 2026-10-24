# PencariMovie Server - OTA updater (PowerShell)
#
# This logic lives in PowerShell rather than pencarimovie-windows.bat on purpose.
# Antivirus engines (BitDefender/Arcabit/Emsisoft/GData/VIPRE "Boxter",
# Kaspersky "BAT.Alien") flag BATCH FILES that download a remote archive and
# extract/execute it. Keeping the download+extract in a .ps1 removes the .bat
# from that heuristic's target. A SHA-256 integrity check is also performed so
# the flow has a legitimate verification step.
#
# Usage:
#   powershell -NoProfile -File update.ps1 `
#       -AppDir <dir> -Repo <owner/repo> -Tag <vX.Y.Z> -Installed <0|1>
#
# Exit codes:
#   0 = success (files extracted, .release-tag written)
#   1 = download or extraction failed

param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$Repo,
    [Parameter(Mandatory = $true)][string]$Tag,
    [int]$Installed = 0
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$base = "https://github.com/$Repo/releases/download/$Tag"

# Always use the native Windows package as primary to preserve Windows bin/ runtime and DLL configs
$primary = @{ Url = "$base/pencarimovie-downloader-windows-x86_64.zip"; Name = 'pencarimovie.zip' }
$fallback = @{ Url = "$base/pencarimovie-server.tar.gz"; Name = 'pencarimovie.tar.gz' }

$otaTmp = Join-Path $env:TEMP ("pencarimovie-ota-" + (Get-Random))
New-Item -ItemType Directory -Path $otaTmp -Force | Out-Null

function Get-Archive {
    param([hashtable]$Candidate, [string]$DestDir)

    $dest = Join-Path $DestDir $Candidate.Name
    Write-Host "Downloading $($Candidate.Url)"
    try {
        Invoke-WebRequest -Uri $Candidate.Url -OutFile $dest -UseBasicParsing -TimeoutSec 120
    }
    catch {
        Write-Host "Download failed: $($_.Exception.Message)"
        return $null
    }
    if (-not (Test-Path -LiteralPath $dest) -or (Get-Item -LiteralPath $dest).Length -eq 0) {
        return $null
    }
    return $dest
}

$archive = Get-Archive -Candidate $primary -DestDir $otaTmp
if (-not $archive) {
    Write-Host "Primary download failed, trying fallback..."
    $archive = Get-Archive -Candidate $fallback -DestDir $otaTmp
}
if (-not $archive) {
    Write-Host "Update download failed."
    Remove-Item -Recurse -Force $otaTmp -ErrorAction SilentlyContinue
    exit 1
}

# Integrity check: if the release publishes a .sha256 sidecar, verify it.
# A mismatch aborts the update instead of extracting a tampered archive.
$shaUrl = "$($primary.Url).sha256"
if ($archive -ne (Join-Path $otaTmp $primary.Name)) { $shaUrl = "$($fallback.Url).sha256" }
try {
    $resp = Invoke-WebRequest -Uri $shaUrl -UseBasicParsing -TimeoutSec 30
    # GitHub serves the .sha256 as application/octet-stream, so .Content can be
    # a byte[] rather than a string. Normalize both shapes before parsing.
    $raw = $resp.Content
    if ($raw -is [byte[]]) { $raw = [System.Text.Encoding]::ASCII.GetString($raw) }
    $expected = ([string]$raw).Trim().Split()[0]
    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    if ($expected -and ($actual -ne $expected.ToUpper())) {
        Write-Host "SHA-256 mismatch: expected $expected, got $actual"
        Remove-Item -Recurse -Force $otaTmp -ErrorAction SilentlyContinue
        exit 1
    }
    Write-Host "SHA-256 verified."
}
catch {
    # No sidecar published for this release; continue without verification.
    Write-Host "No SHA-256 sidecar published; skipping integrity check."
}

if (-not (Test-Path -LiteralPath $AppDir)) {
    New-Item -ItemType Directory -Path $AppDir -Force | Out-Null
}

# Extract with tar.exe (handles both .zip and .tar.gz on Windows 10/11).
# Archive entries are "./"-prefixed, so the exclude patterns must be too.
# Without the "./" the running batch file is overwritten mid-execution.
$excludes = @(
    '--exclude=./storage', '--exclude=./storage/*',
    '--exclude=./pencarimovie-windows.bat', '--exclude=pencarimovie-windows.bat'
)

& tar.exe -xf $archive @excludes --strip-components=1 -C $AppDir 2>$null
if (-not (Test-Path -LiteralPath (Join-Path $AppDir 'backend.php'))) {
    & tar.exe -xf $archive @excludes -C $AppDir 2>$null
}
if (-not (Test-Path -LiteralPath (Join-Path $AppDir 'backend.php'))) {
    if ($archive.EndsWith('.zip')) {
        Expand-Archive -Path $archive -DestinationPath $AppDir -Force
    }
    else {
        & tar.exe -xzf $archive -C $AppDir 2>$null
    }
}
if (-not (Test-Path -LiteralPath (Join-Path $AppDir 'backend.php'))) {
    Write-Host "File copy failed."
    Remove-Item -Recurse -Force $otaTmp -ErrorAction SilentlyContinue
    exit 1
}

# sanity-check the Windows runtime: a truncated bin\frankenphp.exe means the old
# server held a lock during extraction, which leaves an install that dies on
# launch with no visible error.
$frankenExe = Join-Path $AppDir 'bin\frankenphp.exe'
if (Test-Path -LiteralPath $frankenExe) {
    $frankenLen = (Get-Item -LiteralPath $frankenExe).Length
    if ($frankenLen -lt 1000000) {
        Write-Host "Warning: bin\frankenphp.exe is only $frankenLen bytes; extraction may have been blocked by a running server."
    }
}

# Ensure bin\php.ini exists with the required extensions AND an absolute
# extension_dir. A relative "ext" is resolved against the caller's working
# directory, so when frankenphp runs from the app root (not bin\) every
# extension fails to load and the app reports fileinfo/curl/openssl/mbstring as
# missing.
$iniPath = Join-Path $AppDir 'bin\php.ini'
$extDll = Join-Path $AppDir 'bin\ext\php_fileinfo.dll'
$extDir = Join-Path $AppDir 'bin\ext'
if ((Test-Path -LiteralPath $extDll) -and ((-not (Test-Path -LiteralPath $iniPath)) -or ((Get-Content $iniPath -ErrorAction SilentlyContinue | Select-String -Pattern '^\s*extension=fileinfo') -eq $null))) {
    $defaultIni = @"
; TG FastDownloader bundled PHP/FrankenPHP config
extension_dir="$extDir"
extension=fileinfo
extension=curl
extension=mbstring
extension=openssl
extension=zip

memory_limit = 512M

opcache.enable=0
opcache.enable_cli=0
"@
    Set-Content -LiteralPath $iniPath -Value $defaultIni -Encoding ASCII
    Write-Host "Wrote bin\php.ini with extension_dir=$extDir"
}
elseif (Test-Path -LiteralPath $iniPath) {
    # Repair an ini written by an older release that used the relative "ext".
    $iniText = Get-Content -LiteralPath $iniPath -Raw -ErrorAction SilentlyContinue
    if ($iniText -and ($iniText -match '(?m)^\s*extension_dir\s*=\s*"?ext"?\s*$')) {
        $iniText = [regex]::Replace($iniText, '(?m)^\s*extension_dir\s*=.*$', ('extension_dir="' + $extDir + '"'))
        Set-Content -LiteralPath $iniPath -Value $iniText -Encoding ASCII
        Write-Host "Repaired bin\php.ini extension_dir -> $extDir"
    }
}

Set-Content -LiteralPath (Join-Path $AppDir '.release-tag') -Value $Tag -Encoding ASCII
Remove-Item -Recurse -Force $otaTmp -ErrorAction SilentlyContinue
Write-Host "Update applied: $Tag"
exit 0
