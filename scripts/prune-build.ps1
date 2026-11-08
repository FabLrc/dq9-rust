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
# `-Root` with PowerShell wildcards. Files listed in any `build/evidence/<lot>/manifest.json`
# of the repository (written by scripts/evidence.ps1) are always kept, whatever `-Root`.
# The script never deletes `.sav`, `.nds`, `.json`, `.jsonl` or images, and refuses a root
# that is the repository itself.
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
# Evidence cited by the docs: every file listed in a lot manifest is protected.
$Evidence = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$EvidenceRoot = Join-Path (Join-Path $Repo 'build') 'evidence'
if (Test-Path -LiteralPath $EvidenceRoot -PathType Container) {
    foreach ($LotDir in Get-ChildItem -LiteralPath $EvidenceRoot -Directory) {
        $ManifestPath = Join-Path $LotDir.FullName 'manifest.json'
        if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) { continue }
        $Manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
        foreach ($Entry in @($Manifest.files)) {
            $Path = Join-Path $LotDir.FullName ($Entry.path -replace '/', [System.IO.Path]::DirectorySeparatorChar)
            [void]$Evidence.Add([System.IO.Path]::GetFullPath($Path))
        }
    }
}

$Candidates = @(Get-ChildItem -LiteralPath $RootPath -Recurse -File -ErrorAction Stop |
    Where-Object {
        $Extensions -contains $_.Extension -and
        $_.Length -ge $MinBytes -and
        $_.LastWriteTime -lt $Cutoff
    })

$Doomed = [System.Collections.Generic.List[object]]::new()
$Kept = 0
$ByManifest = 0
foreach ($File in $Candidates) {
    if ($Evidence.Contains($File.FullName)) { $ByManifest++; continue }
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
Write-Output ("prune {0}: root {1}, {2} candidate(s), {3} kept, {4} protected by manifest, {5} to remove, {6:N1} MiB ({7})" -f `
    $Root, $RootPath, $Candidates.Count, $Kept, $ByManifest, $Doomed.Count, ($Freed / 1MB), $Mode)
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
