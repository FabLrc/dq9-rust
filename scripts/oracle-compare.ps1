# Compares a melonDS checkpoint (dq9re state --json) with the story state the reimplemented
# runtime prints at the same point (dq9-app --new-game --headless --auto [--pilot]).
# Usage: powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 -State build/oracle/pro-3.ml1 -Report build/oracle/runtime-3.txt [-Rom path\to\rom.nds]
# Exit code 0 only when the current track, every track's progress and flags, and the global
# flags agree; differences are printed one per line. See docs/re/melonds-state.md,
# section "Oracle différentiel".
param(
    [Parameter(Mandatory)] [string] $State,
    [Parameter(Mandatory)] [string] $Report,
    [string] $Rom = $env:DQ9_ROM,
    [string] $Dq9re = ''
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

if (-not $Dq9re) {
    $Dq9re = Join-Path $Root 'target/release/dq9re.exe'
    if (-not (Test-Path $Dq9re)) { $Dq9re = Join-Path $Root 'target/debug/dq9re.exe' }
}
if (-not (Test-Path $Dq9re)) { throw "no dq9re binary; build it or pass -Dq9re" }

$dargs = @('state', (Resolve-Path $State).Path, '--json')
if ($Rom) { $dargs += @('--rom', $Rom) }
$json = & $Dq9re @dargs
if ($LASTEXITCODE -ne 0) { throw "dq9re state failed" }
$ds = $json | ConvertFrom-Json

function Parse-Set([string] $text) {
    $set = [System.Collections.Generic.SortedSet[int]]::new()
    foreach ($part in $text.Split(',')) {
        $t = $part.Trim()
        if ($t) { [void]$set.Add([int]$t) }
    }
    return ,$set
}
function Format-Set($set) { ($set | ForEach-Object { $_ }) -join ',' }

# Runtime report: the last line per track and the global flags.
$trackRe = [regex]'^track (\d+): (\d+)\.(\d+)\.(\d+) flags \[(.*)\]$'
$globalRe = [regex]'^global flags: \{(.*)\}$'
$mapRe = [regex]'^stopped: free control on map (\d+)'
$mapChangeRe = [regex]'^.*map change (?:(\d+) -> )?(\d+) '
$tracks = @{}
$global = $null
$runtimeMap = $null
foreach ($line in Get-Content $Report) {
    $m = $trackRe.Match($line)
    if ($m.Success) {
        $sets = [regex]::Matches($m.Groups[5].Value, '\{([^}]*)\}')
        $tracks[[int]$m.Groups[1].Value] = [pscustomobject]@{
            chapter = [int]$m.Groups[2].Value
            step = [int]$m.Groups[3].Value
            substep = [int]$m.Groups[4].Value
            chapter_flags = if ($sets.Count -gt 0) { Parse-Set $sets[0].Groups[1].Value } else { Parse-Set '' }
            step_flags = if ($sets.Count -gt 1) { Parse-Set $sets[1].Groups[1].Value } else { Parse-Set '' }
        }
        continue
    }
    $m = $globalRe.Match($line)
    if ($m.Success) { $global = Parse-Set $m.Groups[1].Value; continue }
    $m = $mapRe.Match($line)
    if ($m.Success) { $runtimeMap = [int]$m.Groups[1].Value; continue }
    $m = $mapChangeRe.Match($line)
    if ($m.Success -and -not $runtimeMap) { $runtimeMap = [int]$m.Groups[2].Value }
}
if (-not $global) { throw "no 'global flags:' line in $Report" }

$fail = 0
function Fail([string] $what) { $script:fail++; Write-Output "DIFF $what" }
function Same([string] $what, $a, $b) {
    if ("$a" -ne "$b") { Fail "$what`: melonDS $a, runtime $b" }
}

$scenario = $ds.tracks[$ds.track]
Write-Output "checkpoint: vblank $($ds.vblank), map $($ds.map), track $($ds.track) $($scenario.chapter).$($scenario.step).$($scenario.substep)"
Write-Output "runtime: map $runtimeMap, tracks $($tracks.Count), global flags $($global.Count)"
Same 'map' $ds.map $runtimeMap

for ($i = 0; $i -lt $ds.tracks.Count; $i++) {
    $d = $ds.tracks[$i]
    if (-not $tracks.ContainsKey($i)) { Fail "track $i missing from the runtime report"; continue }
    $r = $tracks[$i]
    Same "track $i progress" "$($d.chapter).$($d.step).$($d.substep)" "$($r.chapter).$($r.step).$($r.substep)"
    $dc = (Format-Set $d.chapter_flags)
    $dsf = (Format-Set $d.step_flags)
    $rc = (Format-Set $r.chapter_flags)
    $rs = (Format-Set $r.step_flags)
    if ($dc -ne $rc) { Fail "track $i chapter flags: melonDS [$dc], runtime [$rc]" }
    if ($dsf -ne $rs) { Fail "track $i step flags: melonDS [$dsf], runtime [$rs]" }
}

$dsGlobal = [System.Collections.Generic.SortedSet[int]]::new()
foreach ($f in $ds.flags) { [void]$dsGlobal.Add([int]$f) }
$onlyDs = @($dsGlobal | Where-Object { -not $global.Contains($_) })
$onlyRt = @($global | Where-Object { -not $dsGlobal.Contains($_) })
if ($onlyDs.Count) { Fail "global flags only in melonDS: $($onlyDs -join ',')" }
if ($onlyRt.Count) { Fail "global flags only in the runtime: $($onlyRt -join ',')" }

if ($ds.quests) {
    Write-Output "quests (melonDS): $((($ds.quests.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ','))"
    Write-Output "quests: not printed by the runtime report, not compared"
}
if ($fail -eq 0) { Write-Output 'OK: story point and flags identical' }
exit $fail
