# Applies re/functions.txt bounds/names, then re/symbols.txt overrides (no re-analysis).
# Close the project in the Ghidra GUI first: headless mode needs the lock.
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Jdk = Join-Path $Root 'tools/jdk'
if (Test-Path $Jdk) { $env:JAVA_HOME = $Jdk }
$Ghidra = if ($env:GHIDRA_INSTALL_DIR) { $env:GHIDRA_INSTALL_DIR } else { Join-Path $Root 'tools/ghidra' }
$Headless = Join-Path $Ghidra 'support/analyzeHeadless.bat'
$Project = if ($env:DQ9_GHIDRA_PROJECT) { $env:DQ9_GHIDRA_PROJECT } else { Join-Path $Root 're/ghidra' }

$Output = & $Headless $Project dq9 -process arm9.bin -noanalysis `
    -scriptPath (Join-Path $Root 're/ghidra_scripts') `
    -postScript ApplyFunctions.java (Join-Path $Root 're/functions.txt') `
    -postScript ApplySymbols.java (Join-Path $Root 're/symbols.txt') 2>&1
$ExitCode = $LASTEXITCODE
$Output = $Output | ForEach-Object { "$_" }
$Output | Where-Object { $_ -match 'ApplyFunctions|ApplySymbols|ERROR|Exception' }
if ($ExitCode -ne 0 -or ($Output -match '(^|\s)ERROR\s') `
        -or -not ($Output -match 'ApplyFunctions: applied ') `
        -or -not ($Output -match 'ApplySymbols: applied ')) {
    throw "analyzeHeadless did not apply both symbol maps successfully (exit $ExitCode)"
}
