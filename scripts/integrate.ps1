# Integration gate for worker branches: checks them, squash-merges them into main, removes
# their worktrees. One run integrates one or several branches and runs ONE verify.
#
# Usage (from the main checkout, on `main`, clean tree):
#   powershell -ExecutionPolicy Bypass -File scripts/integrate.ps1 -Branch <b> -Spec build/batches/<batch>.md
#   ./scripts/integrate.ps1 -Branch a,b,c -Apply -Message 'x: first','y: second','z: third' -Cleanup
#   ./scripts/integrate.ps1 -Branch a,b,c -Rebase          # dry run: checks + verify, main untouched
#   ./scripts/integrate.ps1 -Branch <b> -Cleanup           # cleanup-only run, after an integration
#
# -Branch takes a list (`-Branch a,b,c`, also as one comma-separated string under
# `powershell -File`); the order is the integration order. -Spec is a list matched by position
# (default: `build/batches/<batch>.md`, <batch> = the part of the branch after the last `/`).
# -Message is a list matched by position (commas are NOT split: from `powershell -File`, give
# one branch, or put a line `Commit: domain: summary` in the spec, used when -Message has no
# entry for that position).
#
# Checks, in this order; the script stops at the first failure with a non-zero exit:
# - the main checkout is on `main` with a clean tree (`git status --porcelain` empty);
# - per branch (cheap checks first): the branch exists, has a worktree (found with
#   `git worktree list --porcelain`) and contains `main` (otherwise: rebase needed; `-Rebase`
#   runs `git rebase main` in that worktree first, and a conflict aborts the rebase and fails
#   with "rebase conflict, ask the worker"); every file of `git diff --name-only
#   main...<b>` matches a glob of the spec's ```owns block (`**` any path including none, `*`
#   anything but `/`, `?` one character; README.md outside the globs is accepted only when
#   every changed line lies between `<!-- coverage:start -->` and `<!-- coverage:end -->`);
#   no forbidden file (`rom/`, `build/`, `tools/`, `*.nds`, `*.sav`); the worktree is clean
#   and at the branch head;
# - the combined result is built in a TEMPORARY integration worktree (detached at main, under
#   `%LOCALAPPDATA%\dq9-integrate-wt-*`, removed at the end; neither the main checkout nor a
#   batch worktree): each branch is squash-merged there as its own commit. A conflict (with
#   main or an earlier branch) fails and names the branch;
# - `scripts/verify.ps1 -RequireRom` of the combined tree runs ONCE in that worktree (cwd =
#   worktree). Red: the prefixes of the ordered list are verified by binary search and the
#   first culprit is reported (a prefix = main + the first k branches); main stays untouched.
#   The script runs the verify script found in the tree being verified, by a fixed relative
#   path: there is no switch, variable or parameter that replaces or skips it. The verify
#   step always uses a private Cargo target (`%LOCALAPPDATA%\dq9-target-integrate`,
#   overriding the caller: a shared target lets Cargo reuse an rlib built from another
#   worktree's sources, observed with `nds-rom` during the r1 wave) and sets `DQ9_ROM` to the
#   single `*.nds` of the main checkout's `rom/` when the variable is empty;
# - after a green verify the only change allowed in the integration worktree is the README
#   progress block that `verify.ps1` regenerates: it is folded into the LAST integration
#   commit (never into a worker branch, never a commit of its own). Any other change fails.
# There is no option to skip a check. Without -Apply nothing is changed in main.
#
# -Apply: fast-forwards `main` to the verified integration commits (one squash commit per
#   branch, messages `domain: summary`, required), then appends `<batch> <commit sha> <ISO
#   date>` per branch to `build/batches/integrated.txt` (main checkout, ignored by git) so
#   `queue.ps1` still sees a batch after its branch is pruned. Nothing is pushed.
# -Cleanup: per branch, in this order:
#   1. when `build/batches/runs.json` (main checkout, written by dispatch.ps1) has an entry
#      for the batch (key = batch, or `branch` equal to the branch), runs
#      `orca orchestration worker-release --dispatch <id>` (`$env:DQ9_ORCA` replaces `orca`)
#      so the worker's terminal no longer holds the worktree; a failure is reported as a
#      warning, not fatal;
#   2. removes the worktree and deletes the branch. Worktrees hold junctions (`rom`, `tools`)
#      to the main checkout, which hold the user's only copy of the ROM. Every reparse point
#      of the worktree (root and nested, the scan never enters one and skips `.git`/`target`
#      contents) is removed as a link only, without following its target; the script
#      refuses to go on if one remains, then runs `git worktree remove` (never forced) and
#      `git branch -D`. Tracked files hidden by a removed junction (such as rom/README.md) are
#      checked out again so the worktree stays clean;
#   3. deletes the legacy per-batch Cargo target `%LOCALAPPDATA%\dq9-target-<batch>` (the part
#      of the branch after the last `/`, when it matches [A-Za-z0-9._-]+; ~5 GB each). Any
#      `dq9-target-slot*` (long-lived targets reused by successive workers) and
#      `dq9-target-integrate` are never touched;
#   4. deletes the runs.json entry.
#   With -Apply the cleanup follows the fast-forward. Without -Apply it is a cleanup-only run:
#   -Spec is not needed, and every branch must already be integrated (ancestor of main, or
#   main holds the same content for every file the branch changed). A branch without a
#   worktree is only deleted.
param(
    [Parameter(Mandatory = $true)] [string[]] $Branch,
    [string[]] $Spec,
    [string[]] $Message,
    [switch] $Apply,
    [switch] $Rebase,
    [switch] $Cleanup
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Checks = [System.Collections.Generic.List[string]]::new()
$script:TempWorktree = $null

# `a,b` and `'a','b'` both give two items (`powershell -File` passes `a,b` as one string).
function Split-List([string[]] $Values) {
    @($Values | ForEach-Object { "$_" -split ',' } | ForEach-Object { $_.Trim() } |
        Where-Object { $_ -ne '' })
}
$Branches = @(Split-List $Branch)

function Write-Summary {
    Write-Host ''
    Write-Host "integrate.ps1 $($Branches -join ', ')"
    foreach ($Line in $Checks) { Write-Host "  $Line" }
}

function Pass([string] $Name, [string] $Detail) {
    $Line = 'ok      {0}: {1}' -f $Name, $Detail
    $Checks.Add($Line)
    Write-Host "==> $Line"
}

function Warn([string] $Name, [string] $Detail) {
    $Line = 'WARNING {0}: {1}' -f $Name, $Detail
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

# Check name with the batch appended when several branches are integrated.
function Label([string] $Name, $B) {
    if ($Branches.Count -gt 1) { "$Name [$($B.Batch)]" } else { $Name }
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

# The commit subject of a spec: its first line `Commit: domain: summary`, or $null.
function Read-SpecCommit([string] $Path) {
    foreach ($Line in [System.IO.File]::ReadAllLines($Path)) {
        if ($Line -match '^Commit:\s*(\S.*?)\s*$') { return $Matches[1] }
    }
    $null
}

# Appends the trace `queue.ps1` reads for an integrated batch whose branch is pruned:
# `<batch> <commit sha> <ISO date>` in `build/batches/integrated.txt` (main checkout, ignored
# by git). Called once per branch after `-Apply`.
function Add-IntegrationTrace([string] $Batch, [string] $Sha) {
    $Path = Join-Path $Root 'build/batches/integrated.txt'
    $Parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $Parent -PathType Container)) {
        New-Item -ItemType Directory -Path $Parent -Force | Out-Null
    }
    $Line = '{0} {1} {2}' -f $Batch, $Sha, (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    [System.IO.File]::AppendAllText($Path, $Line + "`n", [System.Text.UTF8Encoding]::new($false))
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

function Get-LocalAppData {
    if ([string]::IsNullOrEmpty($env:LOCALAPPDATA)) { Fail 'environment' '%LOCALAPPDATA% is empty' }
    $env:LOCALAPPDATA
}

# --- build/batches/runs.json (written by dispatch.ps1, ignored by git) -------------------------

function Get-RunsPath { Join-Path $Root 'build/batches/runs.json' }

# The parsed runs.json, or $null when absent or empty. An unreadable file is a failure: the
# release of a worker must not be silently skipped.
function Read-Runs {
    $Path = Get-RunsPath
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $Text = [System.IO.File]::ReadAllText($Path)
    if ([string]::IsNullOrWhiteSpace($Text)) { return $null }
    try { $Text | ConvertFrom-Json } catch { Fail 'runs.json' "cannot parse ${Path}: $($_.Exception.Message)" }
}

# The runs.json entry of a branch: key = batch, or a `branch` field equal to the branch.
function Find-Run($B) {
    $Runs = Read-Runs
    if ($null -eq $Runs) { return $null }
    foreach ($Property in $Runs.PSObject.Properties) {
        if ($Property.Name -eq $B.Batch -or ($Property.Value -and $Property.Value.branch -eq $B.Name)) {
            return @{ Key = $Property.Name; Dispatch = [string] $Property.Value.dispatch }
        }
    }
    $null
}

function Remove-Run([string] $Key) {
    $Runs = Read-Runs
    if ($null -eq $Runs) { return }
    $Runs.PSObject.Properties.Remove($Key)
    $Json = ConvertTo-Json -InputObject $Runs -Depth 6
    [System.IO.File]::WriteAllText((Get-RunsPath), $Json, [System.Text.UTF8Encoding]::new($false))
}

# Runs `orca orchestration worker-release`; returns $null on success, else the error text.
function Invoke-WorkerRelease([string] $Dispatch) {
    $Orca = if ([string]::IsNullOrEmpty($env:DQ9_ORCA)) { 'orca' } else { $env:DQ9_ORCA }
    $Saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $global:LASTEXITCODE = 0
        $Out = @(& $Orca orchestration worker-release --dispatch $Dispatch 2>&1 | ForEach-Object { "$_" })
        if ($LASTEXITCODE -ne 0) { return "exit $LASTEXITCODE`: $($Out -join ' ')" }
        foreach ($Line in $Out) { Write-Host "    $Line" }
        return $null
    } catch {
        return $_.Exception.Message
    } finally {
        $ErrorActionPreference = $Saved
    }
}

function Invoke-Cleanup($B) {
    $Worktree = $B.Worktree

    # The worker's terminal must be released first: it is what keeps `git worktree remove`
    # from succeeding. Only the terminal is closed, never the worktree or its junctions.
    $Run = Find-Run $B
    if ($null -ne $Run) {
        if ([string]::IsNullOrWhiteSpace($Run.Dispatch)) {
            Warn (Label 'release' $B) "runs.json entry '$($Run.Key)' has no dispatch id: nothing released"
        } else {
            $Problem = Invoke-WorkerRelease $Run.Dispatch
            if ($null -eq $Problem) {
                Pass (Label 'release' $B) "worker-release --dispatch $($Run.Dispatch)"
            } else {
                Warn (Label 'release' $B) "worker-release --dispatch $($Run.Dispatch) failed ($Problem); release it by hand"
            }
        }
    }

    if ([string]::IsNullOrEmpty($Worktree)) {
        Pass (Label 'worktree' $B) 'none for this branch: only the branch is deleted'
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
        Pass (Label 'junctions' $B) ("{0} link(s) removed, none left" -f $Points.Count)

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
        if ($Restored -gt 0) { Pass (Label 'restore' $B) "$Restored tracked path(s) checked out again" }

        & git worktree remove $Worktree
        if ($LASTEXITCODE -ne 0) { Fail 'worktree remove' "git worktree remove $Worktree exited with $LASTEXITCODE" }
        if (Test-Path -LiteralPath $Worktree) { Fail 'worktree remove' "$Worktree still exists" }
        Pass (Label 'worktree remove' $B) $Worktree
    }

    # Legacy per-batch Cargo target (~5 GB). Slot targets (`dq9-target-slot<N>`, reused by
    # successive workers) and `dq9-target-integrate` are refused whatever the batch is named.
    $TargetName = "dq9-target-$($B.Batch)"
    $TargetDir = Join-Path (Get-LocalAppData) $TargetName
    if ($TargetName -like 'dq9-target-slot*' -or $TargetName -eq 'dq9-target-integrate') {
        Pass (Label 'target' $B) "$TargetDir kept: slot and integrate targets persist between batches"
    } elseif ($B.Batch -match '^[A-Za-z0-9._-]+$' -and
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
        Pass (Label 'target' $B) ("{0} removed ({1:N1} GB freed)" -f $TargetDir, ($Bytes / 1GB))
    }

    Invoke-Git 'branch delete' @('branch', '-D', $B.Name) | Out-Null
    Pass (Label 'branch delete' $B) $B.Name

    if ($null -ne $Run) {
        Remove-Run $Run.Key
        Pass (Label 'runs.json' $B) "entry '$($Run.Key)' removed"
    }
}

# --- temporary integration worktree ------------------------------------------------------------

function Remove-TempWorktree {
    $Path = $script:TempWorktree
    if (-not $Path) { return }
    $script:TempWorktree = $null
    $Saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        if (Test-Path -LiteralPath $Path) {
            if ((Get-ReparsePoints $Path).Count -gt 0) {
                Write-Host "warning: integration worktree $Path holds a reparse point, kept"
                return
            }
            & git -C $Root worktree remove --force $Path 2>&1 | Out-Null
        }
        & git -C $Root worktree prune 2>&1 | Out-Null
        if (Test-Path -LiteralPath $Path) { Write-Host "warning: integration worktree $Path still exists; delete it by hand" }
    } catch {
        Write-Host "warning: cannot remove integration worktree ${Path}: $($_.Exception.Message)"
    } finally {
        $ErrorActionPreference = $Saved
    }
}

# Runs the verify script of the tree in $Dir, exactly as the gate requires; returns its exit code.
function Invoke-VerifyAt([string] $Dir) {
    $VerifyScript = Join-Path $Dir 'scripts/verify.ps1'
    if (-not (Test-Path -LiteralPath $VerifyScript)) { Fail 'verify' "$VerifyScript not found" }
    Write-Host "==> verify: powershell -File $VerifyScript -RequireRom (in $Dir)"
    Push-Location -LiteralPath $Dir
    try {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $VerifyScript -RequireRom | Out-Host
        $Code = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    $Code
}

Push-Location $Root
try {
    $SavedEncoding = [Console]::OutputEncoding
    try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }
    $CleanupOnly = $Cleanup -and -not $Apply

    # Arguments.
    if ($Branches.Count -eq 0) { Fail 'arguments' '-Branch needs at least one branch' }
    if (@($Branches | Select-Object -Unique).Count -ne $Branches.Count) { Fail 'arguments' "duplicate branch in -Branch ($($Branches -join ', '))" }
    if ($Branches -contains 'main') { Fail 'arguments' 'cannot integrate main into itself' }
    $Specs = @(Split-List $Spec)
    if ($Specs.Count -gt 0 -and $Specs.Count -ne $Branches.Count) {
        Fail 'arguments' "-Spec has $($Specs.Count) item(s) for $($Branches.Count) branch(es): one per branch, same order"
    }
    $Messages = @($Message | Where-Object { $null -ne $_ })
    if ($Messages.Count -gt $Branches.Count) { Fail 'arguments' "-Message has $($Messages.Count) item(s) for $($Branches.Count) branch(es)" }

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

    # Branches.
    $Infos = [System.Collections.Generic.List[object]]::new()
    for ($Index = 0; $Index -lt $Branches.Count; $Index++) {
        $Name = $Branches[$Index]
        $B = [pscustomobject]@{
            Name = $Name
            Batch = $Name.Substring($Name.LastIndexOf('/') + 1)
            Head = $null; Worktree = $null; SpecPath = $null; Files = @(); Message = $null
        }
        & git rev-parse --verify --quiet "refs/heads/$Name" | Out-Null
        if ($LASTEXITCODE -ne 0) { Fail 'branch' "no local branch '$Name'" }
        $B.Head = (Invoke-Git 'branch' @('rev-parse', "refs/heads/$Name"))[0]
        Pass (Label 'branch' $B) "$Name at $($B.Head.Substring(0, 7))"
        $B.Worktree = Find-Worktree $Name
        $Infos.Add($B)
    }

    if ($CleanupOnly) {
        foreach ($B in $Infos) {
            & git merge-base --is-ancestor "refs/heads/$($B.Name)" main
            if ($LASTEXITCODE -eq 0) {
                Pass (Label 'integrated' $B) 'branch is an ancestor of main'
            } else {
                $Changed = Invoke-Git 'integrated' @('diff', '--name-only', '--no-renames', "main...refs/heads/$($B.Name)")
                if ($Changed.Count -eq 0) { Fail (Label 'integrated' $B) 'branch has no change of its own but is not in main' }
                & git --literal-pathspecs diff --quiet main "refs/heads/$($B.Name)" -- @Changed
                if ($LASTEXITCODE -ne 0) {
                    Fail (Label 'integrated' $B) 'main differs from the branch on its changed files: integrate it first (-Apply)'
                }
                Pass (Label 'integrated' $B) ("main holds the branch content of its {0} changed file(s)" -f $Changed.Count)
            }
        }
        foreach ($B in $Infos) { Invoke-Cleanup $B }
        Write-Summary
        exit 0
    }

    # Cheap per-branch checks: spec, worktree, rebase, changes, forbidden files, ownership,
    # clean worktree. Nothing expensive runs before all of them pass.
    for ($Index = 0; $Index -lt $Infos.Count; $Index++) {
        $B = $Infos[$Index]
        $SpecArg = if ($Specs.Count -gt 0) { $Specs[$Index] } else { "build/batches/$($B.Batch).md" }
        $SpecPath = (Resolve-Path -LiteralPath $SpecArg -ErrorAction SilentlyContinue)
        if ($null -eq $SpecPath) { Fail (Label 'spec' $B) "spec file not found: $SpecArg" }
        $B.SpecPath = $SpecPath.ProviderPath
        try { $Globs = Read-OwnsGlobs $B.SpecPath } catch { Fail (Label 'spec' $B) "$SpecArg`: $($_.Exception.Message)" }
        if ($Globs.Count -eq 0) { Fail (Label 'spec' $B) "no glob in a ``````owns block of $SpecArg" }
        $Regexes = @($Globs | ForEach-Object { Convert-GlobToRegex $_ })
        Pass (Label 'spec' $B) ("{0} owned glob(s) in {1}" -f $Globs.Count, $SpecArg)

        $B.Message = if ($Index -lt $Messages.Count -and $Messages[$Index]) { $Messages[$Index] } else { Read-SpecCommit $B.SpecPath }
        if ($Apply) {
            if (-not $B.Message) { Fail (Label 'arguments' $B) '-Apply needs a -Message entry (or a `Commit:` line in the spec)' }
            if ($B.Message -notmatch '^[a-z][a-z0-9._/-]*: \S[^\r\n]*$') {
                Fail (Label 'arguments' $B) "commit message must be one line 'domain: summary' (got '$($B.Message)')"
            }
        } elseif (-not $B.Message) {
            $B.Message = "chore: integrate $($B.Batch)"
        }

        if ($null -eq $B.Worktree) { Fail (Label 'worktree' $B) "no worktree has '$($B.Name)' checked out (git worktree list)" }
        if (-not (Test-Path -LiteralPath $B.Worktree)) { Fail (Label 'worktree' $B) "$($B.Worktree) is missing (git worktree prune?)" }
        Pass (Label 'worktree' $B) $B.Worktree

        & git merge-base --is-ancestor main "refs/heads/$($B.Name)"
        if ($LASTEXITCODE -ne 0) {
            if (-not $Rebase) { Fail (Label 'contains main' $B) "rebase needed: $($B.Name) does not contain main ($($MainHead.Substring(0, 7)))" }
            Write-Host "==> rebase: git rebase main (in $($B.Worktree))"
            & git -C $B.Worktree rebase main | Out-Host
            if ($LASTEXITCODE -ne 0) {
                & git -C $B.Worktree rebase --abort | Out-Null
                Fail (Label 'contains main' $B) 'rebase conflict, ask the worker'
            }
            $B.Head = (Invoke-Git 'rebase' @('rev-parse', "refs/heads/$($B.Name)"))[0]
            Pass (Label 'rebase' $B) "rebased on main, branch at $($B.Head.Substring(0, 7))"
        }
        Pass (Label 'contains main' $B) 'yes'

        $B.Files = Invoke-Git 'changes' @('diff', '--name-only', '--no-renames', "main...refs/heads/$($B.Name)")
        if ($B.Files.Count -eq 0) { Fail (Label 'changes' $B) 'no change to integrate' }
        Pass (Label 'changes' $B) ("{0} file(s)" -f $B.Files.Count)

        $Forbidden = @($B.Files | Where-Object { $_ -match '^(rom|build|tools)/' -or $_ -match '\.(nds|sav)$' })
        if ($Forbidden.Count -gt 0) { Fail (Label 'forbidden files' $B) ($Forbidden -join ', ') }
        Pass (Label 'forbidden files' $B) 'none'

        $Unowned = [System.Collections.Generic.List[string]]::new()
        $ReadmeCoverage = $false
        foreach ($File in $B.Files) {
            $Owned = $false
            foreach ($Regex in $Regexes) {
                if ([regex]::IsMatch($File, $Regex)) { $Owned = $true; break }
            }
            if ($Owned) { continue }
            if ($File -eq 'README.md' -and (Test-CoverageOnlyReadme $MainHead "refs/heads/$($B.Name)")) {
                $ReadmeCoverage = $true
                continue
            }
            $Unowned.Add($File)
        }
        if ($Unowned.Count -gt 0) { Fail (Label 'ownership' $B) ("not covered by ``````owns: {0}" -f ($Unowned -join ', ')) }
        $Detail = 'every file matches the owns globs'
        if ($ReadmeCoverage) { $Detail += ' (README.md: coverage block only)' }
        Pass (Label 'ownership' $B) $Detail

        if (-not (Test-Clean 'worktree clean' $B.Worktree)) { Fail (Label 'worktree clean' $B) "$($B.Worktree) has uncommitted changes" }
        $WorktreeHead = (Invoke-Git 'worktree clean' @('-C', $B.Worktree, 'rev-parse', 'HEAD'))[0]
        if ($WorktreeHead -ne $B.Head) { Fail (Label 'worktree clean' $B) "worktree HEAD $WorktreeHead is not the branch head" }
        Pass (Label 'worktree clean' $B) 'before verify'
    }

    # Verify environment: a private target (a shared one lets Cargo reuse an rlib built from
    # another worktree's sources) and the main checkout's ROM when DQ9_ROM is empty.
    $env:CARGO_TARGET_DIR = Join-Path (Get-LocalAppData) 'dq9-target-integrate'
    if ([string]::IsNullOrEmpty($env:DQ9_ROM)) {
        $RomDir = Join-Path $Root 'rom'
        $Roms = @(Get-ChildItem -LiteralPath $RomDir -File -Filter '*.nds' -ErrorAction SilentlyContinue)
        if ($Roms.Count -ne 1) {
            Fail 'rom' "expected exactly one *.nds in $RomDir, found $($Roms.Count): set DQ9_ROM"
        }
        $env:DQ9_ROM = $Roms[0].FullName
    }
    Pass 'verify env' "CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR, DQ9_ROM=$env:DQ9_ROM"

    # Combined result: one squash commit per branch, in a temporary worktree detached at main.
    $Tmp = Join-Path (Get-LocalAppData) ("dq9-integrate-wt-{0}-{1}" -f (Get-Date -Format 'yyyyMMddHHmmss'), $PID)
    & git worktree add --quiet --detach $Tmp $MainHead | Out-Host
    if ($LASTEXITCODE -ne 0) { Fail 'integration worktree' "git worktree add $Tmp failed" }
    $script:TempWorktree = $Tmp
    $TmpTop = (Invoke-Git 'integration worktree' @('-C', $Tmp, 'rev-parse', '--show-toplevel'))[0]
    if ([System.IO.Path]::GetFullPath($TmpTop) -ne [System.IO.Path]::GetFullPath($Tmp)) {
        Fail 'integration worktree' "$Tmp is not a worktree root (git says $TmpTop)"
    }
    Pass 'integration worktree' "$Tmp at main $($MainHead.Substring(0, 7))"

    $Prefix = [System.Collections.Generic.List[string]]::new()   # commit after k branches, k = 1..N
    foreach ($B in $Infos) {
        & git -C $Tmp merge --squash $B.Head | Out-Host
        if ($LASTEXITCODE -ne 0) {
            & git -C $Tmp reset --hard --quiet HEAD | Out-Null
            Fail (Label 'merge' $B) "git merge --squash $($B.Head.Substring(0, 7)) conflicts with main or an earlier branch of the run"
        }
        & git -C $Tmp diff --cached --quiet
        if ($LASTEXITCODE -eq 0) { Fail (Label 'merge' $B) 'adds nothing on top of main and the earlier branches' }
        & git -C $Tmp commit --quiet -m $B.Message | Out-Host
        if ($LASTEXITCODE -ne 0) { Fail (Label 'merge' $B) 'git commit failed in the integration worktree' }
        $Sha = (Invoke-Git 'merge' @('-C', $Tmp, 'rev-parse', 'HEAD'))[0]
        $Prefix.Add($Sha)
        if ($Infos.Count -eq 1) {
            & git diff --quiet $Sha $B.Head
            if ($LASTEXITCODE -ne 0) { Fail 'merge' 'integration tree differs from the branch tree after the squash' }
        }
        Pass (Label 'merge' $B) ("squashed as {0}: {1}" -f $Sha.Substring(0, 7), $B.Message)
    }

    # One verify of the combined tree.
    $VerifyCode = Invoke-VerifyAt $Tmp
    if ($VerifyCode -ne 0) {
        if ($Infos.Count -eq 1) {
            Fail 'verify' "verify.ps1 -RequireRom exited with $VerifyCode"
        }
        # Binary search for the first failing prefix: main + the first k branches. $Lo passed
        # (0 = main, taken as green), $Hi failed; k is the branch count of the prefix.
        Write-Host "==> verify failed on the combined tree ($($Infos.Count) branches): looking for the culprit"
        $Lo = 0
        $Hi = $Infos.Count
        while ($Hi - $Lo -gt 1) {
            $Mid = [int][math]::Floor(($Lo + $Hi) / 2)
            & git -C $Tmp reset --hard --quiet | Out-Null
            & git -C $Tmp clean -fdq | Out-Null
            & git -C $Tmp checkout --quiet --force --detach $Prefix[$Mid - 1] | Out-Null
            if ($LASTEXITCODE -ne 0) { Fail 'verify' "cannot check out prefix $Mid in the integration worktree" }
            Write-Host "==> prefix of $Mid branch(es), up to $($Infos[$Mid - 1].Name)"
            $PrefixCode = Invoke-VerifyAt $Tmp
            if ($PrefixCode -eq 0) {
                Pass 'verify prefix' "first $Mid branch(es) green"
                $Lo = $Mid
            } else {
                Pass 'verify prefix' "first $Mid branch(es) red (exit $PrefixCode)"
                $Hi = $Mid
            }
        }
        $Culprit = $Infos[$Hi - 1]
        Fail 'verify' ("verify.ps1 -RequireRom exited with {0} on the combined tree; culprit: {1} (prefix {2} of {3} is the first red; the {4} branch(es) before it are green together); main untouched" -f `
            $VerifyCode, $Culprit.Name, $Hi, $Infos.Count, $Lo)
    }
    Pass 'verify' ("verify.ps1 -RequireRom passed once on the combined tree ({0} branch(es))" -f $Infos.Count)

    # The only change verify may leave is the README coverage block: fold it into the last
    # integration commit. Anything else is a failure.
    $Status = Invoke-Git 'verify dirt' @('-C', $Tmp, 'status', '--porcelain')
    if ($Status.Count -gt 0) {
        if ($Status.Count -ne 1 -or $Status[0] -ne ' M README.md') {
            $Status | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" }
            Fail 'verify dirt' 'verify changed files other than the README.md progress block'
        }
        $Before = (Invoke-Git 'verify dirt' @('-C', $Tmp, 'rev-parse', 'HEAD'))[0]
        & git -C $Tmp add README.md
        & git -C $Tmp commit --quiet --amend --no-edit | Out-Host
        if ($LASTEXITCODE -ne 0) { Fail 'verify dirt' 'cannot fold the README.md block into the last integration commit' }
        $After = (Invoke-Git 'verify dirt' @('-C', $Tmp, 'rev-parse', 'HEAD'))[0]
        if (-not (Test-CoverageOnlyReadme $Before $After)) {
            Fail 'verify dirt' 'verify changed README.md outside the coverage block'
        }
        Pass 'verify dirt' 'README.md coverage block regenerated and folded into the last integration commit'
    }
    $Final = (Invoke-Git 'verify dirt' @('-C', $Tmp, 'rev-parse', 'HEAD'))[0]
    Pass 'integration' ("{0} commit(s), tip {1}" -f $Infos.Count, $Final.Substring(0, 7))

    if ($Apply) {
        $Head = (Invoke-Git 'apply' @('rev-parse', 'HEAD'))[0]
        if ($Head -ne $MainHead) { Fail 'apply' 'main moved during verification; run again' }
        if (-not (Test-Clean 'apply' $Root)) { Fail 'apply' 'main checkout changed during verification' }
        foreach ($B in $Infos) {
            $Now = (Invoke-Git 'apply' @('rev-parse', "refs/heads/$($B.Name)"))[0]
            if ($Now -ne $B.Head) { Fail (Label 'apply' $B) 'branch moved during verification; run again' }
        }
        & git merge --ff-only $Final | Out-Host
        if ($LASTEXITCODE -ne 0) { Fail 'apply' "git merge --ff-only $Final failed; main unchanged" }
        $Log = Invoke-Git 'apply' @('log', '--reverse', '--format=%h %s', "$MainHead..HEAD")
        Pass 'apply' ("main fast-forwarded, not pushed: {0}" -f ($Log -join '; '))
        # A pruned branch leaves no ancestry: record every integration for `queue.ps1`.
        $Applied = Invoke-Git 'apply' @('log', '--reverse', '--format=%H', "$MainHead..HEAD")
        if ($Applied.Count -ne $Infos.Count) {
            Fail 'trace' "expected $($Infos.Count) integration commit(s), found $($Applied.Count) in $MainHead..HEAD"
        }
        for ($Index = 0; $Index -lt $Infos.Count; $Index++) {
            Add-IntegrationTrace $Infos[$Index].Batch $Applied[$Index]
        }
        Pass 'trace' ("{0} line(s) appended to build/batches/integrated.txt" -f $Infos.Count)
    } else {
        Pass 'apply' 'dry run: main unchanged (pass -Apply -Message to commit)'
    }

    Remove-TempWorktree
    if ($Cleanup) { foreach ($B in $Infos) { Invoke-Cleanup $B } }
    Write-Summary
    exit 0
} finally {
    Remove-TempWorktree
    try { [Console]::OutputEncoding = $SavedEncoding } catch { }
    Pop-Location
}
