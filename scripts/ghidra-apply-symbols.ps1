# Applies re/symbols.txt to the existing project re/ghidra/dq9.gpr (no re-analysis).
# Close the project in the Ghidra GUI first: headless mode needs the lock.
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$env:JAVA_HOME = Join-Path $Root 'tools/jdk'
$Headless = Join-Path $Root 'tools/ghidra/support/analyzeHeadless.bat'

& $Headless (Join-Path $Root 're/ghidra') dq9 -process arm9.bin -noanalysis `
    -scriptPath (Join-Path $Root 're/ghidra_scripts') `
    -postScript ApplySymbols.java (Join-Path $Root 're/symbols.txt') 2>&1 |
    ForEach-Object { "$_" } | Where-Object { $_ -match 'ApplySymbols|ERROR|Exception' }
if ($LASTEXITCODE -ne 0) { throw "analyzeHeadless failed ($LASTEXITCODE)" }
