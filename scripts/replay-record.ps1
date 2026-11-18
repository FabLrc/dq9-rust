# Replays a recorded human session (`dq9-app --record`, docs/app.md, § "Recording a human
# session") headless in release mode and checks it: every state digest of the record must match
# (a mismatch names the step and the field), and with -Expect the event must end.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/replay-record.ps1 -Record build/records/session.dq9rec
#   powershell -ExecutionPolicy Bypass -File scripts/replay-record.ps1 -Record session.dq9rec -Expect 2750 -Checkpoints out.jsonl
#
# -Record      the file `dq9-app --new-game --seed N --record <file>` wrote (required).
# -Expect      an event id; the replay stops when it ends and fails if it never does.
# -Save        the save file the session started from, when it started from one.
# -Checkpoints where to write the story checkpoints (JSONL, the `--checkpoints` format); by
#              default build/replay-record/<record name>-checkpoints.jsonl.
# -Rom         the ROM; default DQ9_ROM, else the only *.nds of rom/ (crates/testrom's lookup).
#
# Output: the stop line (`stopped: event N ended ...` or `stopped: record ended ...`), the
# digest count and the party; the full run log is build/replay-record/<record name>.stdout.txt.
# Exit code 0 when the replay matched every digest (and reached -Expect), 1 otherwise (digest
# mismatch, event not reached, a record or ROM that does not match, build failure).
param(
    [Parameter(Mandatory = $true)] [string] $Record,
    [string] $Expect,
    [string] $Save,
    [string] $Checkpoints,
    [string] $Rom
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path (Join-Path $Root 'build') 'replay-record'

function Stop-Replay([string] $Message) {
    Write-Output "replay-record: $Message"
    exit 1
}

function Find-Rom {
    # Same lookup as crates/testrom/src/lib.rs (find) and scripts/parity.ps1.
    if ($env:DQ9_ROM) {
        if (-not (Test-Path -LiteralPath $env:DQ9_ROM -PathType Leaf)) {
            Stop-Replay "DQ9_ROM points to a missing file: $env:DQ9_ROM"
        }
        return (Resolve-Path -LiteralPath $env:DQ9_ROM).Path
    }
    foreach ($Dir in @((Join-Path $Root 'rom'), $Root)) {
        if (-not (Test-Path -LiteralPath $Dir -PathType Container)) { continue }
        $Found = @(Get-ChildItem -LiteralPath $Dir -File | Where-Object { $_.Extension -eq '.nds' })
        if ($Found.Count -gt 1) { Stop-Replay "several ROMs in $Dir; pass -Rom or set DQ9_ROM" }
        if ($Found.Count -eq 1) { return $Found[0].FullName }
    }
    Stop-Replay 'no ROM found (pass -Rom, set DQ9_ROM or put one *.nds in rom/)'
}

function Format-Arg([string] $Arg) {
    if ($Arg -notmatch '[\s"]') { return $Arg }
    return '"' + ($Arg -replace '"', '\"') + '"'
}

if (-not (Test-Path -LiteralPath $Record -PathType Leaf)) { Stop-Replay "no record at $Record" }
$Record = (Resolve-Path -LiteralPath $Record).Path
if ($Expect -and $Expect -notmatch '^\d+$') { Stop-Replay "-Expect must be an event id, got '$Expect'" }
if ($Save) {
    if (-not (Test-Path -LiteralPath $Save -PathType Leaf)) { Stop-Replay "no save at $Save" }
    $Save = (Resolve-Path -LiteralPath $Save).Path
}
if ($Rom) {
    if (-not (Test-Path -LiteralPath $Rom -PathType Leaf)) { Stop-Replay "no ROM at $Rom" }
    $Rom = (Resolve-Path -LiteralPath $Rom).Path
} else {
    $Rom = Find-Rom
}

Push-Location $Root
try {
    Write-Output 'replay-record: cargo build --release -p dq9-app'
    & cargo build --release -p dq9-app
    if ($LASTEXITCODE -ne 0) { Stop-Replay "cargo build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
$App = Join-Path (Join-Path $TargetDir 'release') 'dq9-app'
if (Test-Path -LiteralPath "$App.exe" -PathType Leaf) { $App = "$App.exe" }
if (-not (Test-Path -LiteralPath $App -PathType Leaf)) { Stop-Replay "no binary at $App after the build" }

if (-not (Test-Path -LiteralPath $OutDir -PathType Container)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }
$Name = [IO.Path]::GetFileNameWithoutExtension($Record)
if (-not $Checkpoints) { $Checkpoints = Join-Path $OutDir "$Name-checkpoints.jsonl" }
$Stdout = Join-Path $OutDir "$Name.stdout.txt"
$Stderr = Join-Path $OutDir "$Name.stderr.txt"

$Command = @('--rom', $Rom, '--play-record', $Record, '--checkpoints', $Checkpoints)
if ($Expect) { $Command += @('--expect-event', $Expect) }
if ($Save) { $Command += @('--save', $Save) }
Write-Output "replay-record: $Record"
$Process = Start-Process -FilePath $App -ArgumentList (($Command | ForEach-Object { Format-Arg $_ }) -join ' ') `
    -NoNewWindow -Wait -PassThru -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
$Lines = @(Get-Content -LiteralPath $Stdout)
foreach ($Line in ($Lines | Where-Object { $_ -match '^(stopped:|digests:|party )' })) { Write-Output "  $Line" }
if ($Process.ExitCode -ne 0) {
    # The process names why on stderr (`Error: digest mismatch at step N ...`).
    $Why = @(Get-Content -LiteralPath $Stderr | Where-Object { $_ -match 'Error|mismatch|did not end|record' } | Select-Object -Last 3)
    foreach ($Line in $Why) { Write-Output "  $Line" }
    Stop-Replay "FAILED (exit $($Process.ExitCode), log $Stdout, $Stderr)"
}
if ($Expect -and -not ($Lines | Where-Object { $_ -match "^stopped: event $Expect ended" })) {
    Stop-Replay "FAILED: the replay did not stop on 'event $Expect ended' (log $Stdout)"
}
Write-Output "replay-record: ok (checkpoints $Checkpoints)"
exit 0
