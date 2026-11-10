# Runs the verification loop. This script is the single source of the verification commands:
# AGENTS.md, the docs and CI all call it. Steps, in this order:
#   fmt       cargo fmt --all --check
#   clippy    cargo clippy --workspace --all-targets -- -D warnings
#   test      cargo test --workspace
#   wasm      cargo build of the portable crates and the app for wasm32-unknown-unknown
#   coverage  cargo dq9re coverage --readme README.md (rewrites the README progress block
#             locally; commit it with the change, CI fails when it is stale)
# The script stops at the first failing step and prints each step's duration plus a summary.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/verify.ps1                 # all five steps
#   powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Fast           # fmt + clippy
#   powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Only fmt,clippy
#   powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -RequireRom     # integration gate
#
# -Only takes step names as an array or as one comma-separated string (`powershell -File`
# passes `fmt,clippy` as a single string); unknown names are an error. -Fast and -Only are
# exclusive.
# -RequireRom fails before any step unless a ROM is found with the lookup of crates/testrom:
# DQ9_ROM, else the `*.nds` of rom/, else the `*.nds` of the repository root. Several `*.nds`
# in the directory searched are an error (testrom would pick one arbitrarily). Without it,
# local tests use the ROM when present; CI never has it and tests/real_rom.rs skip themselves.
param(
    [switch] $Fast,
    [string[]] $Only,
    [switch] $RequireRom
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

$AllSteps = @(
    @{ Name = 'fmt'; Args = @('fmt', '--all', '--check') },
    @{ Name = 'clippy'; Args = @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') },
    @{ Name = 'test'; Args = @('test', '--workspace') },
    # CI sets RUSTFLAGS=-D warnings for every step; locally only this step needs it, because
    # a wasm-only dead_code warning (code used by the native entry points) passed locally and
    # failed CI on 2bec2e2.
    @{ Name = 'wasm'; Env = @{ RUSTFLAGS = '-D warnings' }; Args = @(
        'build',
        '-p', 'nds-rom', '-p', 'dq9-formats', '-p', 'dq9-core', '-p', 'dq9-render',
        '-p', 'dq9-audio', '-p', 'dq9-app',
        '--target', 'wasm32-unknown-unknown'
    ) },
    @{ Name = 'coverage'; Args = @('dq9re', 'coverage', '--readme', 'README.md') }
)
$StepNames = @($AllSteps | ForEach-Object { $_.Name })

if ($Fast -and $Only) { throw '-Fast and -Only are exclusive' }
if ($Only) {
    $Wanted = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } |
        Where-Object { $_ -ne '' })
    if ($Wanted.Count -eq 0) { throw '-Only needs at least one step name' }
    $Unknown = @($Wanted | Where-Object { $StepNames -notcontains $_ })
    if ($Unknown.Count -gt 0) {
        throw ("unknown step(s): {0}; known steps: {1}" -f ($Unknown -join ', '), ($StepNames -join ', '))
    }
    # Keep the canonical order whatever order the names were given in.
    $Steps = @($AllSteps | Where-Object { $Wanted -contains $_.Name })
    $Mode = 'only ' + (($Steps | ForEach-Object { $_.Name }) -join ',')
} elseif ($Fast) {
    $Steps = @($AllSteps | Where-Object { @('fmt', 'clippy') -contains $_.Name })
    $Mode = 'fast'
} else {
    $Steps = $AllSteps
    $Mode = 'full'
}

if ($RequireRom) {
    # Same lookup as crates/testrom/src/lib.rs (find).
    if ($env:DQ9_ROM) {
        if (-not (Test-Path -LiteralPath $env:DQ9_ROM -PathType Leaf)) {
            Write-Host "verify.ps1: DQ9_ROM points to a missing file: $env:DQ9_ROM"
            exit 1
        }
        $Rom = (Resolve-Path -LiteralPath $env:DQ9_ROM).Path
    } else {
        $Rom = $null
        foreach ($Dir in @((Join-Path $Root 'rom'), $Root)) {
            if (-not (Test-Path -LiteralPath $Dir -PathType Container)) { continue }
            $Found = @(Get-ChildItem -LiteralPath $Dir -File | Where-Object { $_.Extension -eq '.nds' })
            if ($Found.Count -gt 1) {
                Write-Host ("verify.ps1: several ROMs in {0} ({1}); set DQ9_ROM" -f $Dir, (($Found | ForEach-Object { $_.Name }) -join ', '))
                exit 1
            }
            if ($Found.Count -eq 1) { $Rom = $Found[0].FullName; break }
        }
        if (-not $Rom) {
            Write-Host 'verify.ps1: -RequireRom: no ROM found (set DQ9_ROM or put one *.nds in rom/)'
            exit 1
        }
    }
    # Hand the ROM to the test binaries explicitly: a target directory shared between
    # worktrees can run a binary built elsewhere, whose own lookup would miss this ROM.
    $env:DQ9_ROM = $Rom
    Write-Host "==> ROM: $Rom"
}

Push-Location $Root
try {
    $Results = @()
    $Failed = $false
    $Started = Get-Date
    foreach ($Step in $Steps) {
        # Native commands do not raise on a non-zero exit code: check $LASTEXITCODE instead.
        Write-Host "==> $($Step.Name): cargo $($Step.Args -join ' ')"
        $StepStarted = Get-Date
        $CargoArgs = $Step.Args
        $SavedFlags = $env:RUSTFLAGS
        if ($Step.Env -and $Step.Env.RUSTFLAGS) { $env:RUSTFLAGS = $Step.Env.RUSTFLAGS }
        try { & cargo @CargoArgs } finally { $env:RUSTFLAGS = $SavedFlags }
        $Code = $LASTEXITCODE
        $Elapsed = ((Get-Date) - $StepStarted).TotalSeconds
        $Results += [pscustomobject]@{
            Step = $Step.Name
            Seconds = [math]::Round($Elapsed, 1)
            Ok = ($Code -eq 0)
        }
        if ($Code -ne 0) {
            $Failed = $true
            break
        }
    }

    $Skipped = $Steps.Count - $Results.Count
    $TotalSeconds = [math]::Round(((Get-Date) - $Started).TotalSeconds, 1)
    Write-Host ''
    Write-Host ('verify.ps1 ({0}) in {1:N1} s' -f $Mode, $TotalSeconds)
    foreach ($Result in $Results) {
        $Status = if ($Result.Ok) { 'ok' } else { 'FAILED' }
        Write-Host ('  {0,-8} {1,8:N1} s  {2}' -f $Result.Step, $Result.Seconds, $Status)
    }
    if ($Skipped -gt 0) {
        Write-Host "  $Skipped step(s) not run"
    }
    if ($Failed) {
        exit 1
    }
} finally {
    Pop-Location
}
