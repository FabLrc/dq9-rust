# Sorts the branches against main and deletes the ones whose work is already there.
#
# Groups:
#   merged      reachable from main (`git branch --merged main`)
#   equivalent  not merged, but every commit has a patch-identical one in main
#               (`git cherry main <b>` prints no `+` line): typically squash-merged lots
#   unique      at least one commit absent from main (`+` in `git cherry`): never deleted
# main, the current branch and any branch checked out in a worktree are skipped.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/prune-branches.ps1            # list only
#   powershell -ExecutionPolicy Bypass -File scripts/prune-branches.ps1 -Apply     # delete
#   powershell -ExecutionPolicy Bypass -File scripts/prune-branches.ps1 -Remote [-Apply]
#
# -Apply deletes the merged and equivalent local branches with `git branch -D` (-d would
# refuse the equivalent ones, which git does not see as merged). -Remote also sorts origin/*
# (against `git cherry main origin/<b>`) and, with -Apply, deletes them with
# `git push origin --delete`. Run `git fetch --prune` first for an up-to-date remote view.
param(
    [switch] $Apply,
    [switch] $Remote
)
$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot

function Invoke-Git {
    $Out = & git -C $Repo @args
    if ($LASTEXITCODE -ne 0) { throw "git $($args -join ' ') failed" }
    $Out
}

# Sorts refs (short names such as `foo` or `origin/foo`) into the three groups.
function Get-BranchGroups([string[]] $Refs, [string[]] $Merged) {
    $Groups = [ordered]@{ merged = @(); equivalent = @(); unique = @() }
    foreach ($Ref in $Refs) {
        $Date = "$(Invoke-Git log -1 '--format=%cs' $Ref)".Trim()
        if ($Merged -contains $Ref) {
            $Kind = 'merged'
            $Ahead = 0
        } else {
            $Cherry = @(Invoke-Git cherry main $Ref)
            $Ahead = @($Cherry | Where-Object { $_ -like '+*' }).Count
            $Kind = if ($Ahead -eq 0) { 'equivalent' } else { 'unique' }
        }
        $Groups[$Kind] += [pscustomobject]@{ Name = $Ref; Date = $Date; Ahead = $Ahead }
    }
    $Groups
}

function Write-Groups([string] $Title, $Groups) {
    foreach ($Kind in $Groups.Keys) {
        $Items = @($Groups[$Kind] | Sort-Object Date -Descending)
        Write-Output ''
        Write-Output ("{0} {1} ({2})" -f $Title, $Kind, $Items.Count)
        foreach ($Item in $Items) {
            $Extra = if ($Kind -eq 'unique') { "  +$($Item.Ahead) commit(s)" } else { '' }
            Write-Output ("  {0}  {1}{2}" -f $Item.Date, $Item.Name, $Extra)
        }
    }
}

$Current = "$(Invoke-Git branch --show-current)".Trim()
$InWorktree = @(Invoke-Git worktree list --porcelain |
    Where-Object { $_ -like 'branch refs/heads/*' } |
    ForEach-Object { $_.Substring('branch refs/heads/'.Length) })
$Skip = @(@('main', $Current) + $InWorktree | Where-Object { $_ } | Sort-Object -Unique)

$Local = @(Invoke-Git for-each-ref '--format=%(refname:short)' refs/heads |
    Where-Object { $Skip -notcontains $_ })
$LocalMerged = @(Invoke-Git branch --merged main '--format=%(refname:short)')
$LocalGroups = Get-BranchGroups $Local $LocalMerged

Write-Output ("skipped (main, current, worktrees): {0}" -f ($Skip -join ', '))
Write-Groups 'local' $LocalGroups

if ($Remote) {
    $RemoteRefs = @(Invoke-Git for-each-ref '--format=%(refname:short)' refs/remotes/origin |
        Where-Object { $_ -like 'origin/*' -and $_ -ne 'origin/HEAD' } |
        Where-Object { $Skip -notcontains $_.Substring('origin/'.Length) })
    $RemoteMerged = @(Invoke-Git branch -r --merged main '--format=%(refname:short)')
    $RemoteGroups = Get-BranchGroups $RemoteRefs $RemoteMerged
    Write-Groups 'remote' $RemoteGroups
}

$LocalDoomed = @($LocalGroups.merged + $LocalGroups.equivalent | ForEach-Object { $_.Name })
$RemoteDoomed = if ($Remote) {
    @($RemoteGroups.merged + $RemoteGroups.equivalent | ForEach-Object { $_.Name.Substring('origin/'.Length) })
} else { @() }

Write-Output ''
if (-not $Apply) {
    $Count = $LocalDoomed.Count + $RemoteDoomed.Count
    if ($Count -gt 0) { Write-Output "nothing deleted; pass -Apply to delete the $Count merged/equivalent branch(es)" }
    else { Write-Output 'nothing to delete' }
    exit 0
}
foreach ($Name in $LocalDoomed) {
    Invoke-Git branch -D $Name | Out-Null
    Write-Output "deleted $Name"
}
foreach ($Name in $RemoteDoomed) {
    Invoke-Git push origin --delete $Name | Out-Null
    Write-Output "deleted origin/$Name"
}
Write-Output ("deleted {0} local and {1} remote branch(es); unique branches kept" -f $LocalDoomed.Count, $RemoteDoomed.Count)
