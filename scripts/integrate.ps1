# Integration gate for a worker branch: checks it, squash-merges it into main, removes its worktree.
#
# Usage (from the main checkout, on `main`, clean tree):
#   powershell -ExecutionPolicy Bypass -File scripts/integrate.ps1 -Branch <b> -Spec build/batches/<batch>.md
#   powershell -ExecutionPolicy Bypass -File scripts/integrate.ps1 -Branch <b> -Spec <spec> -Apply -Message 'docs: summary'
#   powershell -ExecutionPolicy Bypass -File scripts/integrate.ps1 -Branch <b> -Spec <spec> -Apply -Message '...' -Cleanup
#   powershell -ExecutionPolicy Bypass -File scripts/integrate.ps1 -Branch <b> -Spec <spec> -Rebase
#   powershell -ExecutionPolicy Bypass -File scripts/integrate.ps1 -Branch <b> -Cleanup
#
# Checks (always run, in this order; the script stops at the first failure with a non-zero exit):
# - the main checkout is on `main` with a clean tree (`git status --porcelain` empty);
# - the branch exists, has a worktree (found with `git worktree list --porcelain`) and contains
#   `main` (otherwise: rebase needed; `-Rebase` runs `git rebase main` in that worktree first,
#   and a conflict aborts the rebase and fails with "rebase conflict, ask the worker");
# - every file of `git diff --name-only main...<b>` matches a glob of the spec's ```owns block
#   (`**` any path including none, `*` anything but `/`, `?` one character). README.md outside
#   the globs is accepted only when every changed line lies between `<!-- coverage:start -->`
#   and `<!-- coverage:end -->` (markers included) in both versions;
# - no forbidden file: `rom/`, `build/`, `tools/`, `*.nds`, `*.sav`;
# - the worktree is clean, `scripts/verify.ps1 -RequireRom` passes inside it (cwd = worktree),
#   and the worktree is still clean afterwards (otherwise the README progress block must be
#   committed on the branch). The verify step always uses a private Cargo target
#   (`%LOCALAPPDATA%\dq9-target-integrate`, overriding the caller: a shared target lets Cargo
#   reuse an rlib built from another worktree's sources, observed with `nds-rom` during the r1
#   wave) and sets `DQ9_ROM` to the single `*.nds` of the main checkout's `rom/` when the
#   variable is empty (fails when there is not exactly one).
# There is no option to skip a check. Without -Apply nothing is changed in main.
#
# -Apply: `git merge --squash <b>` then one commit with -Message (`domain: summary`, required).
#   Nothing is pushed.
# -Cleanup: removes the worktree and deletes the branch. Release the worker on the Orca side
#   FIRST: the worktree must not be in use. Worktrees hold junctions (`rom`, `tools`) to the main
#   checkout, which hold the user's only copy of the ROM. Every reparse point of the worktree
#   (root and nested, the scan never enters one and skips `.git`/`target` contents) is removed
#   as a link only, without following its target; the script refuses to go on if one remains,
#   then runs `git worktree remove` (never forced) and `git branch -D`. Tracked files hidden by a
#   removed junction (such as rom/README.md) are checked out again so the worktree stays clean.
#   After the worktree is gone, the batch Cargo target `%LOCALAPPDATA%\dq9-target-<batch>`
#   (the part of -Branch after the last `/`, when it matches [A-Za-z0-9._-]+) is deleted: each
#   per-batch target costs ~5 GB, and the 13 targets of a wave filled the disk (os error 112).
#   `dq9-target-integrate` and any other target are never touched.
#   With -Apply the cleanup follows the commit. Without -Apply it is a cleanup-only run: -Spec
#   is not needed, and the branch must already be integrated (ancestor of main, or main holds
#   the same content for every file the branch changed). A branch without a worktree is only
#   deleted.
param(
    [Parameter(Mandatory = $true)] [string] $Branch,
    [string] $Spec,
    [string] $Message,
    [switch] $Apply,
    [switch] $Rebase,
    [switch] $Cleanup
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Checks = [System.Collections.Generic.List[string]]::new()

function Write-Summary {
    Write-Host ''
    Write-Host "integrate.ps1 $Branch"
    foreach ($Line in $Checks) { Write-Host "  $Line" }
}

function Pass([string] $Name, [string] $Detail) {
    $Line = 'ok      {0}: {1}' -f $Name, $Detail
    $Checks.Add($Line)
    Write-Host "==> $Line"
}

function Fail([string] $Name, [string] $Detail) {
    $Line = 'FAILED  {0}: {1}' -f $Name, $Detail
    $Checks.Add($Line)
    Write-Host "==> $Line"
    Write-Summary
    exit 1
}

# Runs git and returns its stdout lines; a non-zero exit code is a failure of check $Name.
function Invoke-Git([string] $Name, [string[]] $GitArgs) {
    $Output = @(& git -c core.quotePath=false @GitArgs)
    if ($LASTEXITCODE -ne 0) { Fail $Name "git $($GitArgs -join ' ') exited with $LASTEXITCODE" }
    , $Output
}

# Converts an ```owns glob to an anchored, case-sensitive regex.
function Convert-GlobToRegex([string] $Glob) {
    $G = $Glob.Trim().Replace('\', '/')
    if ($G.StartsWith('./')) { $G = $G.Substring(2) }
    if ($G.EndsWith('/')) { $G += '**' }
    $Sb = [System.Text.StringBuilder]::new('^')
    $I = 0
    while ($I -lt $G.Length) {
        $C = [string] $G[$I]
        if ($C -eq '*') {
            if ($I + 1 -lt $G.Length -and [string] $G[$I + 1] -eq '*') {
                $I += 2
                if ($I -lt $G.Length -and [string] $G[$I] -eq '/') {
                    # `**/` also matches zero directories.
                    [void] $Sb.Append('(?:.*/)?')
                    $I++
                } else {
                    [void] $Sb.Append('.*')
                }
                continue
            }
            [void] $Sb.Append('[^/]*')
        } elseif ($C -eq '?') {
            [void] $Sb.Append('[^/]')
        } else {
            [void] $Sb.Append([regex]::Escape($C))
        }
        $I++
    }
    [void] $Sb.Append('$')
    $Sb.ToString()
}

# Returns the globs of every ```owns block of a spec file (blank lines and `#` lines ignored).
function Read-OwnsGlobs([string] $Path) {
    $Globs = [System.Collections.Generic.List[string]]::new()
    $InBlock = $false
    foreach ($Line in [System.IO.File]::ReadAllLines($Path)) {
        if (-not $InBlock) {
            if ($Line -match '^\s*```owns\s*$') { $InBlock = $true }
            continue
        }
        if ($Line -match '^\s*```') { $InBlock = $false; continue }
        $Trimmed = $Line.Trim()
        if ($Trimmed -eq '' -or $Trimmed.StartsWith('#')) { continue }
        $Globs.Add($Trimmed)
    }
    if ($InBlock) { throw 'unterminated ```owns block' }
    , $Globs.ToArray()
}

# 1-based line numbers of the coverage markers in a README version, or $null.
function Get-CoverageBlock([string[]] $Lines) {
    $Starts = @(); $Ends = @()
    for ($I = 0; $I -lt $Lines.Count; $I++) {
        $T = $Lines[$I].Trim()
        if ($T -eq '<!-- coverage:start -->') { $Starts += $I + 1 }
        if ($T -eq '<!-- coverage:end -->') { $Ends += $I + 1 }
    }
    if ($Starts.Count -ne 1 -or $Ends.Count -ne 1 -or $Starts[0] -ge $Ends[0]) { return $null }
    @{ Start = $Starts[0]; End = $Ends[0] }
}

# True when a hunk side (start, count) lies inside [Lo, Hi]; count 0 is an insertion/removal
# point after line `start`, which must sit between two lines of the block.
function Test-HunkInside([int] $Start, [int] $Count, [int] $Lo, [int] $Hi) {
    if ($Count -eq 0) { return ($Start -ge $Lo -and $Start + 1 -le $Hi) }
    ($Start -ge $Lo -and $Start + $Count - 1 -le $Hi)
}

# True when every line README.md changes between $Base and $Head lies in the coverage block.
function Test-CoverageOnlyReadme([string] $Base, [string] $Head) {
    foreach ($Rev in @($Base, $Head)) {
        $Present = @(& git ls-tree --name-only $Rev -- README.md)
        if ($LASTEXITCODE -ne 0 -or $Present.Count -eq 0) { return $false }
    }
    $Old = Get-CoverageBlock @(& git show "${Base}:README.md")
    $New = Get-CoverageBlock @(& git show "${Head}:README.md")
    if ($null -eq $Old -or $null -eq $New) { return $false }
    $Diff = @(& git diff --no-ext-diff --no-color -U0 $Base $Head -- README.md)
    if ($LASTEXITCODE -ne 0) { return $false }
    $Hunks = 0
    foreach ($Line in $Diff) {
        if ($Line -notmatch '^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@') { continue }
        $Hunks++
        $OldCount = if ($Matches[2]) { [int] $Matches[2] } else { 1 }
        $NewCount = if ($Matches[4]) { [int] $Matches[4] } else { 1 }
        if (-not (Test-HunkInside ([int] $Matches[1]) $OldCount $Old.Start $Old.End)) { return $false }
        if (-not (Test-HunkInside ([int] $Matches[3]) $NewCount $New.Start $New.End)) { return $false }
    }
    $Hunks -gt 0
}

# Worktree path of a branch from `git worktree list --porcelain`, or $null.
function Find-Worktree([string] $Name) {
    $Lines = Invoke-Git 'worktree' @('worktree', 'list', '--porcelain')
    $Path = $null
    foreach ($Line in $Lines) {
        if ($Line.StartsWith('worktree ')) { $Path = $Line.Substring(9) }
        elseif ($Line -eq "branch refs/heads/$Name") { return [System.IO.Path]::GetFullPath($Path) }
    }
    $null
}

# Every reparse point under $Dir (junctions, symlinks). Never enters one; skips `.git`/`target`
# contents (a reparse point named `.git` or `target` is still reported).
function Get-ReparsePoints([string] $Dir) {
    $Found = [System.Collections.Generic.List[string]]::new()
    $Stack = [System.Collections.Generic.Stack[System.IO.DirectoryInfo]]::new()
    $Stack.Push([System.IO.DirectoryInfo]::new($Dir))
    while ($Stack.Count -gt 0) {
        $Current = $Stack.Pop()
        foreach ($Entry in $Current.EnumerateFileSystemInfos()) {
            if ($Entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                $Found.Add($Entry.FullName)
                continue
            }
            if ($Entry -is [System.IO.DirectoryInfo] -and $Entry.Name -ne '.git' -and $Entry.Name -ne 'target') {
                $Stack.Push($Entry)
            }
        }
    }
    , $Found.ToArray()
}

function Test-Clean([string] $Name, [string] $Dir) {
    $Status = Invoke-Git $Name @('-C', $Dir, 'status', '--porcelain')
    if ($Status.Count -gt 0) {
        $Status | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" }
        return $false
    }
    $true
}

function Invoke-Cleanup([string] $Worktree) {
    if ([string]::IsNullOrEmpty($Worktree)) {
        Pass 'worktree' 'none for this branch: only the branch is deleted'
    } else {
        if ($Worktree.TrimEnd('\') -eq $Root.TrimEnd('\')) { Fail 'worktree' 'refusing to remove the main checkout' }
        if (-not (Test-Path -LiteralPath $Worktree -PathType Container)) {
            Fail 'worktree' "$Worktree is registered but missing: run 'git worktree prune'"
        }
        $RootInfo = [System.IO.DirectoryInfo]::new($Worktree)
        if ($RootInfo.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            Fail 'junctions' "worktree root $Worktree is itself a reparse point; remove it by hand"
        }
        try {
            $Points = Get-ReparsePoints $Worktree
        } catch {
            Fail 'junctions' "cannot scan ${Worktree}: $($_.Exception.Message)"
        }
        foreach ($Point in $Points) {
            # Remove the link itself: a non-recursive delete never follows the target.
            try {
                $Attributes = [System.IO.File]::GetAttributes($Point)
                if ($Attributes -band [System.IO.FileAttributes]::Directory) {
                    [System.IO.Directory]::Delete($Point, $false)
                } else {
                    [System.IO.File]::Delete($Point)
                }
                Write-Host "    removed link $Point"
            } catch {
                Fail 'junctions' "cannot remove link ${Point}: $($_.Exception.Message)"
            }
        }
        try {
            $Left = Get-ReparsePoints $Worktree
        } catch {
            Fail 'junctions' "cannot rescan ${Worktree}: $($_.Exception.Message)"
        }
        if ($Left.Count -gt 0) {
            Fail 'junctions' ("{0} reparse point(s) left, worktree kept: {1}" -f $Left.Count, ($Left -join ', '))
        }
        Pass 'junctions' ("{0} link(s) removed, none left" -f $Points.Count)

        # A junction may have replaced a tracked directory (rom/ holds rom/README.md).
        $Restored = 0
        foreach ($Point in $Points) {
            $Relative = $Point.Substring($Worktree.TrimEnd('\').Length).TrimStart('\', '/').Replace('\', '/')
            $Tracked = Invoke-Git 'restore' @('-C', $Worktree, '--literal-pathspecs', 'ls-files', '--', $Relative)
            if ($Tracked.Count -gt 0) {
                Invoke-Git 'restore' @('-C', $Worktree, '--literal-pathspecs', 'checkout', '--', $Relative) | Out-Null
                $Restored++
            }
        }
        if ($Restored -gt 0) { Pass 'restore' "$Restored tracked path(s) checked out again" }

        & git worktree remove $Worktree
        if ($LASTEXITCODE -ne 0) { Fail 'worktree remove' "git worktree remove $Worktree exited with $LASTEXITCODE" }
        if (Test-Path -LiteralPath $Worktree) { Fail 'worktree remove' "$Worktree still exists" }
        Pass 'worktree remove' $Worktree
    }

    # The batch Cargo target costs ~5 GB. Only the directory named after the batch (the part of
    # -Branch after the last `/`) is removed, never dq9-target-integrate or another target.
    $Batch = $Branch.Substring($Branch.LastIndexOf('/') + 1)
    $TargetDir = Join-Path $env:LOCALAPPDATA "dq9-target-$Batch"
    $IntegrateDir = Join-Path $env:LOCALAPPDATA 'dq9-target-integrate'
    $IsIntegrate = [System.IO.Path]::GetFullPath($TargetDir) -eq [System.IO.Path]::GetFullPath($IntegrateDir)
    if ($Batch -match '^[A-Za-z0-9._-]+$' -and -not $IsIntegrate -and
        (Test-Path -LiteralPath $TargetDir -PathType Container) -and
        -not (([System.IO.DirectoryInfo]::new($TargetDir).Attributes) -band [System.IO.FileAttributes]::ReparsePoint)) {
        $Bytes = (Get-ChildItem -LiteralPath $TargetDir -Recurse -File -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
        if ($null -eq $Bytes) { $Bytes = 0 }
        try {
            Remove-Item -LiteralPath $TargetDir -Recurse -Force
        } catch {
            Fail 'target' "cannot remove ${TargetDir}: $($_.Exception.Message)"
        }
        if (Test-Path -LiteralPath $TargetDir) { Fail 'target' "$TargetDir still exists" }
        Pass 'target' ("{0} removed ({1:N1} GB freed)" -f $TargetDir, ($Bytes / 1GB))
    }

    Invoke-Git 'branch delete' @('branch', '-D', $Branch) | Out-Null
    Pass 'branch delete' $Branch
}

Push-Location $Root
try {
    $SavedEncoding = [Console]::OutputEncoding
    try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }
    $CleanupOnly = $Cleanup -and -not $Apply

    # Arguments.
    if ($Branch -eq 'main') { Fail 'arguments' 'cannot integrate main into itself' }
    if (-not $CleanupOnly -and -not $Spec) { Fail 'arguments' '-Spec is required (except for a cleanup-only run)' }
    if ($Apply) {
        if (-not $Message) { Fail 'arguments' '-Message is required with -Apply' }
        if ($Message -notmatch '^[a-z][a-z0-9._/-]*: \S[^\r\n]*$') {
            Fail 'arguments' "-Message must be one line 'domain: summary' (got '$Message')"
        }
    }

    # Main checkout.
    $Top = (Invoke-Git 'main checkout' @('rev-parse', '--show-toplevel'))[0]
    $MainWorktree = (Invoke-Git 'main checkout' @('worktree', 'list', '--porcelain'))[0].Substring(9)
    if ([System.IO.Path]::GetFullPath($Top) -ne [System.IO.Path]::GetFullPath($MainWorktree)) {
        Fail 'main checkout' "run from the main checkout $MainWorktree, not a linked worktree"
    }
    $Current = (Invoke-Git 'main checkout' @('rev-parse', '--abbrev-ref', 'HEAD'))[0]
    if ($Current -ne 'main') { Fail 'main checkout' "current branch is '$Current', not main" }
    if (-not (Test-Clean 'main checkout' $Root)) { Fail 'main checkout' 'tree not clean (git status --porcelain)' }
    $MainHead = (Invoke-Git 'main checkout' @('rev-parse', 'HEAD'))[0]
    Pass 'main checkout' "main at $($MainHead.Substring(0, 7)), clean"

    & git rev-parse --verify --quiet "refs/heads/$Branch" | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail 'branch' "no local branch '$Branch'" }
    $BranchHead = (Invoke-Git 'branch' @('rev-parse', "refs/heads/$Branch"))[0]
    Pass 'branch' "$Branch at $($BranchHead.Substring(0, 7))"

    $Worktree = Find-Worktree $Branch

    if ($CleanupOnly) {
        & git merge-base --is-ancestor "refs/heads/$Branch" main
        if ($LASTEXITCODE -eq 0) {
            Pass 'integrated' 'branch is an ancestor of main'
        } else {
            $Changed = Invoke-Git 'integrated' @('diff', '--name-only', '--no-renames', "main...refs/heads/$Branch")
            if ($Changed.Count -eq 0) { Fail 'integrated' 'branch has no change of its own but is not in main' }
            & git --literal-pathspecs diff --quiet main "refs/heads/$Branch" -- @Changed
            if ($LASTEXITCODE -ne 0) {
                Fail 'integrated' 'main differs from the branch on its changed files: integrate it first (-Apply)'
            }
            Pass 'integrated' ("main holds the branch content of its {0} changed file(s)" -f $Changed.Count)
        }
        Invoke-Cleanup $Worktree
        Write-Summary
        exit 0
    }

    # Spec.
    $SpecPath = (Resolve-Path -LiteralPath $Spec -ErrorAction SilentlyContinue)
    if ($null -eq $SpecPath) { Fail 'spec' "spec file not found: $Spec" }
    try { $Globs = Read-OwnsGlobs $SpecPath.ProviderPath } catch { Fail 'spec' "$Spec`: $($_.Exception.Message)" }
    if ($Globs.Count -eq 0) { Fail 'spec' "no glob in a ``````owns block of $Spec" }
    $Regexes = @($Globs | ForEach-Object { Convert-GlobToRegex $_ })
    Pass 'spec' ("{0} owned glob(s) in {1}" -f $Globs.Count, $Spec)

    if ($null -eq $Worktree) { Fail 'worktree' "no worktree has '$Branch' checked out (git worktree list)" }
    if (-not (Test-Path -LiteralPath $Worktree)) { Fail 'worktree' "$Worktree is missing (git worktree prune?)" }
    Pass 'worktree' $Worktree

    & git merge-base --is-ancestor main "refs/heads/$Branch"
    if ($LASTEXITCODE -ne 0) {
        if (-not $Rebase) { Fail 'contains main' "rebase needed: $Branch does not contain main ($($MainHead.Substring(0, 7)))" }
        Write-Host "==> rebase: git rebase main (in $Worktree)"
        & git -C $Worktree rebase main | Out-Host
        if ($LASTEXITCODE -ne 0) {
            & git -C $Worktree rebase --abort | Out-Null
            Fail 'contains main' 'rebase conflict, ask the worker'
        }
        $BranchHead = (Invoke-Git 'rebase' @('rev-parse', "refs/heads/$Branch"))[0]
        Pass 'rebase' "rebased on main, branch at $($BranchHead.Substring(0, 7))"
    }
    Pass 'contains main' 'yes'

    $Files = Invoke-Git 'changes' @('diff', '--name-only', '--no-renames', "main...refs/heads/$Branch")
    if ($Files.Count -eq 0) { Fail 'changes' 'no change to integrate' }
    Pass 'changes' ("{0} file(s)" -f $Files.Count)

    $Forbidden = @($Files | Where-Object { $_ -match '^(rom|build|tools)/' -or $_ -match '\.(nds|sav)$' })
    if ($Forbidden.Count -gt 0) { Fail 'forbidden files' ($Forbidden -join ', ') }
    Pass 'forbidden files' 'none'

    $Unowned = [System.Collections.Generic.List[string]]::new()
    $ReadmeCoverage = $false
    foreach ($File in $Files) {
        $Owned = $false
        foreach ($Regex in $Regexes) {
            if ([regex]::IsMatch($File, $Regex)) { $Owned = $true; break }
        }
        if ($Owned) { continue }
        if ($File -eq 'README.md' -and (Test-CoverageOnlyReadme $MainHead "refs/heads/$Branch")) {
            $ReadmeCoverage = $true
            continue
        }
        $Unowned.Add($File)
    }
    if ($Unowned.Count -gt 0) { Fail 'ownership' ("not covered by ``````owns: {0}" -f ($Unowned -join ', ')) }
    $Detail = 'every file matches the owns globs'
    if ($ReadmeCoverage) { $Detail += ' (README.md: coverage block only)' }
    Pass 'ownership' $Detail

    if (-not (Test-Clean 'worktree clean' $Worktree)) { Fail 'worktree clean' "$Worktree has uncommitted changes" }
    $WorktreeHead = (Invoke-Git 'worktree clean' @('-C', $Worktree, 'rev-parse', 'HEAD'))[0]
    if ($WorktreeHead -ne $BranchHead) { Fail 'worktree clean' "worktree HEAD $WorktreeHead is not the branch head" }
    Pass 'worktree clean' 'before verify'

    # Verify environment: a private target (a shared one lets Cargo reuse an rlib built from
    # another worktree's sources) and the main checkout's ROM when DQ9_ROM is empty.
    $env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\dq9-target-integrate"
    if ([string]::IsNullOrEmpty($env:DQ9_ROM)) {
        $RomDir = Join-Path $Root 'rom'
        $Roms = @(Get-ChildItem -LiteralPath $RomDir -File -Filter '*.nds' -ErrorAction SilentlyContinue)
        if ($Roms.Count -ne 1) {
            Fail 'rom' "expected exactly one *.nds in $RomDir, found $($Roms.Count): set DQ9_ROM"
        }
        $env:DQ9_ROM = $Roms[0].FullName
    }
    Pass 'verify env' "CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR, DQ9_ROM=$env:DQ9_ROM"

    $VerifyScript = Join-Path $Worktree 'scripts/verify.ps1'
    if (-not (Test-Path -LiteralPath $VerifyScript)) { Fail 'verify' "$VerifyScript not found" }
    Write-Host "==> verify: powershell -File $VerifyScript -RequireRom (in $Worktree)"
    Push-Location -LiteralPath $Worktree
    try {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $VerifyScript -RequireRom | Out-Host
        $VerifyCode = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($VerifyCode -ne 0) { Fail 'verify' "verify.ps1 -RequireRom exited with $VerifyCode" }
    Pass 'verify' 'verify.ps1 -RequireRom passed in the worktree'

    if (-not (Test-Clean 'worktree clean' $Worktree)) {
        Fail 'worktree clean' 'verify changed the worktree (README progress block?): commit it on the branch'
    }
    Pass 'worktree clean' 'after verify'

    if ($Apply) {
        $Head = (Invoke-Git 'apply' @('rev-parse', 'HEAD'))[0]
        if ($Head -ne $MainHead) { Fail 'apply' 'main moved during verification; run again' }
        if (-not (Test-Clean 'apply' $Root)) { Fail 'apply' 'main checkout changed during verification' }
        $Now = (Invoke-Git 'apply' @('rev-parse', "refs/heads/$Branch"))[0]
        if ($Now -ne $BranchHead) { Fail 'apply' 'branch moved during verification; run again' }
        & git merge --squash $BranchHead | Out-Host
        if ($LASTEXITCODE -ne 0) { Fail 'apply' "git merge --squash failed; restore main with 'git reset --merge'" }
        & git commit --quiet -m $Message | Out-Host
        if ($LASTEXITCODE -ne 0) { Fail 'apply' "git commit failed; staged squash left in main ('git reset --merge' to drop it)" }
        & git diff --quiet HEAD $BranchHead
        if ($LASTEXITCODE -ne 0) { Fail 'apply' 'main tree differs from the branch tree after the squash' }
        $Commit = (Invoke-Git 'apply' @('log', '-1', '--format=%h %s'))[0]
        Pass 'apply' "squash commit $Commit (not pushed)"
    } else {
        Pass 'apply' 'dry run: main unchanged (pass -Apply -Message to commit)'
    }

    if ($Cleanup) { Invoke-Cleanup $Worktree }
    Write-Summary
} finally {
    try { [Console]::OutputEncoding = $SavedEncoding } catch { }
    Pop-Location
}
