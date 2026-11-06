# Prunes large emulator state files under a build directory, keeping named checkpoints.
#
# The differential runs leave hundreds of multi-megabyte states (`.ml1`, `.state*`) under
# `build/`. They are ignored by git but fill the disk and slow archiving. This script lists
# what it would delete; nothing is removed without `-Apply`.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/prune-build.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/prune-build.ps1 -Days 7 -Apply
#   powershell -ExecutionPolicy Bypass -File scripts/prune-build.ps1 -Keep 'bat*','pro1*' -Apply
#
# Only files with one of `-Extensions` (default: savestates) that are older than `-Days` and
# at least `-MinSizeMB` are candidates. `-Keep` matches the file name or its path relative to
# `-Root` with PowerShell wildcards. The script never deletes `.sav`, `.nds`, `.json`,
# `.jsonl` or images, and refuses a root that is the repository itself.
param(
    [string] $Root = 'build',
    [int] $Days = 3,
    [string[]] $Keep = @('bat*', 'pro1*', 'pro11*', '*current*'),
    [string[]] $Extensions = @('.ml1', '.state', '.state1', '.state2', '.state3', '.state4'),
    [double] $MinSizeMB = 1,
    [switch] $Apply
)
$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot
$RootPath = (Resolve-Path (Join-Path $Repo $Root) -ErrorAction Stop).Path
if ($RootPath -eq $Repo) { throw 'give a subdirectory such as build, not the repository root' }
$Cutoff = (Get-Date).AddDays(-$Days)
$MinBytes = [int64]($MinSizeMB * 1MB)

$Candidates = @(Get-ChildItem -LiteralPath $RootPath -Recurse -File -ErrorAction Stop |
    Where-Object {
        $Extensions -contains $_.Extension -and
        $_.Length -ge $MinBytes -and
        $_.LastWriteTime -lt $Cutoff
    })

$Doomed = [System.Collections.Generic.List[object]]::new()
$Kept = 0
foreach ($File in $Candidates) {
    $Relative = $File.FullName.Substring($RootPath.Length).TrimStart('\', '/')
    $Protected = $false
    foreach ($Pattern in $Keep) {
        if ($File.Name -like $Pattern -or $Relative -like $Pattern) { $Protected = $true; break }
    }
    if ($Protected) { $Kept++ } else { $Doomed.Add($File) }
}

$Freed = ($Doomed | Measure-Object -Property Length -Sum).Sum
if ($null -eq $Freed) { $Freed = 0 }
$Mode = if ($Apply) { 'delete' } else { 'dry run' }
Write-Output ("prune {0}: root {1}, {2} candidate(s), {3} kept, {4} to remove, {5:N1} MiB ({6})" -f `
    $Root, $RootPath, $Candidates.Count, $Kept, $Doomed.Count, ($Freed / 1MB), $Mode)
$Doomed | Sort-Object Length -Descending | Select-Object -First 10 | ForEach-Object {
    Write-Output ("  {0,8:N1} MiB  {1}" -f ($_.Length / 1MB), $_.FullName.Substring($RootPath.Length + 1))
}
if ($Doomed.Count -gt 10) { Write-Output ("  ... and {0} more" -f ($Doomed.Count - 10)) }

if ($Apply) {
    foreach ($File in $Doomed) {
        Remove-Item -LiteralPath $File.FullName -Force
    }
    Write-Output ("removed {0} file(s), freed {1:N1} MiB" -f $Doomed.Count, ($Freed / 1MB))
} elseif ($Doomed.Count -gt 0) {
    Write-Output 'nothing removed; pass -Apply to delete these files'
}
