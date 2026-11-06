# Compares a melonDS checkpoint (dq9re state --json) with the story state the reimplemented
# runtime reports at the same point: either one checkpoint object from `dq9-app --checkpoints`
# (JSONL contract, -RuntimeJson) or the text report of `--new-game --headless --auto`
# (-Report, plus -TrackIndex because that report does not name the current track).
#
# Usage:
#   powershell -File scripts/oracle-compare.ps1 -State observatory.ml1 `
#       -Report runtime-auto.txt -TrackIndex 0
#   powershell -File scripts/oracle-compare.ps1 -State observatory.ml1 `
#       -RuntimeJson checkpoint.json
#
# Exit code 0 only when the map, the current track index, every track's progress and both
# flag sets, the global flags and (when the runtime provides them) the quests agree.
# See docs/re/melonds-state.md, § « Oracle différentiel ».
param(
    [Parameter(Mandatory)] [string] $State,
    [string] $Report = '',
    [string] $RuntimeJson = '',
    [int] $TrackIndex = -1,
    [string] $Rom = $env:DQ9_ROM,
    [string] $Dq9re = ''
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Report -and -not $RuntimeJson) { throw 'pass -Report or -RuntimeJson' }

if (-not $Dq9re) {
    $Dq9re = Join-Path $Root 'target/release/dq9re.exe'
    if (-not (Test-Path $Dq9re)) { $Dq9re = Join-Path $Root 'target/debug/dq9re.exe' }
}
if (-not (Test-Path $Dq9re)) { throw "no dq9re binary; build it or pass -Dq9re" }

$dargs = @('state', (Resolve-Path $State).Path, '--json')
if ($Rom) { $dargs += @('--rom', $Rom) }
$json = & $Dq9re @dargs
if ($LASTEXITCODE -ne 0) { throw 'dq9re state failed' }
$ds = $json | ConvertFrom-Json

function Parse-Set([string] $text) {
    $set = [System.Collections.Generic.SortedSet[int]]::new()
    foreach ($part in $text.Split(',')) {
        $t = $part.Trim()
        if ($t) { [void]$set.Add([int]$t) }
    }
    return ,$set
}
function Set-FromArray($values) {
    $set = [System.Collections.Generic.SortedSet[int]]::new()
    foreach ($v in @($values)) { if ($null -ne $v) { [void]$set.Add([int]$v) } }
    return ,$set
}
function Format-Set($set) { ($set | ForEach-Object { $_ }) -join ',' }
function Quest-Map($object) {
    $map = @{}
    if ($null -ne $object) {
        foreach ($p in $object.PSObject.Properties) { $map[[int]$p.Name] = [int]$p.Value }
    }
    return $map
}

$tracks = @{}
$global = $null
$runtimeMap = $null
$runtimeQuests = $null
$runtimeDaytime = $null
$runtimeTrack = $null

if ($RuntimeJson) {
    $rt = Get-Content -Raw $RuntimeJson | ConvertFrom-Json
    $runtimeMap = [int]$rt.map
    $runtimeTrack = [int]$rt.track
    $global = Set-FromArray $rt.flags
    $runtimeQuests = Quest-Map $rt.quests
    if ($null -ne $rt.daytime) { $runtimeDaytime = [bool]$rt.daytime }
    for ($i = 0; $i -lt $rt.tracks.Count; $i++) {
        $t = $rt.tracks[$i]
        $tracks[$i] = [pscustomobject]@{
            chapter = [int]$t.chapter
            step = [int]$t.step
            substep = [int]$t.substep
            chapter_flags = Set-FromArray $t.chapter_flags
            step_flags = Set-FromArray $t.step_flags
        }
    }
} else {
    $trackRe = [regex]'^track (\d+): (\d+)\.(\d+)\.(\d+) flags \[(.*)\]$'
    $globalRe = [regex]'^global flags: \{(.*)\}$'
    $mapRe = [regex]'^stopped: free control on map (\d+)'
    $mapChangeRe = [regex]'^.*map change (?:(\d+) -> )?(\d+) '
    $currentRe = [regex]'^current track (\d+)'
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
        $m = $currentRe.Match($line)
        if ($m.Success) { $runtimeTrack = [int]$m.Groups[1].Value; continue }
        $m = $mapChangeRe.Match($line)
        if ($m.Success -and $null -eq $runtimeMap) { $runtimeMap = [int]$m.Groups[2].Value }
    }
    if ($TrackIndex -ge 0) { $runtimeTrack = $TrackIndex }
}

if ($null -eq $global) { throw "no global flags in $Report (or -RuntimeJson)" }

$fail = 0
function Fail([string] $what) { $script:fail++; Write-Output "DIFF $what" }
function Same([string] $what, $a, $b) {
    if ("$a" -ne "$b") { Fail "$what`: melonDS $a, runtime $b" }
}

$scenario = $ds.tracks[$ds.track]
Write-Output "checkpoint: vblank $($ds.vblank), map $($ds.map), track $($ds.track) $($scenario.chapter).$($scenario.step).$($scenario.substep)"
Write-Output "runtime: map $runtimeMap, tracks $($tracks.Count), global flags $($global.Count)"
Same 'map' $ds.map $runtimeMap
if ($null -eq $runtimeTrack) {
    Fail 'current track: the runtime report does not name it; pass -TrackIndex or -RuntimeJson'
} else {
    Same 'current track' $ds.track $runtimeTrack
}
if ($null -ne $runtimeDaytime) { Same 'daytime' $ds.daytime $runtimeDaytime }

for ($i = 0; $i -lt $ds.tracks.Count; $i++) {
    $d = $ds.tracks[$i]
    if (-not $tracks.ContainsKey($i)) { Fail "track $i missing from the runtime state"; continue }
    $r = $tracks[$i]
    Same "track $i progress" "$($d.chapter).$($d.step).$($d.substep)" "$($r.chapter).$($r.step).$($r.substep)"
    $dc = (Format-Set $d.chapter_flags)
    $dsf = (Format-Set $d.step_flags)
    $rc = (Format-Set $r.chapter_flags)
    $rs = (Format-Set $r.step_flags)
    if ($dc -ne $rc) { Fail "track $i chapter flags: melonDS [$dc], runtime [$rc]" }
    if ($dsf -ne $rs) { Fail "track $i step flags: melonDS [$dsf], runtime [$rs]" }
}

$dsGlobal = Set-FromArray $ds.flags
$onlyDs = @($dsGlobal | Where-Object { -not $global.Contains($_) })
$onlyRt = @($global | Where-Object { -not $dsGlobal.Contains($_) })
if ($onlyDs.Count) { Fail "global flags only in melonDS: $($onlyDs -join ',')" }
if ($onlyRt.Count) { Fail "global flags only in the runtime: $($onlyRt -join ',')" }

$dsQuests = Quest-Map $ds.quests
if ($null -ne $runtimeQuests) {
    $qOnlyDs = @($dsQuests.Keys | Where-Object { -not $runtimeQuests.ContainsKey($_) } | Sort-Object)
    $qOnlyRt = @($runtimeQuests.Keys | Where-Object { -not $dsQuests.ContainsKey($_) } | Sort-Object)
    $qDiff = @($dsQuests.Keys | Where-Object { $runtimeQuests.ContainsKey($_) -and $runtimeQuests[$_] -ne $dsQuests[$_] } | Sort-Object)
    if ($qOnlyDs.Count) { Fail "quests only in melonDS: $(($qOnlyDs | ForEach-Object { "$_=$($dsQuests[$_])" }) -join ',')" }
    if ($qOnlyRt.Count) { Fail "quests only in the runtime: $(($qOnlyRt | ForEach-Object { "$_=$($runtimeQuests[$_])" }) -join ',')" }
    if ($qDiff.Count) { Fail "quest nibbles differ: $(($qDiff | ForEach-Object { "$_ melonDS=$($dsQuests[$_]) runtime=$($runtimeQuests[$_])" }) -join ', ')" }
} elseif ($dsQuests.Count) {
    Fail "quests present in melonDS ($(($dsQuests.Keys | Sort-Object | ForEach-Object { "$_=$($dsQuests[$_])" }) -join ',')) but the runtime state has none: use -RuntimeJson"
} else {
    Write-Output 'quests: none on either side'
}

if ($fail -eq 0) { Write-Output 'OK: story point and flags identical' }
exit $fail
