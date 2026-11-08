# Automated parity with the original game: replays every reference of a references file on the
# headless runtime and compares the story checkpoints with the original's, aligned by story
# point through scripts/compare-runs.ps1.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/parity.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/parity.ps1 -References build/evidence/parity/references.json -Rom rom/game.nds
#
# -References defaults to build/evidence/parity/references.json (local, ignored: it points to
# original checkpoints, which are game data). -Rom defaults to the lookup of crates/testrom:
# DQ9_ROM, else the only *.nds of rom/, else the only *.nds of the repository root.
#
# References file (UTF-8 JSON, paths relative to its own directory):
#   { "version": 1, "references": [ {
#       "name": "control-1.1.2",                       # [A-Za-z0-9._-]+, unique
#       "description": "...",                          # optional
#       "inputs":   { "path": "...", "sha256": "...", "source": "..." },  # --trace-inputs JSONL
#       "original": { "path": "...", "sha256": "...", "source": "..." },  # original checkpoints
#       "args": ["--new-game", "--mute"],              # launch options of dq9-app
#       "heroTolerance": 0.01,                         # compare-runs -HeroTolerance (0 = off)
#       "accepted": [ { "flag": 4135, "only": "original", "reason": "..." } ]
#   } ] }
# The script adds `--rom <rom> --headless --replay <inputs> --checkpoints <out>` itself; `args`
# must not repeat them. `source` is provenance text only.
#
# Each reference: both files must exist and match their sha256 before use; the runtime run
# (one per distinct inputs + args, release dq9-app built once) is compared with the original
# by compare-runs.ps1. An accepted deviation names one global flag present on one side only
# ("original" or "runtime") with its justification: the flag is removed from that side's
# checkpoints before the comparison, so any other difference still fails, and the flag
# reappearing on the other side fails too. An accepted flag that is absent from its side is
# stale and fails. Every original checkpoint must be aligned (compare-runs reports unmatched
# originals as information only; here they fail: the reference would prove less than it says).
#
# Output: one line per reference, `ok` or `FAILED`, followed by the differences. Exit code 0
# when every reference passes, 1 otherwise (difference, missing file, sha256 mismatch, invalid
# references file, build or replay failure). Writes only under build/parity/ (runtime
# checkpoints, filtered copies, dq9-app logs); cargo itself writes to its target directory.
param(
    [string] $References,
    [string] $Rom
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path (Join-Path $Root 'build') 'parity'
$CompareRuns = Join-Path $PSScriptRoot 'compare-runs.ps1'
$Utf8 = New-Object System.Text.UTF8Encoding $false

function Stop-Parity([string] $Message) {
    Write-Output "parity: $Message"
    exit 1
}

function Find-Rom {
    # Same lookup as crates/testrom/src/lib.rs (find) and scripts/verify.ps1 -RequireRom.
    if ($env:DQ9_ROM) {
        if (-not (Test-Path -LiteralPath $env:DQ9_ROM -PathType Leaf)) {
            Stop-Parity "DQ9_ROM points to a missing file: $env:DQ9_ROM"
        }
        return (Resolve-Path -LiteralPath $env:DQ9_ROM).Path
    }
    foreach ($Dir in @((Join-Path $Root 'rom'), $Root)) {
        if (-not (Test-Path -LiteralPath $Dir -PathType Container)) { continue }
        $Found = @(Get-ChildItem -LiteralPath $Dir -File | Where-Object { $_.Extension -eq '.nds' })
        if ($Found.Count -gt 1) { Stop-Parity "several ROMs in $Dir; pass -Rom or set DQ9_ROM" }
        if ($Found.Count -eq 1) { return $Found[0].FullName }
    }
    Stop-Parity 'no ROM found (pass -Rom, set DQ9_ROM or put one *.nds in rom/)'
}

function Get-Sha256([string] $Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Test-Keys($Object, [string[]] $Allowed, [string] $Where) {
    foreach ($Property in $Object.PSObject.Properties) {
        if ($Allowed -notcontains $Property.Name) { throw "$Where`: unknown key '$($Property.Name)'" }
    }
}

# Resolves a { path, sha256, source } entry and checks the file against its digest.
function Resolve-Checked($Entry, [string] $BaseDir, [string] $Where) {
    if ($null -eq $Entry) { throw "$Where`: missing" }
    Test-Keys $Entry @('path', 'sha256', 'source') $Where
    if (-not $Entry.path) { throw "$Where`: no path" }
    if ("$($Entry.sha256)" -notmatch '^[0-9a-fA-F]{64}$') { throw "$Where`: sha256 must be 64 hex digits" }
    $Path = Join-Path $BaseDir $Entry.path
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Where`: missing file $Path" }
    $Actual = Get-Sha256 $Path
    if ($Actual -ne "$($Entry.sha256)".ToLowerInvariant()) {
        throw "$Where`: sha256 mismatch for $Path (expected $($Entry.sha256), got $Actual)"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

# Copies a checkpoint file, removing the given global flags from the "flags" array of every
# line (the per-track chapter_flags/step_flags are left alone). Returns how many lines lost a
# flag, per flag.
function Copy-WithoutFlags([string] $Source, [string] $Destination, [int[]] $Flags) {
    $Removed = @{}
    foreach ($Flag in $Flags) { $Removed[$Flag] = 0 }
    $Pattern = [regex] '"flags"\s*:\s*\[([^\]]*)\]'
    $Lines = foreach ($Line in [IO.File]::ReadAllLines($Source)) {
        if ($Flags.Count -eq 0) { $Line; continue }
        $Pattern.Replace($Line, {
            param($Match)
            $Kept = foreach ($Text in ($Match.Groups[1].Value -split ',')) {
                if ($Text.Trim() -eq '') { continue }
                $Value = [int]$Text.Trim()
                if ($Removed.ContainsKey($Value)) { $Removed[$Value]++; continue }
                $Value
            }
            '"flags":[' + (@($Kept) -join ',') + ']'
        })
    }
    [IO.File]::WriteAllLines($Destination, [string[]]@($Lines), $Utf8)
    return $Removed
}

function Format-Arg([string] $Arg) {
    if ($Arg -notmatch '[\s"]') { return $Arg }
    return '"' + ($Arg -replace '"', '\"') + '"'
}

# --- references file ---------------------------------------------------------------------
if (-not $References) { $References = Join-Path $Root 'build/evidence/parity/references.json' }
if (-not (Test-Path -LiteralPath $References -PathType Leaf)) { Stop-Parity "no references file $References" }
$References = (Resolve-Path -LiteralPath $References).Path
$BaseDir = Split-Path -Parent $References
try {
    $Config = [IO.File]::ReadAllText($References) | ConvertFrom-Json
} catch {
    Stop-Parity "$References is not JSON: $_"
}
if ($Config.version -ne 1) { Stop-Parity "$References`: version must be 1" }
$Refs = @($Config.references)
if ($Refs.Count -eq 0) { Stop-Parity "$References`: no references" }
$Names = @{}
foreach ($Ref in $Refs) {
    if ("$($Ref.name)" -notmatch '^[A-Za-z0-9._-]+$') { Stop-Parity "invalid reference name '$($Ref.name)'" }
    if ($Names.ContainsKey($Ref.name)) { Stop-Parity "duplicate reference name '$($Ref.name)'" }
    $Names[$Ref.name] = $true
}

if ($Rom) {
    if (-not (Test-Path -LiteralPath $Rom -PathType Leaf)) { Stop-Parity "no ROM at $Rom" }
    $Rom = (Resolve-Path -LiteralPath $Rom).Path
} else {
    $Rom = Find-Rom
}
Write-Output "parity: ROM $Rom"
Write-Output "parity: references $References ($($Refs.Count))"

# --- build -------------------------------------------------------------------------------
Push-Location $Root
try {
    Write-Output 'parity: cargo build --release -p dq9-app'
    & cargo build --release -p dq9-app
    if ($LASTEXITCODE -ne 0) { Stop-Parity "cargo build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
$App = Join-Path (Join-Path $TargetDir 'release') 'dq9-app'
if (Test-Path -LiteralPath "$App.exe" -PathType Leaf) { $App = "$App.exe" }
if (-not (Test-Path -LiteralPath $App -PathType Leaf)) { Stop-Parity "no binary at $App after the build" }
if (-not (Test-Path -LiteralPath $OutDir -PathType Container)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }

# --- references --------------------------------------------------------------------------
$Runs = @{}       # inputs sha256 + args -> runtime checkpoint file
$Failed = 0
$Reserved = @('--rom', '--headless', '--replay', '--checkpoints', '--auto', '--pilot', '--trace-inputs')
foreach ($Ref in $Refs) {
    $Name = $Ref.name
    $Details = [System.Collections.Generic.List[string]]::new()
    $Summary = ''
    $Ok = $false
    try {
        Test-Keys $Ref @('name', 'description', 'inputs', 'original', 'args', 'heroTolerance', 'accepted') $Name
        $Inputs = Resolve-Checked $Ref.inputs $BaseDir "$Name inputs"
        $Original = Resolve-Checked $Ref.original $BaseDir "$Name original"
        $AppArgs = @()
        if ($null -ne $Ref.args) { $AppArgs = @($Ref.args | ForEach-Object { "$_" }) }
        foreach ($Arg in $AppArgs) {
            if ($Reserved -contains ($Arg -split '=')[0]) { throw "$Name args: $Arg is set by parity.ps1" }
        }
        $Tolerance = 0.0
        if ($null -ne $Ref.heroTolerance) { $Tolerance = [double]$Ref.heroTolerance }

        $FlagsBySide = @{ original = [System.Collections.Generic.List[int]]::new(); runtime = [System.Collections.Generic.List[int]]::new() }
        $Accepted = @($Ref.accepted | Where-Object { $null -ne $_ })
        foreach ($Entry in $Accepted) {
            Test-Keys $Entry @('flag', 'only', 'reason') "$Name accepted"
            if ($null -eq $Entry.flag) { throw "$Name accepted: no flag" }
            if (@('original', 'runtime') -notcontains $Entry.only) { throw "$Name accepted $($Entry.flag): only must be 'original' or 'runtime'" }
            if (-not "$($Entry.reason)".Trim()) { throw "$Name accepted $($Entry.flag): no reason" }
            $FlagsBySide[$Entry.only].Add([int]$Entry.flag)
        }

        $Key = (Get-Sha256 $Inputs) + ' ' + ($AppArgs -join ' ')
        if (-not $Runs.ContainsKey($Key)) {
            $Index = $Runs.Count + 1
            $RunOut = Join-Path $OutDir "run$Index-checkpoints.jsonl"
            $RunLog = Join-Path $OutDir "run$Index"
            if (Test-Path -LiteralPath $RunOut) { Remove-Item -LiteralPath $RunOut }
            $Command = @('--rom', $Rom) + $AppArgs + @('--headless', '--replay', $Inputs, '--checkpoints', $RunOut)
            $Process = Start-Process -FilePath $App -ArgumentList (($Command | ForEach-Object { Format-Arg $_ }) -join ' ') `
                -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$RunLog.stdout.txt" -RedirectStandardError "$RunLog.stderr.txt"
            if ($Process.ExitCode -ne 0) { throw "dq9-app exited with $($Process.ExitCode) (see $RunLog.stderr.txt)" }
            if (-not (Test-Path -LiteralPath $RunOut -PathType Leaf)) { throw "dq9-app wrote no checkpoints ($RunOut)" }
            $Runs[$Key] = $RunOut
        }
        $Runtime = $Runs[$Key]

        $RefDir = Join-Path $OutDir $Name
        if (-not (Test-Path -LiteralPath $RefDir -PathType Container)) { New-Item -ItemType Directory -Path $RefDir | Out-Null }
        $OriginalCopy = Join-Path $RefDir 'original.jsonl'
        $RuntimeCopy = Join-Path $RefDir 'runtime.jsonl'
        $Removed = @{
            original = Copy-WithoutFlags $Original $OriginalCopy $FlagsBySide['original'].ToArray()
            runtime = Copy-WithoutFlags $Runtime $RuntimeCopy $FlagsBySide['runtime'].ToArray()
        }
        $Stale = 0
        $AcceptedText = foreach ($Entry in $Accepted) {
            $Count = $Removed[$Entry.only][[int]$Entry.flag]
            if ($Count -eq 0) {
                $Stale++
                $Details.Add("stale accepted deviation: flag $($Entry.flag) is not in any $($Entry.only) checkpoint (remove it from the reference)")
            }
            "flag $($Entry.flag) $($Entry.only) only ($Count checkpoint(s))"
        }

        $Output = @(& $CompareRuns -Original $OriginalCopy -Runtime $RuntimeCopy -Limit 100 -HeroTolerance $Tolerance)
        $Code = $LASTEXITCODE
        $Unmatched = -1
        foreach ($Line in $Output) {
            if ($Line -match '^aligned (\d+) original .*; original (\d+), runtime (\d+); unmatched original (\d+)') {
                $Summary = "aligned $($Matches[1])/$($Matches[2]) original checkpoint(s), runtime $($Matches[3])"
                $Unmatched = [int]$Matches[4]
            } elseif ($Line -like 'DIFF *' -or $Line -like 'NO PARITY*') {
                $Details.Add($Line)
            }
        }
        if ($Code -ge 2) { throw "compare-runs failed: $($Output -join ' ')" }
        if ($Unmatched -gt 0) {
            $Details.Add("$Unmatched original checkpoint(s) have no runtime counterpart at the same story point and kind")
        }
        if (@($AcceptedText).Count -gt 0) { $Summary += '; accepted: ' + (@($AcceptedText) -join ', ') }
        $Ok = ($Code -eq 0 -and $Unmatched -eq 0 -and $Stale -eq 0)
    } catch {
        $Details.Add("$_")
    }
    if ($Ok) {
        Write-Output ("ok      {0}  {1}" -f $Name, $Summary)
    } else {
        $Failed++
        Write-Output ("FAILED  {0}  {1}" -f $Name, $Summary)
    }
    foreach ($Detail in $Details) { Write-Output "        $Detail" }
}

Write-Output ("parity: {0} reference(s), {1} ok, {2} failed" -f $Refs.Count, ($Refs.Count - $Failed), $Failed)
if ($Failed -gt 0) { exit 1 }
exit 0
