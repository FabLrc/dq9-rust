# Runs one continuous Web pilot session in a real Chromium (Edge or Chrome) and records it:
# builds the wasm release bundle (the one pages.yml publishes), serves it locally, opens the
# page with the session's query string, chooses the ROM through the page's <input type=file>
# (read by the browser; the local server only serves the bundle), then follows the run until
# `stopped: ...` and writes the console log, the checkpoints (JSONL, the native --checkpoints
# format), the report and the milestone screenshots. See docs/app.md, "Route mode in the
# browser".
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/web-session.ps1 -Out build/evidence/j4-web-pilot/run1
#   powershell -ExecutionPolicy Bypass -File scripts/web-session.ps1 -Out ... `
#       -Query 'newgame=1&pilot=1&fight=1&route=2750&seed=7&mute=1&speed=60&render=0'
#
# `-Rom` defaults to DQ9_ROM, then the single *.nds of rom/. `-SkipBuild` reuses the bundle in
# build/web-session/site. The wasm-bindgen CLI must match Cargo.lock (tools/wasm-bindgen, see
# docs/app.md, "Build and local server"). The Cargo target is the caller's CARGO_TARGET_DIR.
param(
    [Parameter(Mandatory = $true)] [string] $Out,
    [string] $Query = 'newgame=1&pilot=1&fight=1&route=2750&seed=7&mute=1',
    [string] $Rom = '',
    [int] $Port = 8765,
    [int] $DebugPort = 9333,
    [int] $MaxWallSeconds = 14400,
    [int] $StallSeconds = 600,
    [string] $Browser = '',
    [string] $BrowserArgs = '',
    [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot
Set-Location $Repo

if (-not $Rom) { $Rom = $env:DQ9_ROM }
if (-not $Rom) {
    $found = @(Get-ChildItem -LiteralPath (Join-Path $Repo 'rom') -Filter *.nds -ErrorAction SilentlyContinue)
    if ($found.Count -eq 1) { $Rom = $found[0].FullName }
}
if (-not $Rom -or -not (Test-Path -LiteralPath $Rom)) {
    throw 'no ROM: pass -Rom or set DQ9_ROM'
}

$Site = Join-Path $Repo 'build/web-session/site'
if (-not $SkipBuild) {
    $env:RUSTFLAGS = '-D warnings'
    & cargo build --locked -p dq9-app --release --target wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw 'wasm build failed' }
    Remove-Item Env:RUSTFLAGS
    $Target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Repo 'target' }
    $Wasm = Join-Path $Target 'wasm32-unknown-unknown/release/dq9-app.wasm'
    $Bindgen = Join-Path $Repo 'tools/wasm-bindgen/bin/wasm-bindgen.exe'
    if (-not (Test-Path -LiteralPath $Bindgen)) {
        throw "no $Bindgen (cargo install wasm-bindgen-cli --version <Cargo.lock> --locked --root tools/wasm-bindgen)"
    }
    Remove-Item -Recurse -Force $Site -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path (Join-Path $Site 'pkg') | Out-Null
    Copy-Item (Join-Path $Repo 'crates/dq9-app/web/index.html') (Join-Path $Site 'index.html')
    & $Bindgen --target web --no-typescript --out-dir (Join-Path $Site 'pkg') $Wasm
    if ($LASTEXITCODE -ne 0) { throw 'wasm-bindgen failed' }
}

$nodeArgs = @(
    (Join-Path $Repo 'web/session-driver.mjs'),
    '--site', $Site, '--rom', $Rom, '--out', $Out, '--query', $Query,
    '--port', $Port, '--debug-port', $DebugPort,
    '--max-wall', $MaxWallSeconds, '--stall', $StallSeconds
)
if ($Browser) { $nodeArgs += @('--browser', $Browser) }
if ($BrowserArgs) { $nodeArgs += "--browser-args=$BrowserArgs" }
& node @nodeArgs
exit $LASTEXITCODE
