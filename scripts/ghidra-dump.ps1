# Decompiles every function in the given ranges into a text file (re/ghidra_scripts/DumpDecomp.java).
# Usage: powershell -File scripts/ghidra-dump.ps1 build/decomp/ov17.c ov_017::0218b5a0-ov_017::021ee000
# Same project locking rules as ghidra-query.ps1 (DQ9_GHIDRA_PROJECT selects a project copy).
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$env:JAVA_HOME = Join-Path $Root 'tools/jdk'
$Headless = Join-Path $Root 'tools/ghidra/support/analyzeHeadless.bat'
$Project = if ($env:DQ9_GHIDRA_PROJECT) { $env:DQ9_GHIDRA_PROJECT } else { Join-Path $Root 're/ghidra' }

$out = & $Headless $Project dq9 -process arm9.bin -noanalysis -readOnly `
    -scriptPath (Join-Path $Root 're/ghidra_scripts') -postScript DumpDecomp.java @args 2>&1
$out | ForEach-Object { "$_" } |
    Where-Object { $_ -match 'DumpDecomp\.java> ' } |
    ForEach-Object { ($_ -replace '^.*?DumpDecomp\.java> ', '') -replace ' \(GhidraScript\)\s*$', '' }
