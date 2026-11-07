# Compares two runs of story checkpoints (JSONL, one object per line) by aligning them on
# (map, current track) and the story point (chapter.step.substep) instead of frame counts.
#
# The original run and the runtime do not share a tick base: melonDS runs at wall-clock until
# a state is saved, and the runtime compresses waits. `dq9-app --checkpoints FILE` and the
# libretro host emit the same fields, so this script pairs the first, second, ... checkpoint
# with the same key on both sides and compares the state at that point. It is a diagnostic
# for the strict oracle, not a replacement for `scripts/oracle-compare.ps1`: a checkpoint
# that exists on one side only is reported as information, not as parity.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/compare-runs.ps1 `
#       -Original build/oracle/original.jsonl -Runtime build/oracle/runtime.jsonl
#
# Exit code 0 when every aligned pair agrees and at least one pair was aligned; 1 on a
# difference, or when nothing could be aligned (the files then prove nothing together); 2 when
# an input is unreadable or a line is not JSON. `-HeroTolerance` (default 0) also compares the
# hero's x/y/z; 0 disables it because the two runs may stand a few centimetres apart at a scene
# boundary.
#
# Each runtime checkpoint is paired at most once. A checkpoint whose kind has no counterpart
# (a `f012625` movement sample, for example) is left unmatched: pairing it with a control
# point that shares its story key would compare two different moments as if they were one.
# The only cross-kind pair kept is the original `after` (a follow-up capture at the same
# story point) with the runtime `control`.
param(
    [Parameter(Mandatory)] [string] $Original,
    [Parameter(Mandatory)] [string] $Runtime,
    [int] $Limit = 30,
    [double] $HeroTolerance = 0
)
$ErrorActionPreference = 'Stop'
trap { [Console]::Error.WriteLine("compare-runs: $_"); exit 2 }

function Load-CheckpointFile([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path)) { throw "no checkpoint file $Path" }
    $list = [System.Collections.Generic.List[object]]::new()
    $number = 0
    foreach ($line in Get-Content -LiteralPath $Path) {
        $number++
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        try { $object = $line | ConvertFrom-Json } catch { throw "line $number of $Path is not JSON: $_" }
        $list.Add($object)
    }
    return $list
}

function Track-At($Checkpoint, [int] $Index) {
    $tracks = @($Checkpoint.tracks)
    if ($Index -lt 0 -or $Index -ge $tracks.Count) { return $null }
    return $tracks[$Index]
}

function Story-Key($Checkpoint) {
    $index = [int]$Checkpoint.track
    $track = Track-At $Checkpoint $index
    if ($null -eq $track) { return "$($Checkpoint.map):?" }
    return "$($Checkpoint.map):$($track.chapter).$($track.step).$($track.substep)"
}

function Set-Text($Values) {
    $set = @($Values) | Where-Object { $null -ne $_ } | ForEach-Object { [int]$_ } | Sort-Object -Unique
    return ($set -join ',')
}

function Quest-Text($Quests) {
    if ($null -eq $Quests) { return '' }
    $parts = @($Quests.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" } | Sort-Object)
    return ($parts -join ',')
}

$originalChecks = @(Load-CheckpointFile $Original)
$runtimeChecks = @(Load-CheckpointFile $Runtime)
$fail = 0
$reported = 0
function Fail([string] $What) {
    $script:fail++
    if ($script:reported -lt $Limit) {
        Write-Output "DIFF $What"
        $script:reported++
    } elseif ($script:reported -eq $Limit) {
        Write-Output "DIFF ... (more differences suppressed; -Limit to raise)"
        $script:reported++
    }
}
function Same([string] $What, $A, $B) {
    if ("$A" -ne "$B") { Fail "$What`: original $A, runtime $B" }
}

$runtimeByKey = @{}
for ($index = 0; $index -lt $runtimeChecks.Count; $index++) {
    $key = Story-Key $runtimeChecks[$index]
    if (-not $runtimeByKey.ContainsKey($key)) {
        $runtimeByKey[$key] = [System.Collections.Generic.List[object]]::new()
    }
    $runtimeByKey[$key].Add([pscustomobject]@{
        Index = $index
        Checkpoint = $runtimeChecks[$index]
    })
}
$usedRuntime = [System.Collections.Generic.HashSet[int]]::new()

$aligned = 0
$unmatched = 0
foreach ($or in $originalChecks) {
    $key = Story-Key $or
    $pick = $null
    if ($runtimeByKey.ContainsKey($key)) {
        $available = @($runtimeByKey[$key] |
            Where-Object { -not $usedRuntime.Contains([int]$_.Index) })
        # Same kind first: a map load, an event boundary and a control point that share a
        # story key are different moments.
        $same = @($available | Where-Object { $_.Checkpoint.kind -eq $or.kind })
        if ($same.Count -gt 0) {
            $pick = $same[0]
        } elseif ($or.kind -eq 'after') {
            $control = @($available | Where-Object { $_.Checkpoint.kind -eq 'control' })
            if ($control.Count -gt 0) { $pick = $control[0] }
        }
    }
    if ($null -eq $pick) { $unmatched++; continue }
    [void]$usedRuntime.Add([int]$pick.Index)
    $rt = $pick.Checkpoint
    $aligned++
    $where = "$key $($or.kind)"
    Same "$where map" $or.map $rt.map
    Same "$where current track" $or.track $rt.track
    if ($null -ne $or.daytime -and $null -ne $rt.daytime) {
        Same "$where daytime" ([bool]$or.daytime) ([bool]$rt.daytime)
    } elseif ($null -ne $or.daytime -or $null -ne $rt.daytime) {
        Fail "$where daytime: only one checkpoint reports it"
    }
    $orTracks = @($or.tracks)
    $rtTracks = @($rt.tracks)
    Same "$where track count" $orTracks.Count $rtTracks.Count
    for ($i = 0; $i -lt [Math]::Min($orTracks.Count, $rtTracks.Count); $i++) {
        $a = $orTracks[$i]
        $b = $rtTracks[$i]
        Same "$where track $i progress" "$($a.chapter).$($a.step).$($a.substep)" "$($b.chapter).$($b.step).$($b.substep)"
        $ac = Set-Text $a.chapter_flags
        $bc = Set-Text $b.chapter_flags
        if ($ac -ne $bc) { Fail "$where track $i chapter flags: original [$ac], runtime [$bc]" }
        $as = Set-Text $a.step_flags
        $bs = Set-Text $b.step_flags
        if ($as -ne $bs) { Fail "$where track $i step flags: original [$as], runtime [$bs]" }
    }
    $af = Set-Text $or.flags
    $bf = Set-Text $rt.flags
    if ($af -ne $bf) { Fail "$where global flags: original [$af], runtime [$bf]" }
    $aq = Quest-Text $or.quests
    $bq = Quest-Text $rt.quests
    if ($aq -ne $bq) { Fail "$where quests: original [$aq], runtime [$bq]" }
    if ($HeroTolerance -gt 0) {
        if ($null -eq $or.hero -or $null -eq $rt.hero) {
            Fail "$where hero: only one checkpoint reports it"
        } else {
            foreach ($axis in 'x', 'y', 'z') {
                $delta = [Math]::Abs([double]$or.hero.$axis - [double]$rt.hero.$axis)
                if ($delta -gt $HeroTolerance) {
                    Fail ("$where hero $axis`: original $($or.hero.$axis), runtime $($rt.hero.$axis), delta {0:N4}" -f $delta)
                }
            }
        }
    }
}

$unused = $runtimeChecks.Count - $usedRuntime.Count
Write-Output ("aligned {0} original checkpoint(s) with {1} distinct runtime checkpoint(s); original {2}, runtime {3}; unmatched original {4}, unused runtime {5}" -f `
    $aligned, $usedRuntime.Count, $originalChecks.Count, $runtimeChecks.Count, $unmatched, $unused)
if ($aligned -eq 0) {
    Write-Output 'NO PARITY: no original checkpoint has a runtime checkpoint at the same story point and kind'
    exit 1
}
if ($unmatched -gt 0) {
    Write-Output ("note: {0} original checkpoint(s) have no unused runtime counterpart and are not compared" -f $unmatched)
}
if ($fail -eq 0) { Write-Output 'OK: aligned checkpoints identical' }
if ($fail -gt 0) { exit 1 }
exit 0
