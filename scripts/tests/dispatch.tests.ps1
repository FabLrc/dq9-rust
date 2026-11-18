# Tests of scripts/dispatch.ps1 and scripts/queue.ps1: plain PowerShell assertions, a throwaway
# git repository per case under build/scratch/, a stub `orca` (DQ9_ORCA) and a stub `cargo` on
# PATH. No real worker is launched, no real target is touched (DQ9_TARGETS points at the
# scratch directory).
#
# Usage (from the worktree or the main checkout):
#   powershell -ExecutionPolicy Bypass -File scripts/tests/dispatch.tests.ps1
#
# Exit code 0 when every assertion passes; the fixtures are left in build/scratch/ on failure.
param()
$ErrorActionPreference = 'Stop'
$ScriptRoot = Split-Path -Parent $PSScriptRoot
$Root = Split-Path -Parent $ScriptRoot
$Scratch = Join-Path (Join-Path (Join-Path $Root 'build') 'scratch') 'dispatch-tests'

$script:Passed = 0
$script:Failed = 0

function Assert([string] $Name, [bool] $Condition, [string] $Detail = '') {
    if ($Condition) {
        Write-Output "ok   $Name"
        $script:Passed++
    } else {
        Write-Output "FAIL $Name"
        if ($Detail) { Write-Output "     $Detail" }
        $script:Failed++
    }
}

# Runs a native command with stderr folded into the text: with
# $ErrorActionPreference = 'Stop' PowerShell 5.1 turns a native stderr line (a git warning) into
# a terminating error record.
function Invoke-Native([scriptblock] $Call) {
    $Previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $Output = @(& $Call 2>&1 | ForEach-Object { "$_" })
        $Code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $Previous
    }
    [pscustomobject]@{ Code = $Code; Text = ($Output -join "`n") }
}

function Invoke-FixtureGit([string] $Dir, [string[]] $GitArgs) {
    $Result = Invoke-Native { git -C $Dir @GitArgs }
    if ($Result.Code -ne 0) {
        throw ("git {0} failed in {1}: {2}" -f ($GitArgs -join ' '), $Dir, $Result.Text)
    }
    $Result.Text
}

function Write-FixtureFile([string] $Path, [string] $Text) {
    $Parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $Parent -PathType Container)) {
        New-Item -ItemType Directory -Path $Parent -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

# A minimal repository shaped like this one: the two scripts, the prompt template, a workspace
# manifest with one member, a `main` branch and an ignored build/.
function New-Fixture([string] $Name) {
    $Dir = Join-Path $Scratch $Name
    if (Test-Path -LiteralPath $Dir) { Remove-Item -LiteralPath $Dir -Recurse -Force }
    New-Item -ItemType Directory -Path (Join-Path $Dir 'scripts') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $Dir 'docs/orchestration') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $Dir 'build/batches') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $Dir 'crates/a/src') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $ScriptRoot 'dispatch.ps1') -Destination (Join-Path $Dir 'scripts/dispatch.ps1')
    Copy-Item -LiteralPath (Join-Path $ScriptRoot 'queue.ps1') -Destination (Join-Path $Dir 'scripts/queue.ps1')
    Copy-Item -LiteralPath (Join-Path $Root 'docs/orchestration/prompt-dispatch.md') `
        -Destination (Join-Path $Dir 'docs/orchestration/prompt-dispatch.md')
    Write-FixtureFile (Join-Path $Dir 'Cargo.toml') @'
[workspace]
resolver = "3"
members = ["crates/*"]
'@
    Write-FixtureFile (Join-Path $Dir 'crates/a/Cargo.toml') @'
[package]
name = "crate-a"
version = "0.1.0"
edition = "2024"
'@
    Write-FixtureFile (Join-Path $Dir 'crates/a/src/lib.rs') "pub fn a() {}`n"
    Write-FixtureFile (Join-Path $Dir '.gitignore') "/build/`n"
    Invoke-FixtureGit $Dir @('init', '-q', '-b', 'main') | Out-Null
    Invoke-FixtureGit $Dir @('config', 'user.email', 'test@example.com') | Out-Null
    Invoke-FixtureGit $Dir @('config', 'user.name', 'dispatch tests') | Out-Null
    Invoke-FixtureGit $Dir @('config', 'commit.gpgsign', 'false') | Out-Null
    Invoke-FixtureGit $Dir @('add', '--all') | Out-Null
    Invoke-FixtureGit $Dir @('commit', '-q', '-m', 'test: fixture') | Out-Null
    $Dir
}

function Write-Spec([string] $Dir, [string] $Name, [string] $Text) {
    Write-FixtureFile (Join-Path $Dir "build/batches/$Name.md") $Text
}

function Write-RunsMap([string] $Dir, $Map) {
    $Out = [ordered]@{}
    foreach ($Name in $Map.Keys) { $Out[$Name] = [pscustomobject] $Map[$Name] }
    Write-FixtureFile (Join-Path $Dir 'build/batches/runs.json') ((ConvertTo-Json -InputObject $Out -Depth 5) + "`n")
}

function New-RunsEntry([string] $Dispatch, [int] $Slot, [string] $Model = 'opencode-go/deepseek-v4.1-flash',
    [int] $Attempt = 1) {
    [ordered]@{
        dispatch = $Dispatch
        slot     = $Slot
        model    = $Model
        attempt  = $Attempt
        started  = '2026-11-18T00:00:00.0000000+01:00'
        branch   = 'FabLrc/x'
    }
}

function Invoke-Dispatch([string] $Dir, [string[]] $DispatchArgs) {
    Invoke-Native {
        powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Dir 'scripts/dispatch.ps1') @DispatchArgs
    }
}

function Invoke-Queue([string] $Dir) {
    Invoke-Native {
        powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Dir 'scripts/queue.ps1')
    }
}

# Stub executables: `orca` answers a ready receipt, `cargo` records the arguments and the target.
$StubBin = Join-Path $Scratch 'bin'
New-Item -ItemType Directory -Path $StubBin -Force | Out-Null
$OrcaStub = Join-Path $StubBin 'orca-stub.cmd'
$OrcaStubText = @'
@echo off
echo {"ok":true,"result":{"dispatchId":"ctx_test1234ab","state":"ready"}}
exit /b 0
'@
Write-FixtureFile $OrcaStub ($OrcaStubText -replace "`r?`n", "`r`n")
$CargoLog = Join-Path $Scratch 'cargo-calls.log'
$CargoStub = Join-Path $StubBin 'cargo.cmd'
$CargoStubText = @'
@echo off
echo %CARGO_TARGET_DIR% %*>>"{0}"
exit /b 0
'@
Write-FixtureFile $CargoStub (($CargoStubText -f $CargoLog) -replace "`r?`n", "`r`n")

$SavedTargets = $env:DQ9_TARGETS
$SavedOrca = $env:DQ9_ORCA
$SavedPath = $env:PATH
$Targets = Join-Path $Scratch 'targets'
if (Test-Path -LiteralPath $Targets) { Remove-Item -LiteralPath $Targets -Recurse -Force }
if (Test-Path -LiteralPath $CargoLog) { Remove-Item -LiteralPath $CargoLog -Force }
$env:DQ9_TARGETS = $Targets
$env:DQ9_ORCA = $OrcaStub

# Fixtures shared by the cases below. alpha is running in most of them.
$Specs = @{
    'alpha' = @'
Model: `opencode-go/deepseek-v4.1-flash`. Branch `FabLrc/alpha`.
Commit: core: alpha

## Scope
```owns
crates/a/**
```
'@
    'beta' = @'
Owner: one Orca/OMP `opencode-go/deepseek-v4.1-flash` worker, branch `FabLrc/beta`, base `main` (`0000000`).

## Scope
```owns
docs/x.md
```
'@
    'gamma' = @'
Model: `anthropic/claude-sonnet-5-5`
Depends: alpha
Commit: docs: gamma

## Scope
```owns
docs/y.md
```
'@
    'delta' = @'
## Goal
A spec without a model.

## Scope
```owns
docs/z.md
```
'@
    'epsilon' = @'
Model: `opencode-go/deepseek-v4.1-flash`

## Scope
```owns
crates/a/mod.rs
```
'@
    'zeta' = @'
Model: `opencode-go/deepseek-v4.1-flash`

## Scope
```owns
crates/a/mod.rs
docs/other/**
```
'@
}

try {
    # 1. Legacy `Owner:` line, dry run writes nothing.
    $Dir = New-Fixture 'dry-run'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    $Run = Invoke-Dispatch $Dir @('-Batch', 'beta', '-DryRun')
    Assert 'legacy Owner: model parsed' ($Run.Text -match 'model: opencode-go/deepseek-v4.1-flash') $Run.Text
    Assert 'dry run exits 0' ($Run.Code -eq 0) $Run.Text
    Assert 'dry run: slot 1 target' ($Run.Text -match ([regex]::Escape($Targets) + '\\dq9-target-slot1')) $Run.Text
    Assert 'dry run prints the prompt' ($Run.Text -match 'Batch beta') $Run.Text
    Assert 'dry run writes no prompt file' (-not (Test-Path -LiteralPath (Join-Path $Dir 'build/batches/beta.prompt.txt')))
    Assert 'dry run writes no runs.json' (-not (Test-Path -LiteralPath (Join-Path $Dir 'build/batches/runs.json')))
    Assert 'dry run creates no slot target' (-not (Test-Path -LiteralPath (Join-Path $Targets 'dq9-target-slot1')))
    Assert 'dry run does not call cargo' (-not (Test-Path -LiteralPath $CargoLog))

    # 2. Owns overlap with a running batch: `crates/a/mod.rs` against `crates/a/**`.
    $Dir = New-Fixture 'overlap'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    Write-RunsMap $Dir ([ordered]@{ alpha = New-RunsEntry 'ctx_alpha000001' 1 })
    $Run = Invoke-Dispatch $Dir @('-Batch', 'epsilon', '-DryRun')
    Assert 'overlap refused' ($Run.Code -ne 0) $Run.Text
    Assert 'overlap names the running batch' ($Run.Text -match "owns overlap with running batch 'alpha'") $Run.Text

    # 3. Dependency refusal, then acceptance once the dependency's commit is in main.
    $Dir = New-Fixture 'depends'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    $Run = Invoke-Dispatch $Dir @('-Batch', 'gamma', '-DryRun')
    Assert 'dependency refused' ($Run.Code -ne 0) $Run.Text
    Assert 'dependency named' ($Run.Text -match 'depends on not integrated batch\(es\): alpha') $Run.Text
    Invoke-FixtureGit $Dir @('commit', '-q', '--allow-empty', '-m', 'core: alpha') | Out-Null
    $Run = Invoke-Dispatch $Dir @('-Batch', 'gamma', '-DryRun')
    Assert 'dependency integrated by commit subject' ($Run.Code -eq 0) $Run.Text

    # 4. Slot choice: the lowest slot not in runs.json.
    $Dir = New-Fixture 'slots'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    Write-RunsMap $Dir ([ordered]@{
            alpha = New-RunsEntry 'ctx_alpha000001' 1
            delta = New-RunsEntry 'ctx_delta000001' 3
        })
    $Run = Invoke-Dispatch $Dir @('-Batch', 'beta', '-DryRun')
    Assert 'slot choice: exit 0' ($Run.Code -eq 0) $Run.Text
    Assert 'slot choice: slot 2' ($Run.Text -match 'dq9-target-slot2') $Run.Text

    # 5. Launch: runs.json entry, prompt file, and the prompt's slot target.
    $Dir = New-Fixture 'launch'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    $Run = Invoke-Dispatch $Dir @('-Batch', 'beta', '-Attempt', '2')
    Assert 'launch exits 0' ($Run.Code -eq 0) $Run.Text
    $PromptPath = Join-Path $Dir 'build/batches/beta.prompt.txt'
    Assert 'launch writes the prompt file' (Test-Path -LiteralPath $PromptPath)
    if (Test-Path -LiteralPath $PromptPath) {
        $Prompt = [System.IO.File]::ReadAllText($PromptPath)
        Assert 'prompt names the batch' ($Prompt -match 'Batch beta')
        Assert 'prompt carries the slot target' ($Prompt -match ([regex]::Escape((Join-Path $Targets 'dq9-target-slot1'))))
        Assert 'prompt carries the spec path' ($Prompt -match [regex]::Escape((Join-Path $Dir 'build/batches/beta.md')))
        Assert 'prompt carries the base sha' ($Prompt -match 'Start from [0-9a-f]{7,}') $Prompt
        Assert 'prompt carries the model' ($Prompt -match 'opencode-go/deepseek-v4\.1-flash') $Prompt
    }
    $RunsPath = Join-Path $Dir 'build/batches/runs.json'
    Assert 'launch writes runs.json' (Test-Path -LiteralPath $RunsPath)
    if (Test-Path -LiteralPath $RunsPath) {
        $Entry = ([System.IO.File]::ReadAllText($RunsPath) | ConvertFrom-Json).beta
        Assert 'runs.json dispatch id' ($Entry.dispatch -eq 'ctx_test1234ab') "$($Entry.dispatch)"
        Assert 'runs.json slot' ($Entry.slot -eq 1) "$($Entry.slot)"
        Assert 'runs.json attempt' ($Entry.attempt -eq 2) "$($Entry.attempt)"
        Assert 'runs.json model' ($Entry.model -eq 'opencode-go/deepseek-v4.1-flash') "$($Entry.model)"
        Assert 'runs.json branch' ($Entry.branch -eq 'FabLrc/beta') "$($Entry.branch)"
        Assert 'runs.json started is ISO' ($Entry.started -match '^\d{4}-\d\d-\d\dT') "$($Entry.started)"
    }
    $Marker = Join-Path $Targets 'dq9-target-slot1/.last-batch'
    Assert 'slot marker written' ((Test-Path -LiteralPath $Marker) -and
        ([System.IO.File]::ReadAllText($Marker)).Trim() -eq 'beta')
    Assert 'no cargo clean on a fresh slot' (-not (Test-Path -LiteralPath $CargoLog))

    # 6. A slot last used by another batch is cleaned per workspace member, not wiped.
    $Dir = New-Fixture 'clean'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    New-Item -ItemType Directory -Path (Join-Path $Targets 'dq9-target-slot1') -Force | Out-Null
    Write-FixtureFile (Join-Path $Targets 'dq9-target-slot1/.last-batch') "someone-else`n"
    Write-FixtureFile (Join-Path $Targets 'dq9-target-slot1/keep.cache') "third-party`n"
    if (Test-Path -LiteralPath $CargoLog) { Remove-Item -LiteralPath $CargoLog -Force }
    $env:PATH = "$StubBin;$SavedPath"
    try {
        $Run = Invoke-Dispatch $Dir @('-Batch', 'beta')
    } finally {
        $env:PATH = $SavedPath
    }
    Assert 'clean: launch exits 0' ($Run.Code -eq 0) $Run.Text
    $Calls = if (Test-Path -LiteralPath $CargoLog) { [System.IO.File]::ReadAllText($CargoLog) } else { '' }
    Assert 'clean: cargo clean -p crate-a' ($Calls -match 'clean -p crate-a') $Calls
    Assert 'clean: target dir of the slot' ($Calls -match [regex]::Escape((Join-Path $Targets 'dq9-target-slot1'))) $Calls
    Assert 'clean: third-party cache kept' (Test-Path -LiteralPath (Join-Path $Targets 'dq9-target-slot1/keep.cache'))
    Assert 'clean: marker updated' (([System.IO.File]::ReadAllText((Join-Path $Targets 'dq9-target-slot1/.last-batch'))).Trim() -eq 'beta')

    # 7. Queue: states, and a wave of ready specs disjoint from running ones and from each other.
    $Dir = New-Fixture 'queue'
    foreach ($Key in $Specs.Keys) { Write-Spec $Dir $Key $Specs[$Key] }
    Write-RunsMap $Dir ([ordered]@{ alpha = New-RunsEntry 'ctx_alpha000001' 1 })
    $Run = Invoke-Queue $Dir
    Assert 'queue exits 0' ($Run.Code -eq 0) $Run.Text
    Assert 'queue: running state' ($Run.Text -match '(?m)^alpha\s+running') $Run.Text
    Assert 'queue: draft state' ($Run.Text -match '(?m)^delta\s+draft') $Run.Text
    Assert 'queue: waiting on a dependency' ($Run.Text -match '(?m)^gamma\s+waiting') $Run.Text
    Assert 'queue: beta is ready' ($Run.Text -match '(?m)^beta\s+ready') $Run.Text
    Assert 'queue: wave lists beta' ($Run.Text -match 'dispatch\.ps1 -Batch beta') $Run.Text
    Assert 'queue: wave excludes the overlapping zeta' (-not ($Run.Text -match 'dispatch\.ps1 -Batch zeta')) $Run.Text
    Assert 'queue: wave excludes the running alpha' (-not ($Run.Text -match 'dispatch\.ps1 -Batch alpha')) $Run.Text
    Assert 'queue: wave excludes the draft delta' (-not ($Run.Text -match 'dispatch\.ps1 -Batch delta')) $Run.Text

    # 8. Queue: no slot left when five batches run.
    $Dir = New-Fixture 'queue-full'
    Write-Spec $Dir 'beta' $Specs['beta']
    $BusySpec = @'
Model: `anthropic/claude-sonnet-5-5`

```owns
docs/b{0}.md
```
'@
    $Map = [ordered]@{}
    foreach ($Index in 1..5) {
        Write-Spec $Dir "busy$Index" ($BusySpec -f $Index)
        $Map["busy$Index"] = New-RunsEntry "ctx_busy00000$Index" $Index
    }
    Write-RunsMap $Dir $Map
    $Run = Invoke-Queue $Dir
    Assert 'queue: five slots used' ($Run.Code -eq 0 -and $Run.Text -match 'slots: 5 of 5 used') $Run.Text
    Assert 'queue: no wave at five running' ($Run.Text -match 'next wave: none \(all five slots are used\)') $Run.Text

    # 9. A failed start is reported with its state and never recorded.
    $FailStub = Join-Path $StubBin 'orca-fail.cmd'
    $FailStubText = @'
@echo off
echo {"ok":false,"result":{"dispatchId":"ctx_fail123456","state":"failed"}}
exit /b 1
'@
    Write-FixtureFile $FailStub ($FailStubText -replace "`r?`n", "`r`n")
    $Dir = New-Fixture 'failed-start'
    Write-Spec $Dir 'beta' $Specs['beta']
    $env:DQ9_ORCA = $FailStub
    $Run = Invoke-Dispatch $Dir @('-Batch', 'beta')
    $env:DQ9_ORCA = $OrcaStub
    Assert 'failed start: non-zero exit' ($Run.Code -ne 0) $Run.Text
    Assert 'failed start: names the state' ($Run.Text -match "worker-start failed \(state 'failed'") $Run.Text
    Assert 'failed start: nothing recorded' (-not (Test-Path -LiteralPath (Join-Path $Dir 'build/batches/runs.json')))
} finally {
    $env:DQ9_TARGETS = $SavedTargets
    $env:DQ9_ORCA = $SavedOrca
    $env:PATH = $SavedPath
}

Write-Output ''
Write-Output ("{0} passed, {1} failed" -f $script:Passed, $script:Failed)
if ($script:Failed -gt 0) {
    Write-Output "fixtures kept in $Scratch"
    exit 1
}
exit 0
