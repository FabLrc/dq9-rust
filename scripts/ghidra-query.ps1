# Runs re/ghidra_scripts/Query.java read-only against re/ghidra/dq9.gpr and prints its output.
# Results are cached under build/ghidra-cache/ (ignored by git): a second identical query
# skips the 8-30 s analyzeHeadless run. The key covers the command, its addresses, the ROM
# (canonical path, size, modification time) and the SHA-256 of Query.java; -Refresh bypasses
# and rewrites the cache. "cache: miss" or "cache: hit" is printed on stderr.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 decomp 0x020f27f8 ov_017::0218c0f4
#        powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 xrefs 0x020f27f8
#        powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 decomp 0x020f27f8 -Refresh
# Ghidra locks a project while a query runs: to run queries in parallel, give each its own
# copy of re/ghidra and point DQ9_GHIDRA_PROJECT at it. In a linked worktree the tools,
# project and ROM are read from the main checkout when the worktree does not have them.
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

# Main checkout when this script runs from a linked worktree, whose .git is a file.
function Get-MainRoot {
    $git = Join-Path $Root '.git'
    if (-not (Test-Path $git -PathType Leaf)) { return $Root }
    $line = Get-Content $git -TotalCount 1
    if ($line -match '^gitdir:\s*(.+)$') {
        $dir = $Matches[1].Trim()
        if (-not [IO.Path]::IsPathRooted($dir)) { $dir = Join-Path $Root $dir }
        $dir = [IO.Path]::GetFullPath($dir)
        $dir = Split-Path -Parent (Split-Path -Parent $dir)
        if ($dir) { return (Split-Path -Parent $dir) }
    }
    return $Root
}

function Resolve-SharedPath([string] $Relative) {
    $local = Join-Path $Root $Relative
    if (Test-Path $local) { return $local }
    $main = Get-MainRoot
    if ($main -and $main -ne $Root) {
        $shared = Join-Path $main $Relative
        if (Test-Path $shared) { return $shared }
    }
    return $local
}

function Get-FileSha256([string] $Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        try {
            return ([BitConverter]::ToString($sha.ComputeHash($stream)) -replace '-', '').ToLowerInvariant()
        } finally { $sha.Dispose() }
    } finally { $stream.Dispose() }
}

# The ROM the Ghidra project was imported from: -Rom is not accepted here, so DQ9_ROM,
# rom/*.nds or *.nds at either checkout root. Its identity makes a cache entry stale when
# the player switches dumps.
function Get-RomIdentity {
    if ($env:DQ9_ROM -and (Test-Path $env:DQ9_ROM -PathType Leaf)) {
        $rom = Get-Item $env:DQ9_ROM
        return ('{0}|{1}|{2}' -f $rom.FullName.ToLowerInvariant(), $rom.Length, $rom.LastWriteTimeUtc.Ticks)
    }
    $roots = @($Root)
    $main = Get-MainRoot
    if ($main -and $main -ne $Root) { $roots += $main }
    foreach ($root in $roots) {
        foreach ($dir in @((Join-Path $root 'rom'), $root)) {
            $rom = @(Get-ChildItem $dir -Filter *.nds -File -ErrorAction SilentlyContinue | Select-Object -First 1)
            if ($rom.Count -gt 0) {
                return ('{0}|{1}|{2}' -f $rom[0].FullName.ToLowerInvariant(), $rom[0].Length, $rom[0].LastWriteTimeUtc.Ticks)
            }
        }
    }
    return 'no-rom'
}

# --- arguments ----------------------------------------------------------------------------
$Refresh = $false
$Rest = [System.Collections.Generic.List[string]]::new()
foreach ($argument in $args) {
    if ($argument -ieq '-Refresh') { $Refresh = $true } else { $Rest.Add([string] $argument) }
}
if ($Rest.Count -lt 2) {
    [Console]::Error.WriteLine('usage: ghidra-query.ps1 [-Refresh] decomp|xrefs|calls|disasm <address> [more addresses]')
    exit 2
}
$Command = $Rest[0]
$Targets = $Rest.GetRange(1, $Rest.Count - 1)

# --- tools, project, cache key -------------------------------------------------------------
$env:JAVA_HOME = Resolve-SharedPath 'tools/jdk'
$Headless = Resolve-SharedPath 'tools/ghidra/support/analyzeHeadless.bat'
$Project = if ($env:DQ9_GHIDRA_PROJECT) { $env:DQ9_GHIDRA_PROJECT } else { Resolve-SharedPath 're/ghidra' }
$Scripts = Join-Path $Root 're/ghidra_scripts'
$QueryScript = Join-Path $Scripts 'Query.java'
if (-not (Test-Path $Headless)) {
    [Console]::Error.WriteLine("ghidra-query: analyzeHeadless.bat not found (looked in $Headless); run scripts/setup-re-tools.ps1")
    exit 1
}
if (-not (Test-Path $QueryScript)) {
    [Console]::Error.WriteLine("ghidra-query: missing $QueryScript")
    exit 1
}

$keyParts = @($Command) + @($Targets) + @((Get-RomIdentity), (Get-FileSha256 $QueryScript))
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $key = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($keyParts -join "`n")))) -replace '-', '').ToLowerInvariant()
} finally { $sha.Dispose() }

$cache = Join-Path $Root ('build/ghidra-cache/' + $key + '.txt')
if (-not $Refresh -and (Test-Path $cache -PathType Leaf)) {
    [Console]::Error.WriteLine('cache: hit')
    foreach ($line in [IO.File]::ReadAllLines($cache)) { Write-Output $line }
    exit 0
}
[Console]::Error.WriteLine('cache: miss')

# --- query --------------------------------------------------------------------------------
$output = & $Headless $Project dq9 -process arm9.bin -noanalysis -readOnly `
    -scriptPath $Scripts -postScript Query.java $Command @Targets 2>&1
$code = $LASTEXITCODE
# Keep only the script's own output, without Ghidra's log prefix/suffix.
$lines = @($output | ForEach-Object { "$_" } |
    Where-Object { $_ -match 'Query\.java> ' } |
    ForEach-Object { ($_ -replace '^.*?Query\.java> ', '') -replace ' \(GhidraScript\)\s*$', '' })
foreach ($line in $lines) { Write-Output $line }

if ($code -ne 0) {
    [Console]::Error.WriteLine("ghidra-query: analyzeHeadless exited $code; result not cached")
    exit $code
}
if ($lines.Count -eq 0) {
    # Every Query.java command prints at least a header; no line means the script failed
    # before running (bad arguments, locked project), so do not cache the empty result.
    [Console]::Error.WriteLine('ghidra-query: Query.java produced no output; result not cached')
    exit 1
}

$directory = Split-Path -Parent $cache
New-Item -ItemType Directory -Force $directory | Out-Null
$temporary = $cache + '.tmp'
[IO.File]::WriteAllLines($temporary, [string[]] $lines, (New-Object Text.UTF8Encoding($false)))
Move-Item -Force $temporary $cache
