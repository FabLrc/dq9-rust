# Launches one batch worker from the main checkout: parses the spec, refuses an owns overlap
# with a running batch, picks a free Cargo target slot, writes the prompt and runs
# `orca orchestration worker-start`.
#
# Usage (from the main checkout):
#   powershell -ExecutionPolicy Bypass -File scripts/dispatch.ps1 -Batch k1-linker
#   powershell -ExecutionPolicy Bypass -File scripts/dispatch.ps1 -Batch k1-linker -DryRun
#   powershell -ExecutionPolicy Bypass -File scripts/dispatch.ps1 -Batch k1-linker `
#       -Model anthropic/claude-sonnet-5-5 -Attempt 2 -RetryOf ctx_0123456789ab
#
# What it does:
# - reads `build/batches/<batch>.md`: `Model: <id>` (or the legacy
#   `Owner: ... `<id>` worker` line), the ```owns globs and an optional `Depends: a,b`;
# - refuses when an owns glob overlaps one of a batch registered in
#   `build/batches/runs.json` (conservative: equality, prefix or `**` expansion, see
#   Test-GlobOverlap) or when a dependency is not integrated (its `Commit:` subject is in
#   `git log main`, or its branch `FabLrc/<dep>` is an ancestor of main);
# - picks the lowest free slot 1..5 of runs.json: `%LOCALAPPDATA%\dq9-target-slot<N>`
#   (`DQ9_TARGETS` overrides that root; the tests point it at a scratch directory). Cargo can
#   take an rlib built from another worktree's sources as fresh, so when the slot's
#   `.last-batch` marker names another batch (or is missing while the slot is not empty) the
#   workspace members are cleaned with `cargo clean -p <member>`: the third-party cache, the
#   expensive part, is kept;
# - writes `build/batches/<batch>.prompt.txt` from the ```text block of
#   `docs/orchestration/prompt-dispatch.md` and passes it with `--spec-file`;
# - records the receipt in `build/batches/runs.json` (contract with `integrate.ps1 -Cleanup`:
#   dispatch, slot, model, attempt, started, branch) and prints the worker state.
#
# -DryRun prints everything (the prompt included) and writes or launches nothing. `DQ9_ORCA`
# overrides the `orca` executable (the tests pass a stub).
param(
    [Parameter(Mandatory = $true)] [string] $Batch,
    [string] $Model,
    [int] $Attempt = 1,
    [string] $RetryOf,
    [switch] $DryRun
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$BatchesDir = Join-Path (Join-Path $Root 'build') 'batches'

# The three models of AGENTS.md; anything else is a typo or an escalation that was not decided.
$KnownModels = @(
    'opencode-go/deepseek-v4.1-flash',
    'anthropic/claude-sonnet-5-5',
    'anthropic/claude-opus-5-5'
)
# Identity of this repository in Orca (`orca worktree list`); fixed by AGENTS.md.
$RepoSelector = 'id:8ef45a12-a7ec-4e6c-b9dc-c85007817f63'

function Exit-Error([string] $Message) {
    Write-Host "dispatch.ps1: $Message"
    exit 1
}

# Converts an ```owns glob to an anchored, case-sensitive regex (same rules as integrate.ps1:
# `**` any path, `*` anything but `/`, `?` one character, a trailing `/` means `**`).
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

# True when two globs can cover the same file. Conservative on purpose: a false overlap only
# serialises two batches, a missed one lets two workers edit the same file.
function Test-GlobOverlap([string] $A, [string] $B) {
    $SA = ConvertTo-GlobSegments $A
    $SB = ConvertTo-GlobSegments $B
    if (($SA -join '/') -eq ($SB -join '/')) { return $true }
    # Literal prefix: the segments before the first one holding a wildcard. One prefix covering
    # the whole other path (`crates/a/**` vs `crates/a/mod.rs`, `crates/*` vs `crates/a/b.rs`)
    # is an overlap; `**` matches zero or more segments, so it is covered by the same test.
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
    # Same depth, no prefix relation: two segments can still name one file when one of them has
    # a wildcard (`crates/app/*.rs` vs `crates/app/main.rs`).
    if ($SA.Count -eq $SB.Count) {
        for ($I = 0; $I -lt $SA.Count; $I++) {
            if ($SA[$I] -eq $SB[$I]) { continue }
            $RA = Convert-GlobToRegex $SA[$I]
            $RB = Convert-GlobToRegex $SB[$I]
            $Probe = ($SA[$I], $SB[$I] | Where-Object { $_ -notmatch '[*?]' } | Select-Object -First 1)
            if ($null -eq $Probe) { return $true }   # both are wildcards: assume they meet
            if ($Probe -eq $SA[$I]) {
                if (-not [regex]::IsMatch($SA[$I], $RB)) { return $false }
            } elseif (-not [regex]::IsMatch($SB[$I], $RA)) { return $false }
        }
        return $true
    }
    $false
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

# Model id of a spec: `Model: `<id>`` first, else the legacy `Owner: ... `<id>` worker` line
# (the first backticked token that is one of the three known models: the line also backticks
# the branch).
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

# One spec: model, owns globs, dependencies, and the integration commit subject.
function Read-Spec([string] $Path) {
    $Lines = [System.IO.File]::ReadAllLines($Path)
    # No @() around Read-OwnsGlobs: it already returns one array (a wrapper would nest it).
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

# `build/batches/runs.json`: batch -> { dispatch, slot, model, attempt, started, branch }.
function Read-Runs {
    $Runs = [ordered]@{}
    $Path = Join-Path $BatchesDir 'runs.json'
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $Runs }
    $Text = [System.IO.File]::ReadAllText($Path)
    if ([string]::IsNullOrWhiteSpace($Text)) { return $Runs }
    $Json = $Text | ConvertFrom-Json
    foreach ($Property in $Json.PSObject.Properties) {
        $Entry = [ordered]@{}
        foreach ($Field in $Property.Value.PSObject.Properties) { $Entry[$Field.Name] = $Field.Value }
        $Runs[$Property.Name] = $Entry
    }
    $Runs
}

function Write-Runs($Runs) {
    $Out = [ordered]@{}
    foreach ($Name in $Runs.Keys) { $Out[$Name] = [pscustomobject] $Runs[$Name] }
    $Path = Join-Path $BatchesDir 'runs.json'
    $Json = ConvertTo-Json -InputObject $Out -Depth 5
    [System.IO.File]::WriteAllText($Path, $Json + "`n", (New-Object System.Text.UTF8Encoding($false)))
}

function Invoke-Git([string[]] $GitArgs) {
    $Output = @(& git -C $Root @GitArgs)
    if ($LASTEXITCODE -ne 0) { Exit-Error "git $($GitArgs -join ' ') exited with $LASTEXITCODE" }
    $Output
}

# Commit subjects of main, read once per run by the dependency checks.
function Get-MainSubjects {
    @(Invoke-Git @('log', 'main', '--format=%s'))
}

# A dependency is integrated when `build/batches/integrated.txt` (written by `integrate.ps1
# -Apply`) lists it, when its one-line commit subject is in `git log main`, or when its branch
# is an ancestor of main (a branch still existing is not enough: it must be merged).
function Test-Integrated([string] $Name, [string] $Commit, [string[]] $Subjects) {
    $Trace = Join-Path $BatchesDir 'integrated.txt'
    if ((Test-Path -LiteralPath $Trace) -and
        (@(Get-Content -LiteralPath $Trace | Where-Object { ($_ -split '\s+')[0] -eq $Name }).Count -gt 0)) {
        return $true
    }
    if ($Commit -and $Subjects -contains $Commit) { return $true }
    $Branch = "FabLrc/$Name"
    & git -C $Root rev-parse --verify --quiet "refs/heads/$Branch" | Out-Null
    if ($LASTEXITCODE -ne 0) { return $false }
    & git -C $Root merge-base --is-ancestor "refs/heads/$Branch" main
    $LASTEXITCODE -eq 0
}

# Package names of the workspace members: `cargo clean -p` takes names, and Cargo.toml lists
# directories (possibly with a wildcard). Read here rather than with `cargo metadata` so a
# dispatch does not pay a metadata resolve.
function Read-MemberNames {
    $Manifest = Join-Path $Root 'Cargo.toml'
    if (-not (Test-Path -LiteralPath $Manifest)) { return @() }
    $Text = [System.IO.File]::ReadAllText($Manifest)
    $Block = [regex]::Match($Text, '(?s)members\s*=\s*\[(.*?)\]')
    if (-not $Block.Success) { return @() }
    $Names = [System.Collections.Generic.List[string]]::new()
    foreach ($Match in [regex]::Matches($Block.Groups[1].Value, '"([^"]+)"')) {
        $Pattern = Join-Path $Root $Match.Groups[1].Value
        $Dirs = if ($Match.Groups[1].Value -match '[*?]') {
            @(Get-ChildItem -Path $Pattern -Directory -ErrorAction SilentlyContinue)
        } else {
            @(Get-Item -Path $Pattern -ErrorAction SilentlyContinue | Where-Object { $_.PSIsContainer })
        }
        foreach ($Dir in $Dirs) {
            $Package = Join-Path $Dir.FullName 'Cargo.toml'
            if (-not (Test-Path -LiteralPath $Package)) { continue }
            $Name = [regex]::Match([System.IO.File]::ReadAllText($Package),
                '(?sm)\[package\].*?^\s*name\s*=\s*"([^"]+)"').Groups[1].Value
            if ($Name) { $Names.Add($Name) } else { $Names.Add($Dir.Name) }
        }
    }
    @($Names | Sort-Object -Unique)
}

# The ```text block of prompt-dispatch.md, placeholders replaced: that file stays the single
# source of the prompt, and this script only fills it.
function New-PromptText([hashtable] $Fields) {
    $TemplatePath = Join-Path $Root 'docs/orchestration/prompt-dispatch.md'
    if (-not (Test-Path -LiteralPath $TemplatePath)) {
        Exit-Error "prompt template not found: $TemplatePath"
    }
    $Block = [System.Collections.Generic.List[string]]::new()
    $InBlock = $false
    $Found = $false
    foreach ($Line in [System.IO.File]::ReadAllLines($TemplatePath)) {
        if (-not $InBlock) {
            if ($Line -match '^\s*```text\s*$') { $InBlock = $true; $Found = $true }
            continue
        }
        if ($Line -match '^\s*```\s*$') { $InBlock = $false; continue }
        $Block.Add($Line)
    }
    if (-not $Found) { Exit-Error "no ```text block in $TemplatePath" }
    $Text = $Block -join "`n"
    # Longest placeholders first: `<absolute spec path>` and `<main checkout>` do not nest, but
    # `<slot target>` must not be eaten by `<target>`.
    foreach ($Key in @('absolute spec path', 'main checkout', 'slot target', 'base sha',
            'batch', 'model', 'target')) {
        if ($Fields.ContainsKey($Key)) {
            $Text = $Text.Replace("<$Key>", [string] $Fields[$Key])
        }
    }
    $Text.TrimEnd() + "`n"
}

if ($Batch -notmatch '^[A-Za-z0-9._-]+$' -or $Batch -eq '.' -or $Batch -eq '..') {
    Exit-Error "invalid batch name '$Batch' (letters, digits, '.', '_', '-')"
}
if ($Attempt -lt 1) { Exit-Error "-Attempt must be 1 or more (got $Attempt)" }
if (-not (Test-Path -LiteralPath $BatchesDir -PathType Container)) {
    Exit-Error "no ${BatchesDir}: dispatch from the main checkout (its build/ is not in a worktree)"
}
$SpecFile = Join-Path $BatchesDir "$Batch.md"
if (-not (Test-Path -LiteralPath $SpecFile -PathType Leaf)) {
    Exit-Error "spec not found: $SpecFile"
}

# The main checkout, not a linked worktree: runs.json and the prompt live in its ignored build/.
$Top = @(& git -C $Root rev-parse --show-toplevel)
if ($LASTEXITCODE -ne 0) { Exit-Error "$Root is not a git checkout" }
$MainWorktree = @(& git -C $Root worktree list --porcelain)
if ($LASTEXITCODE -ne 0) { Exit-Error 'git worktree list failed' }
$MainPath = $null
foreach ($Line in $MainWorktree) {
    if ($Line.StartsWith('worktree ')) { $MainPath = $Line.Substring(9); break }
}
$SamePath = $MainPath -and
    ([System.IO.Path]::GetFullPath($MainPath) -eq [System.IO.Path]::GetFullPath($Top[0]))
if (-not $SamePath) {
    Exit-Error "run from the main checkout ($MainPath), not from a linked worktree"
}

$Spec = Read-Spec $SpecFile
if ($Spec.Owns.Count -eq 0) { Exit-Error "no ```owns glob in $SpecFile" }
$SelectedModel = if ($Model) { $Model.Trim() } else { $Spec.Model }
if (-not $SelectedModel) {
    Exit-Error "no 'Model: <id>' line (nor a legacy Owner: line) in $SpecFile"
}
if ($KnownModels -notcontains $SelectedModel) {
    Exit-Error "unknown model '$SelectedModel' (AGENTS.md models: $($KnownModels -join ', '))"
}
$BaseSha = @(Invoke-Git @('rev-parse', '--short', 'main'))[0]
if (-not $BaseSha) { Exit-Error 'no local branch main (git rev-parse --short main)' }

# Dependencies, then the batches still running.
$Subjects = Get-MainSubjects
$Missing = @()
foreach ($Dep in $Spec.Depends) {
    $DepSpec = Join-Path $BatchesDir "$Dep.md"
    $DepCommit = if (Test-Path -LiteralPath $DepSpec) { (Read-Spec $DepSpec).Commit } else { $null }
    if (-not (Test-Integrated $Dep $DepCommit $Subjects)) { $Missing += $Dep }
}
if ($Missing.Count -gt 0) {
    Exit-Error ("depends on not integrated batch(es): {0} (see build/batches/runs.json and git log main)" -f
        ($Missing -join ', '))
}

$Runs = Read-Runs
foreach ($Name in @($Runs.Keys)) {
    if ($Name -eq $Batch) { continue }   # a retry replaces its own entry
    $OtherSpec = Join-Path $BatchesDir "$Name.md"
    if (-not (Test-Path -LiteralPath $OtherSpec)) {
        Exit-Error "batch '$Name' is registered in runs.json but its spec is missing: cannot check owns overlap"
    }
    $Other = Read-Spec $OtherSpec
    foreach ($Mine in $Spec.Owns) {
        foreach ($Theirs in $Other.Owns) {
            if (Test-GlobOverlap $Mine $Theirs) {
                Exit-Error ("owns overlap with running batch '{0}': '{1}' and '{2}'" -f $Name, $Mine, $Theirs)
            }
        }
    }
}

# Lowest free slot, and whether its target must be cleaned before another batch uses it.
$UsedSlots = @()
foreach ($Name in $Runs.Keys) {
    $Slot = $Runs[$Name].slot
    if ($null -ne $Slot -and "$Slot" -ne '') { $UsedSlots += [int] $Slot }
}
$Slot = $null
foreach ($Candidate in 1..5) {
    if ($UsedSlots -notcontains $Candidate) { $Slot = $Candidate; break }
}
if (-not $Slot) {
    Exit-Error 'all five target slots are used (build/batches/runs.json); wait for an integration'
}
$TargetsRoot = if ($env:DQ9_TARGETS) { $env:DQ9_TARGETS } else { $env:LOCALAPPDATA }
if (-not $TargetsRoot) { Exit-Error 'neither DQ9_TARGETS nor LOCALAPPDATA is set' }
$TargetDir = Join-Path $TargetsRoot "dq9-target-slot$Slot"
$MarkerPath = Join-Path $TargetDir '.last-batch'
$LastBatch = $null
if (Test-Path -LiteralPath $MarkerPath -PathType Leaf) {
    $LastBatch = ([System.IO.File]::ReadAllText($MarkerPath)).Trim()
}
$NeedsClean = (Test-Path -LiteralPath $TargetDir -PathType Container) -and ($LastBatch -ne $Batch)
$CleanReason = if (-not $NeedsClean) {
    if ($LastBatch -eq $Batch) { "slot already built for ${Batch}: no clean" } else { 'slot is empty: no clean' }
} elseif ($LastBatch) {
    "slot last used by '$LastBatch': cargo clean -p each member"
} else {
    'slot has content but no .last-batch marker: cargo clean -p each member'
}

$Prompt = New-PromptText @{
    'batch'              = $Batch
    'model'              = $SelectedModel
    'base sha'           = $BaseSha
    'absolute spec path' = (Resolve-Path -LiteralPath $SpecFile).Path
    'main checkout'      = (Resolve-Path -LiteralPath $Root).Path
    'slot target'        = $TargetDir
}
$PromptFile = Join-Path $BatchesDir "$Batch.prompt.txt"
$OrcaExe = if ($env:DQ9_ORCA) { $env:DQ9_ORCA } else { 'orca' }
$OrcaArgs = @(
    'orchestration', 'worker-start', '--spec-file', $PromptFile,
    '--worktree', 'new-top-level', '--repo', $RepoSelector, '--name', $Batch,
    '--model', $SelectedModel, '--base-branch', 'main', '--agent', 'omp',
    '--setup', 'skip', '--json'
)
if ($RetryOf) { $OrcaArgs += @('--retry-of', $RetryOf) }

Write-Output "batch: $Batch (attempt $Attempt)"
Write-Output "model: $SelectedModel"
Write-Output "base: main @ $BaseSha"
Write-Output "spec: $((Resolve-Path -LiteralPath $SpecFile).Path)"
Write-Output ("owns: {0}" -f ($Spec.Owns -join ', '))
Write-Output ("depends: {0}" -f $(if ($Spec.Depends.Count -gt 0) { $Spec.Depends -join ', ' } else { 'none' }))
Write-Output "slot: $Slot ($TargetDir)"
Write-Output "clean: $CleanReason"
Write-Output "prompt: $PromptFile"
Write-Output "launch: $OrcaExe $($OrcaArgs -join ' ')"

if ($DryRun) {
    Write-Output ''
    Write-Output '--- prompt ---'
    Write-Output $Prompt
    Write-Output '--- dry run: nothing written, nothing launched ---'
    exit 0
}

if ($NeedsClean) {
    Write-Output "==> clean: $CleanReason"
    $SavedTarget = $env:CARGO_TARGET_DIR
    $env:CARGO_TARGET_DIR = $TargetDir
    Push-Location $Root
    try {
        foreach ($Package in (Read-MemberNames)) {
            Write-Output "    cargo clean -p $Package"
            & cargo clean -p $Package
            if ($LASTEXITCODE -ne 0) { Exit-Error "cargo clean -p $Package exited with $LASTEXITCODE" }
        }
    } finally {
        Pop-Location
        $env:CARGO_TARGET_DIR = $SavedTarget
    }
}

if (-not (Test-Path -LiteralPath $TargetDir -PathType Container)) {
    New-Item -ItemType Directory -Path $TargetDir | Out-Null
}
[System.IO.File]::WriteAllText($MarkerPath, "$Batch`n", (New-Object System.Text.UTF8Encoding($false)))
[System.IO.File]::WriteAllText($PromptFile, $Prompt, (New-Object System.Text.UTF8Encoding($false)))
Write-Output "==> prompt written: $PromptFile"

# Runs `orca` and folds its stderr into the text: with $ErrorActionPreference = 'Stop',
# PowerShell 5.1 turns a native stderr line into a terminating error record, which would hide
# the receipt and the real exit code of a failed start.
function Invoke-Orca {
    $Previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $Lines = @(& $OrcaExe @OrcaArgs 2>&1 | ForEach-Object { "$_" })
        $Code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $Previous
    }
    [pscustomobject]@{ Code = $Code; Lines = $Lines }
}

$Receipt = Invoke-Orca
$ExitCode = $Receipt.Code
$Json = $null
foreach ($Line in $Receipt.Lines) {
    $Text = "$Line".Trim()
    if (-not $Text.StartsWith('{')) { continue }
    try { $Json = $Text | ConvertFrom-Json } catch { }
}
$Dispatch = $null
$State = $null
if ($Json) {
    $Dispatch = $Json.result.dispatchId
    if (-not $Dispatch) { $Dispatch = $Json.dispatchId }
    $State = $Json.result.state
}
if (-not $Dispatch) {
    $Receipt.Lines | ForEach-Object { Write-Host $_ }
    Exit-Error "no dispatch id in the worker-start receipt (exit $ExitCode)"
}
if ($ExitCode -ne 0 -and $State -ne 'outcome_unknown') {
    $Receipt.Lines | ForEach-Object { Write-Host $_ }
    Exit-Error "worker-start failed (state '$State', exit $ExitCode); $Dispatch not recorded in runs.json"
}

$Runs[$Batch] = [ordered]@{
    dispatch = $Dispatch
    slot     = $Slot
    model    = $SelectedModel
    attempt  = $Attempt
    started  = (Get-Date).ToString('o')
    branch   = "FabLrc/$Batch"
}
Write-Runs $Runs
Write-Output "==> dispatch: $Dispatch (state '$State') on slot $Slot, target $TargetDir"
Write-Output "==> runs.json: build/batches/runs.json"
if ($State -eq 'outcome_unknown') {
    Write-Output '==> the worker may still be starting: do not resend it'
}
