# Runs the local verification loop, in the order CI uses: `cargo fmt --all --check` and clippy,
# then (unless -Fast) the workspace tests and the wasm build of the portable crates and the app.
# The script stops at the first failing step and prints each step's duration plus a summary.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 [-Fast]
# -Fast only runs fmt and clippy.
# Local tests use the ROM when one is present (rom/, --rom or DQ9_ROM); CI never has it and
# the tests/real_rom.rs tests skip themselves there.
param(
    [switch] $Fast
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Push-Location $Root
try {
    $Steps = @(
        @{ Name = 'fmt'; Args = @('fmt', '--all', '--check') },
        @{ Name = 'clippy'; Args = @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') }
    )
    if (-not $Fast) {
        $Steps += @{ Name = 'test'; Args = @('test', '--workspace') }
        $Steps += @{ Name = 'wasm'; Args = @(
            'build',
            '-p', 'nds-rom', '-p', 'dq9-formats', '-p', 'dq9-core', '-p', 'dq9-render',
            '-p', 'dq9-audio', '-p', 'dq9-app',
            '--target', 'wasm32-unknown-unknown'
        ) }
    }

    $Mode = if ($Fast) { 'fast' } else { 'full' }
    $Results = @()
    $Failed = $false
    $Started = Get-Date
    foreach ($Step in $Steps) {
        # Native commands do not raise on a non-zero exit code: check $LASTEXITCODE instead.
        Write-Host "==> $($Step.Name): cargo $($Step.Args -join ' ')"
        $StepStarted = Get-Date
        $CargoArgs = $Step.Args
        & cargo @CargoArgs
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
        Write-Host ('  {0,-7} {1,8:N1} s  {2}' -f $Result.Step, $Result.Seconds, $Status)
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
