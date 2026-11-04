# Launches the Ghidra GUI with the project-local JDK. Open re/ghidra/dq9.gpr from there.
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$env:JAVA_HOME = Join-Path $Root 'tools/jdk'
& (Join-Path $Root 'tools/ghidra/ghidraRun.bat')
