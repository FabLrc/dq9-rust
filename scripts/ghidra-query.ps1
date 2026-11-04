# Runs re/ghidra_scripts/Query.java read-only against re/ghidra/dq9.gpr and prints its output.
# Usage: powershell -File scripts/ghidra-query.ps1 decomp 0x020f27f8 ov_017::0218c0f4
#        powershell -File scripts/ghidra-query.ps1 xrefs 0x020f27f8
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$env:JAVA_HOME = Join-Path $Root 'tools/jdk'
$Headless = Join-Path $Root 'tools/ghidra/support/analyzeHeadless.bat'

$out = & $Headless (Join-Path $Root 're/ghidra') dq9 -process arm9.bin -noanalysis -readOnly `
    -scriptPath (Join-Path $Root 're/ghidra_scripts') -postScript Query.java @args 2>&1
# Keep only the script's own output, without Ghidra's log prefix/suffix.
$out | ForEach-Object { "$_" } |
    Where-Object { $_ -match 'Query\.java> ' } |
    ForEach-Object { ($_ -replace '^.*?Query\.java> ', '') -replace ' \(GhidraScript\)\s*$', '' }
