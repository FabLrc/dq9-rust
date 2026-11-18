# Shows the state of every batch spec of `build/batches/` and the next wave to dispatch.
#
# Usage (from the main checkout):
#   powershell -ExecutionPolicy Bypass -File scripts/queue.ps1
#
# State of a spec:
#   running    its batch has an entry in `build/batches/runs.json` (a worker holds a slot), or
#              a worktree in `git worktree list` holds a branch `FabLrc/<batch>` or
#              `FabLrc/<anything>-<batch>` (a worker launched by hand);
#   integrated `build/batches/integrated.txt` lists the batch (trace written by
#              `integrate.ps1 -Apply`), its `Commit:` subject is in `git log main`,
#              `docs/status.md` lists the batch, or main holds the content of the files its
#              branch changed (nothing left to launch: squash integration leaves no branch
#              ancestor to check);
#   no-owns    no ```owns block: `dispatch.ps1` refuses it, so it needs a spec fix first;
#   draft      no `Model:` line (nor a legacy `Owner:` model): the spec is not ready to launch;
#   waiting    a `Depends:` batch is not integrated yet (shown in `notes`);
#   ready      everything above is satisfied.
#
# The next wave is the ready specs that are pairwise disjoint on their ```owns globs and
# disjoint from the running ones, at most as many as the five target slots leave free, newest
# spec file first. Each line is the command to run; the slot itself is picked at launch,
# because a slot is free until a dispatch records it in runs.json.
#
# Read-only: this script writes nothing. The ```owns overlap test is conservative and mirrors
# `dispatch.ps1` (the two scripts stay standalone, no shared module).
param()
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$BatchesDir = Join-Path (Join-Path $Root 'build') 'batches'
$Slots = 5

$KnownModels = @(
    'opencode-go/deepseek-v4.1-flash',
    'anthropic/claude-sonnet-5-5',
    'anthropic/claude-opus-5-5'
)

function Exit-Error([string] $Message) {
    Write-Host "queue.ps1: $Message"
    exit 1
}

# Keep in sync with the same functions in dispatch.ps1 and integrate.ps1.
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

function ConvertTo-GlobSegments([string] $Glob) {
    $G = $Glob.Trim().Replace('\', '/')
    if ($G.StartsWith('./')) { $G = $G.Substring(2) }
    if ($G.EndsWith('/')) { $G += '**' }
    @($G -split '/')
}

function Test-GlobOverlap([string] $A, [string] $B) {
    $SA = ConvertTo-GlobSegments $A
    $SB = ConvertTo-GlobSegments $B
    if (($SA -join '/') -eq ($SB -join '/')) { return $true }
    $PrefixA = [System.Collections.Generic.List[string]]::new()
    foreach ($S in $SA) { if ($S -match '[*?]') { break }; $PrefixA.Add($S) }
    $PrefixB = [System.Collections.Generic.List[string]]::new()
    foreach ($S in $SB) { if ($S -match '[*?]') { break }; $PrefixB.Add($S) }
    if ($PrefixA.Count -le $SB.Count) {
        $Covers = $true
        for ($I = 0; $I -lt $PrefixA.Count; $I++) { if ($PrefixA[$I] -ne $SB[$I]) { $Covers = $false; break } }
        if ($Covers) { return $true }
    }
    if ($PrefixB.Count -le $SA.Count) {
        $Covers = $true
        for ($I = 0; $I -lt $PrefixB.Count; $I++) { if ($PrefixB[$I] -ne $SA[$I]) { $Covers = $false; break } }
        if ($Covers) { return $true }
    }
    if ($SA.Count -eq $SB.Count) {
        for ($I = 0; $I -lt $SA.Count; $I++) {
            if ($SA[$I] -eq $SB[$I]) { continue }
            $RA = Convert-GlobToRegex $SA[$I]
            $RB = Convert-GlobToRegex $SB[$I]
            $Probe = ($SA[$I], $SB[$I] | Where-Object { $_ -notmatch '[*?]' } | Select-Object -First 1)
            if ($null -eq $Probe) { return $true }
            if ($Probe -eq $SA[$I]) {
                if (-not [regex]::IsMatch($SA[$I], $RB)) { return $false }
            } elseif (-not [regex]::IsMatch($SB[$I], $RA)) { return $false }
        }
        return $true
    }
    $false
}

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

function Read-SpecModel([string[]] $Lines) {
    foreach ($Line in $Lines) {
        if ($Line -match '^\s*Model:') {
            $M = [regex]::Match($Line, '`([^`]+)`')
            if ($M.Success) { return $M.Groups[1].Value.Trim() }
            $M = [regex]::Match($Line, '^\s*Model:\s*(\S+)')
            if ($M.Success) { return $M.Groups[1].Value.Trim().TrimEnd('.', ',') }
            return $null
        }
    }
    foreach ($Line in $Lines) {
        if ($Line -notmatch '^\s*Owner:') { continue }
        foreach ($M in [regex]::Matches($Line, '`([^`]+)`')) {
            if ($KnownModels -contains $M.Groups[1].Value.Trim()) { return $M.Groups[1].Value.Trim() }
        }
        return $null
    }
    $null
}

function Read-Spec([string] $Path) {
    $Lines = [System.IO.File]::ReadAllLines($Path)
    $Spec = [pscustomobject]@{
        Model   = Read-SpecModel $Lines
        Owns    = Read-OwnsGlobs $Path
        Depends = @()
        Commit  = $null
    }
    foreach ($Line in $Lines) {
        if ($Line -match '^\s*Depends:\s*(.*)$') {
            $Spec.Depends = @($Matches[1] -split ',' | ForEach-Object { $_.Trim() } |
                Where-Object { $_ -ne '' -and $_ -ne 'none' })
        }
        if ($Line -match '^\s*Commit:\s*(.+)$') {
            $Spec.Commit = $Matches[1].Trim().Trim('`')
        }
    }
    $Spec
}

function Read-Runs {
    $Runs = [ordered]@{}
    $Path = Join-Path $BatchesDir 'runs.json'
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $Runs }
    $Text = [System.IO.File]::ReadAllText($Path)
    if ([string]::IsNullOrWhiteSpace($Text)) { return $Runs }
    $Json = $Text | ConvertFrom-Json
    foreach ($Property in $Json.PSObject.Properties) {
        $Entry = [ordered]@{}
        if ($null -ne $Property.Value) {
            foreach ($Field in $Property.Value.PSObject.Properties) { $Entry[$Field.Name] = $Field.Value }
        }
        $Runs[$Property.Name] = $Entry
    }
    $Runs
}

function Invoke-Git([string[]] $GitArgs) {
    $Output = @(& git -C $Root @GitArgs)
    if ($LASTEXITCODE -ne 0) { Exit-Error "git $($GitArgs -join ' ') exited with $LASTEXITCODE" }
    $Output
}

# Integration evidence, read once: the subjects of main, the local branches with their
# worktrees, the trace written by `integrate.ps1 -Apply`, and the coordinator's record
# (docs/status.md). Squash integration leaves no branch ancestor, so the branch check compares
# the tree of the files the branch changed, like `integrate.ps1`.
$Subjects = @(Invoke-Git @('log', 'main', '--format=%s'))
$SubjectSet = @{}
foreach ($Item in $Subjects) { $SubjectSet["$Item"] = $true }
$Branches = @(& git -C $Root for-each-ref '--format=%(refname:short)' refs/heads)
if ($LASTEXITCODE -ne 0) { Exit-Error 'git for-each-ref failed' }
$BranchSet = @{}
foreach ($Item in $Branches) { $BranchSet["$($Item.Trim())"] = $true }
# Branches with a worktree: a branch checked out in one means a worker holds the batch, even
# when `runs.json` has no entry (a launch by hand).
$WorktreeBranches = @{}
$WorktreeLines = @(& git -C $Root worktree list --porcelain)
if ($LASTEXITCODE -ne 0) { Exit-Error 'git worktree list failed' }
foreach ($Line in $WorktreeLines) {
    if ($Line -match '^branch refs/heads/(.+)$') { $WorktreeBranches[$Matches[1].Trim()] = $true }
}
# The integration trace, one `<batch> <commit sha> <ISO date>` line per applied batch: the
# branch of an integrated batch is pruned, so `git log main` cannot find it by ancestry.
$TraceSet = @{}
$TracePath = Join-Path $BatchesDir 'integrated.txt'
if (Test-Path -LiteralPath $TracePath -PathType Leaf) {
    foreach ($Line in [System.IO.File]::ReadAllLines($TracePath)) {
        $Fields = @($Line.Trim() -split '\s+' | Where-Object { $_ -ne '' })
        if ($Fields.Count -ge 1 -and $Fields[0] -match '^[A-Za-z0-9._-]+$') { $TraceSet[$Fields[0]] = $true }
    }
}
$StatusText = ''
$StatusPath = Join-Path $Root 'docs/status.md'
if (Test-Path -LiteralPath $StatusPath -PathType Leaf) {
    $StatusText = [System.IO.File]::ReadAllText($StatusPath)
}

# The branch of a batch checked out in a worktree, or $null: a worker launched by hand has no
# `runs.json` entry. `dispatch.ps1` names the branch `FabLrc/<batch>`; older waves used
# `FabLrc/<milestone>-<batch>`.
function Find-RunningBranch([string] $Name) {
    foreach ($Branch in $WorktreeBranches.Keys) {
        if (-not $Branch.StartsWith('FabLrc/')) { continue }
        if ($Branch -eq "FabLrc/$Name" -or $Branch.EndsWith("-$Name")) { return $Branch }
    }
    $null
}

function Test-Integrated([string] $Name, [string] $Commit) {
    # A pruned branch leaves no ancestry: the trace of `integrate.ps1 -Apply` is authoritative.
    if ($TraceSet.ContainsKey($Name)) { return $true }
    if ($Commit -and $SubjectSet.ContainsKey($Commit)) { return $true }
    # The status file lists the batches of every wave (AGENTS.md: it is the only place the
    # current state is recorded), so an integrated batch whose branch was pruned is found there.
    if ($StatusText -and $StatusText -match ('(?<![\w-])' + [regex]::Escape($Name) + '(?![\w-])')) {
        return $true
    }
    $Branch = "FabLrc/$Name"
    if (-not $BranchSet.ContainsKey($Branch)) { return $false }
    $Changed = @(& git -C $Root diff --name-only --no-renames "main...refs/heads/$Branch")
    if ($LASTEXITCODE -ne 0 -or $Changed.Count -eq 0) { return $false }
    # A branch created from main and not committed yet has no change of its own: not integrated.
    & git -C $Root --literal-pathspecs diff --quiet main "refs/heads/$Branch" -- @Changed
    $LASTEXITCODE -eq 0
}

if (-not (Test-Path -LiteralPath $BatchesDir -PathType Container)) {
    Exit-Error "no ${BatchesDir}: run from the main checkout (build/ is not in a worktree)"
}
& git -C $Root rev-parse --verify --quiet refs/heads/main | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-Error "no local branch main in $Root" }

$Runs = Read-Runs

$Specs = [System.Collections.Generic.List[object]]::new()
foreach ($File in @(Get-ChildItem -LiteralPath $BatchesDir -File -Filter '*.md' | Sort-Object Name)) {
    $Name = $File.BaseName
    $Spec = Read-Spec $File.FullName
    $State = $null
    $Notes = @()
    $RunningBranch = Find-RunningBranch $Name
    if ($Runs.Contains($Name)) {
        $State = 'running'
        $Slot = $Runs[$Name].slot
        if ($null -ne $Slot -and "$Slot" -ne '') { $Notes += "slot $Slot" }
        if ($Runs[$Name].attempt) { $Notes += "attempt $($Runs[$Name].attempt)" }
    } elseif ($RunningBranch) {
        $State = 'running'
        $Notes += "branch $RunningBranch, no runs.json entry"
    } elseif (Test-Integrated $Name $Spec.Commit) {
        $State = 'integrated'
    } elseif ($Spec.Owns.Count -eq 0) {
        $State = 'no-owns'
    } elseif (-not $Spec.Model) {
        $State = 'draft'
    } else {
        $Waiting = @($Spec.Depends | Where-Object {
                $DepSpec = Join-Path $BatchesDir "$_.md"
                $DepCommit = if (Test-Path -LiteralPath $DepSpec) { (Read-Spec $DepSpec).Commit } else { $null }
                -not (Test-Integrated $_ $DepCommit)
            })
        if ($Waiting.Count -gt 0) {
            $State = 'waiting'
            $Notes += "depends on $($Waiting -join ', ')"
        } else {
            $State = 'ready'
        }
    }
    $Specs.Add([pscustomobject]@{
            Name     = $Name
            State    = $State
            Model    = $Spec.Model
            Owns     = $Spec.Owns
            Depends  = $Spec.Depends
            Notes    = ($Notes -join ', ')
            Mtime    = $File.LastWriteTimeUtc
        })
}

# Running batches hold their owns and their slot; ready ones fill what is left, newest spec
# first (the coordinator writes the specs of the wave it is launching, older ready ones are
# usually leftovers of an earlier wave), then in name order.
$Running = @($Specs | Where-Object { $_.State -eq 'running' })
$RunningOwns = @()
foreach ($Spec in $Running) { $RunningOwns += $Spec.Owns }
$Free = $Slots - $Running.Count
if ($Free -lt 0) { $Free = 0 }
$Candidates = @($Specs | Where-Object { $_.State -eq 'ready' } |
    Sort-Object -Property @{ Expression = { $_.Mtime }; Descending = $true }, Name)
$Wave = [System.Collections.Generic.List[object]]::new()
$Taken = @()
foreach ($Spec in $Candidates) {
    if ($Wave.Count -ge $Free) { break }
    $Conflict = $null
    $Overlaps = @($RunningOwns) + @($Taken)
    foreach ($Mine in $Spec.Owns) {
        foreach ($Theirs in $Overlaps) {
            if (Test-GlobOverlap $Mine $Theirs) { $Conflict = $Theirs; break }
        }
        if ($Conflict) { break }
    }
    if ($Conflict) { $Spec.Notes = (@($Spec.Notes, "owns overlaps '$Conflict'") |
            Where-Object { $_ -ne '' }) -join '; '
        continue
    }
    $Wave.Add($Spec)
    $Taken += $Spec.Owns
}

$Counts = @{}
foreach ($Spec in $Specs) {
    if ($Counts.ContainsKey($Spec.State)) { $Counts[$Spec.State]++ } else { $Counts[$Spec.State] = 1 }
}

$NameWidth = [Math]::Max(6, ($Specs | ForEach-Object { $_.Name.Length } | Measure-Object -Maximum).Maximum + 2)
$NameWidth = [Math]::Min($NameWidth, 32)
$StateWidth = 11
$ModelWidth = 20

Write-Output ("{0} batch specs in {1}" -f $Specs.Count, $BatchesDir)
$Summary = @('running', 'ready', 'waiting', 'draft', 'no-owns', 'integrated' | ForEach-Object {
        if ($Counts.ContainsKey($_)) { "$_ $($Counts[$_])" }
    })
Write-Output ("states: {0}" -f ($Summary -join ', '))
Write-Output ("slots: {0} of {1} used{2}" -f $Running.Count, $Slots, $(if ($Running.Count -gt 0) { ", $Free free" } else { '' }))
Write-Output ''
Write-Output ("{0}{1}{2}{3}" -f 'batch'.PadRight($NameWidth), 'state'.PadRight($StateWidth),
    'model'.PadRight($ModelWidth), 'notes')
foreach ($Spec in $Specs) {
    # Short id: the provider is the same for the three models of AGENTS.md.
    $Model = if ($Spec.Model) { ($Spec.Model -split '/')[-1] } else { '-' }
    Write-Output ("{0}{1}{2}{3}" -f $Spec.Name.PadRight($NameWidth), $Spec.State.PadRight($StateWidth),
        $Model.PadRight($ModelWidth), $Spec.Notes)
}

Write-Output ''
if ($Wave.Count -eq 0) {
    if ($Free -eq 0) {
        Write-Output 'next wave: none (all five slots are used)'
    } else {
        Write-Output 'next wave: none (no ready spec is disjoint from the running owns)'
    }
} else {
    Write-Output ("next wave ({0} of {1} free slot(s)), slots are picked at launch:" -f $Wave.Count, $Free)
    foreach ($Spec in $Wave) {
        Write-Output ("  ./scripts/dispatch.ps1 -Batch {0}" -f $Spec.Name)
    }
}
