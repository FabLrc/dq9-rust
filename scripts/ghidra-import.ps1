# Creates/refreshes the Ghidra project re/ghidra/dq9.gpr from `dq9re extract` output.
# Prereqs: scripts/setup-re-tools.ps1, then `cargo run -p dq9re -- extract --code-only`.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/ghidra-import.ps1 [-Extracted build/rom]
param(
    [string]$Extracted = 'build/rom'
)
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Ghidra = if ($env:GHIDRA_INSTALL_DIR) { $env:GHIDRA_INSTALL_DIR } else { Join-Path $Root 'tools/ghidra' }
$Headless = Join-Path $Ghidra 'support/analyzeHeadless.bat'
if (-not (Test-Path $Headless)) { throw "Ghidra not found at $Ghidra; run scripts/setup-re-tools.ps1" }

$Jdk = Join-Path $Root 'tools/jdk'
if (Test-Path $Jdk) { $env:JAVA_HOME = $Jdk }
$Extracted = Resolve-Path (Join-Path $Root $Extracted)
$LayoutPath = Join-Path $Extracted 'layout.json'
if (-not (Test-Path $LayoutPath)) { throw "$LayoutPath missing; run: cargo run -p dq9re -- extract --code-only" }
$Layout = Get-Content $LayoutPath -Raw | ConvertFrom-Json
$Base = '0x{0:x}' -f [int64]$Layout.arm9.load_addr

$Project = Join-Path $Root 're/ghidra'
New-Item -ItemType Directory -Force -Path $Project | Out-Null

& $Headless $Project dq9 `
    -import (Join-Path $Extracted 'arm9.bin') -overwrite `
    -processor 'ARM:LE:32:v5t' `
    -loader BinaryLoader -loader-baseAddr $Base `
    -scriptPath (Join-Path $Root 're/ghidra_scripts') `
    -preScript NdsLayout.java $LayoutPath `
    -postScript ApplySymbols.java (Join-Path $Root 're/symbols.txt') `
    -max-cpu ([Environment]::ProcessorCount)
if ($LASTEXITCODE -ne 0) { throw "analyzeHeadless failed ($LASTEXITCODE)" }
Write-Host "Open $Project/dq9.gpr with tools/ghidra/ghidraRun.bat"
