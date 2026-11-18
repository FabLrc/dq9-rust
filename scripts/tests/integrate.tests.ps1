# Tests of scripts/integrate.ps1 on throwaway repositories (no Cargo, no ROM, no network).
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/tests/integrate.tests.ps1
#
# Every test builds its own repository under build/scratch/integrate-tests-<pid>/ (ignored by
# git): a `main` checkout holding a copy of the script under test, worker branches in linked
# worktrees, and a STUB `scripts/verify.ps1` committed in that repository. integrate.ps1 runs
# the verify script found in the tree it verifies, at the fixed relative path
# `scripts/verify.ps1`; the stub therefore replaces it by living in the throwaway tree, and
# the production script has no variable, parameter or switch to skip or swap its verify step.
# The stub fails (exit 2) unless it is called with -RequireRom, logs each call, fails (exit 1)
# when `bad.txt` is in the tree, litters the tree when `dirt.flag` is in it, and rewrites the
# README coverage block the way the real `coverage` step does.
# %LOCALAPPDATA% and DQ9_ROM/DQ9_ORCA are redirected so cleanups never touch the real targets.
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ScriptUnderTest = Join-Path $RepoRoot 'scripts/integrate.ps1'
$Scratch = Join-Path $RepoRoot ("build/scratch/integrate-tests-{0}" -f $PID)
$Failures = 0
$Total = 0

function Assert-True([bool] $Condition, [string] $Name) {
    $script:Total++
    if ($Condition) {
        Write-Host "  ok   - $Name"
    } else {
        $script:Failures++
        Write-Host "  FAIL - $Name"
    }
}

function Assert-Match([string] $Text, [string] $Pattern, [string] $Name) {
    $Hit = $Text -match $Pattern
    Assert-True $Hit $Name
    if (-not $Hit) { Write-Host "         pattern: $Pattern" }
}

function Show-Output($Result) {
    $Result.Out -split "`r?`n" | ForEach-Object { Write-Host "         | $_" }
}

function G([string] $Dir, [string[]] $GitArgs) {
    $Out = @(& git -C $Dir @GitArgs 2>&1 | ForEach-Object { "$_" })
    if ($LASTEXITCODE -ne 0) { throw "git $($GitArgs -join ' ') failed in ${Dir}: $($Out -join ' ')" }
    $Out
}

function Write-Text([string] $Path, [string] $Text) {
    $Parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $Parent)) { New-Item -ItemType Directory -Path $Parent -Force | Out-Null }
    [System.IO.File]::WriteAllText($Path, $Text, [System.Text.UTF8Encoding]::new($false))
}

$StubVerify = @'
param([switch] $Fast, [string[]] $Only, [switch] $RequireRom)
if (-not $RequireRom) { Write-Host 'stub verify: -RequireRom missing'; exit 2 }
$Tip = (& git rev-parse --short HEAD)
Add-Content -LiteralPath $env:STUB_VERIFY_LOG -Value ("{0} {1}" -f (Get-Location).Path, $Tip)
if (Test-Path -LiteralPath 'bad.txt') { Write-Host 'stub verify: bad.txt is in the tree'; exit 1 }
if (Test-Path -LiteralPath 'dirt.flag') { [System.IO.File]::WriteAllText('junk.txt', 'x') }
$Count = @(& git ls-files).Count
$Lines = [System.IO.File]::ReadAllLines('README.md')
$Out = New-Object System.Collections.Generic.List[string]
$In = $false
foreach ($L in $Lines) {
    if ($L -eq '<!-- coverage:start -->') { $Out.Add($L); $Out.Add("files: $Count"); $In = $true; continue }
    if ($L -eq '<!-- coverage:end -->') { $In = $false }
    if (-not $In) { $Out.Add($L) }
}
[System.IO.File]::WriteAllText('README.md', (($Out.ToArray() -join "`n") + "`n"))
exit 0
'@

$MockOrca = @'
Add-Content -LiteralPath $env:MOCK_ORCA_LOG -Value ($args -join ' ')
$Dispatch = [string] $args[$args.Count - 1]
$Map = Get-Content -LiteralPath $env:MOCK_ORCA_MAP -Raw | ConvertFrom-Json
$Junction = $Map.$Dispatch
if ($Junction) {
    Add-Content -LiteralPath $env:MOCK_ORCA_LOG -Value ("junction-present {0} {1}" -f $Dispatch, (Test-Path -LiteralPath $Junction))
}
if ($Dispatch -eq 'D-FAIL') { Write-Host 'boom'; exit 1 }
exit 0
'@

# A throwaway repository: main checkout with the script under test and the stub verify.
function New-Fixture([string] $Name) {
    $Dir = Join-Path $Scratch $Name
    $Main = Join-Path $Dir 'main'
    New-Item -ItemType Directory -Path $Main -Force | Out-Null
    G $Main @('init', '--quiet') | Out-Null
    G $Main @('symbolic-ref', 'HEAD', 'refs/heads/main') | Out-Null
    G $Main @('config', 'user.name', 'Test') | Out-Null
    G $Main @('config', 'user.email', 'test@example.invalid') | Out-Null
    G $Main @('config', 'core.autocrlf', 'false') | Out-Null
    Write-Text (Join-Path $Main 'scripts/verify.ps1') $StubVerify
    Copy-Item -LiteralPath $ScriptUnderTest -Destination (Join-Path $Main 'scripts/integrate.ps1')
    Write-Text (Join-Path $Main 'README.md') "# Throwaway`n<!-- coverage:start -->`nfiles: 0`n<!-- coverage:end -->`ntail`n"
    Write-Text (Join-Path $Main '.gitignore') "/build/`n/tools/`n"
    G $Main @('add', '-A') | Out-Null
    G $Main @('commit', '--quiet', '-m', 'init: throwaway') | Out-Null
    $Log = Join-Path $Dir 'verify.log'
    $env:STUB_VERIFY_LOG = $Log
    [pscustomobject]@{ Dir = $Dir; Main = $Main; Log = $Log }
}

function Get-Head($Fix, [string] $Rev = 'main') { @(G $Fix.Main @('rev-parse', $Rev))[0] }
function Get-VerifyCalls($Fix) {
    if (-not (Test-Path -LiteralPath $Fix.Log)) { return 0 }
    @(Get-Content -LiteralPath $Fix.Log).Count
}

# A worker branch in its own worktree with one commit; its spec is build/batches/<batch>.md.
function New-Worker($Fix, [string] $Branch, [hashtable] $Files, [string[]] $Owns, [string] $CommitLine = $null) {
    $Batch = $Branch.Substring($Branch.LastIndexOf('/') + 1)
    $Worktree = Join-Path $Fix.Dir "wt-$Batch"
    G $Fix.Main @('worktree', 'add', '--quiet', '-b', $Branch, $Worktree, 'main') | Out-Null
    foreach ($Key in $Files.Keys) { Write-Text (Join-Path $Worktree $Key) $Files[$Key] }
    G $Worktree @('add', '-A') | Out-Null
    G $Worktree @('commit', '--quiet', '-m', "wip: $Batch") | Out-Null
    $Spec = "# Batch $Batch`n"
    if ($CommitLine) { $Spec += "Commit: $CommitLine`n" }
    $Spec += "`n``````owns`n" + ($Owns -join "`n") + "`n```````n"
    Write-Text (Join-Path $Fix.Main "build/batches/$Batch.md") $Spec
    $Worktree
}

function Format-Arg([string[]] $Values) { ($Values | ForEach-Object { "'" + ($_ -replace "'", "''") + "'" }) -join ',' }

# Runs the script under test in a child PowerShell from the fixture's main checkout.
function Invoke-Integrate($Fix, [string[]] $Branch, [string[]] $Message, [switch] $Apply, [switch] $Rebase, [switch] $Cleanup) {
    $Command = "& '$($Fix.Main)\scripts\integrate.ps1' -Branch $(Format-Arg $Branch)"
    if ($Message) { $Command += " -Message $(Format-Arg $Message)" }
    if ($Apply) { $Command += ' -Apply' }
    if ($Rebase) { $Command += ' -Rebase' }
    if ($Cleanup) { $Command += ' -Cleanup' }
    Push-Location $Fix.Main
    $Saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $Out = & powershell -NoProfile -ExecutionPolicy Bypass -Command $Command 2>&1 | Out-String
        $Code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $Saved
        Pop-Location
    }
    [pscustomobject]@{ Code = $Code; Out = $Out }
}

function Test-NoIntegrationWorktree($Fix) {
    $Listed = @(G $Fix.Main @('worktree', 'list', '--porcelain') | Where-Object { $_ -like '*dq9-integrate-wt-*' })
    $OnDisk = @(Get-ChildItem -LiteralPath $env:LOCALAPPDATA -Directory -Filter 'dq9-integrate-wt-*' -ErrorAction SilentlyContinue)
    ($Listed.Count -eq 0) -and ($OnDisk.Count -eq 0)
}

$SavedEnv = @{
    LOCALAPPDATA = $env:LOCALAPPDATA; DQ9_ROM = $env:DQ9_ROM; DQ9_ORCA = $env:DQ9_ORCA
    STUB_VERIFY_LOG = $env:STUB_VERIFY_LOG; CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR
}
try {
    if (Test-Path -LiteralPath $Scratch) { Remove-Item -LiteralPath $Scratch -Recurse -Force }
    New-Item -ItemType Directory -Path $Scratch -Force | Out-Null
    $env:LOCALAPPDATA = Join-Path $Scratch 'local'
    New-Item -ItemType Directory -Path $env:LOCALAPPDATA -Force | Out-Null
    $env:DQ9_ROM = Join-Path $Scratch 'dummy.nds'
    Write-Text $env:DQ9_ROM 'not a rom'
    $env:DQ9_ORCA = $null

    # --- two green branches: one verify, two commits, README folded into the last one -----------
    Write-Host 'two green branches'
    $Fix = New-Fixture 'green'
    New-Worker $Fix 'FabLrc/t-a' @{ 'a/one.txt' = "1`n" } @('a/**') 'feat: add a (from the spec)' | Out-Null
    New-Worker $Fix 'FabLrc/t-b' @{ 'b/two.txt' = "2`n" } @('b/**') | Out-Null
    $Before = Get-Head $Fix
    $HeadA = Get-Head $Fix 'FabLrc/t-a'
    $HeadB = Get-Head $Fix 'FabLrc/t-b'
    $Dry = Invoke-Integrate $Fix @('FabLrc/t-a', 'FabLrc/t-b') @('', 'feat: add b')
    Assert-True ($Dry.Code -eq 0) 'dry run exits 0'
    Assert-True ((Get-Head $Fix) -eq $Before) 'dry run leaves main untouched'
    Assert-True ((Get-VerifyCalls $Fix) -eq 1) 'dry run verifies the combined tree once'
    Assert-True (Test-NoIntegrationWorktree $Fix) 'dry run removes its integration worktree'
    $TracePath = Join-Path $Fix.Main 'build/batches/integrated.txt'
    Assert-True (-not (Test-Path -LiteralPath $TracePath)) 'dry run writes no integration trace'
    $Run = Invoke-Integrate $Fix @('FabLrc/t-a,FabLrc/t-b') @('', 'feat: add b') -Apply
    if ($Run.Code -ne 0) { Show-Output $Run }
    Assert-True ($Run.Code -eq 0) 'apply exits 0'
    Assert-True ((Get-VerifyCalls $Fix) -eq 2) 'apply verifies once more (one verify per run, not per branch)'
    Assert-True (@(G $Fix.Main @('rev-list', '--count', "$Before..main"))[0] -eq '2') 'main gains exactly two commits'
    Assert-True (@(G $Fix.Main @('rev-list', '--merges', '--count', "$Before..main"))[0] -eq '0') 'main stays linear'
    $Subjects = @(G $Fix.Main @('log', '--reverse', '--format=%s', "$Before..main"))
    Assert-True (($Subjects -join '|') -eq 'feat: add a (from the spec)|feat: add b') 'commit subjects: spec Commit line, then -Message entry'
    $LastFiles = @(G $Fix.Main @('show', '--name-only', '--format=', 'main'))
    $FirstFiles = @(G $Fix.Main @('show', '--name-only', '--format=', 'main~1'))
    Assert-True ($LastFiles -contains 'README.md') 'the README block is folded into the last integration commit'
    Assert-True (-not ($FirstFiles -contains 'README.md')) 'and not into the earlier one'
    Assert-True ((G $Fix.Main @('show', 'main:README.md')) -contains 'files: 6') 'main holds the regenerated block'
    Assert-True (@(G $Fix.Main @('status', '--porcelain')).Count -eq 0) 'main checkout is clean after the run'
    Assert-True ((Get-Head $Fix 'FabLrc/t-a') -eq $HeadA -and (Get-Head $Fix 'FabLrc/t-b') -eq $HeadB) 'worker branches are not modified'
    Assert-True (Test-NoIntegrationWorktree $Fix) 'integration worktree is gone'
    Assert-True (Test-Path -LiteralPath $TracePath) 'apply writes the integration trace'
    $Trace = @(Get-Content -LiteralPath $TracePath)
    Assert-True ($Trace.Count -eq 2) 'one trace line per integrated branch'
    $Shas = @(G $Fix.Main @('log', '--reverse', '--format=%H', "$Before..main"))
    Assert-True ($Trace[0] -match ('^t-a ' + $Shas[0] + ' \d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')) 'trace line: batch, squash commit sha, ISO date'
    Assert-True ($Trace[1] -match ('^t-b ' + $Shas[1] + ' ')) 'second trace line names its own commit'

    # --- culprit in the middle ----------------------------------------------------------------
    Write-Host 'culprit in the middle'
    $Fix = New-Fixture 'culprit'
    New-Worker $Fix 'FabLrc/c-1' @{ 'a/one.txt' = "1`n" } @('a/**') | Out-Null
    New-Worker $Fix 'FabLrc/c-2' @{ 'bad.txt' = "x`n" } @('bad.txt') | Out-Null
    New-Worker $Fix 'FabLrc/c-3' @{ 'b/three.txt' = "3`n" } @('b/**') | Out-Null
    $Before = Get-Head $Fix
    $Run = Invoke-Integrate $Fix @('FabLrc/c-1', 'FabLrc/c-2', 'FabLrc/c-3') @('x: one', 'x: two', 'x: three') -Apply
    Assert-True ($Run.Code -ne 0) 'run fails'
    Assert-Match $Run.Out 'culprit: FabLrc/c-2' 'names the middle branch as culprit'
    Assert-True ((Get-Head $Fix) -eq $Before) 'main untouched'
    Assert-True ((Get-VerifyCalls $Fix) -eq 3) 'combined + two prefix verifications (binary search)'
    Assert-True (Test-NoIntegrationWorktree $Fix) 'integration worktree is gone'

    # --- owns violation -----------------------------------------------------------------------
    Write-Host 'owns violation'
    $Fix = New-Fixture 'owns'
    New-Worker $Fix 'FabLrc/o-1' @{ 'a/one.txt' = "1`n"; 'outside.txt' = "y`n" } @('a/**') | Out-Null
    $Before = Get-Head $Fix
    $Run = Invoke-Integrate $Fix @('FabLrc/o-1') @('x: one') -Apply
    Assert-True ($Run.Code -ne 0) 'run fails'
    Assert-Match $Run.Out 'not covered by ```owns: outside\.txt' 'reports the unowned file'
    Assert-True ((Get-VerifyCalls $Fix) -eq 0) 'no verify runs after a failed cheap check'
    Assert-True ((Get-Head $Fix) -eq $Before) 'main untouched'

    # --- verify dirt other than the README block fails ----------------------------------------
    Write-Host 'dirt outside the README block'
    $Fix = New-Fixture 'dirt'
    New-Worker $Fix 'FabLrc/d-1' @{ 'dirt.flag' = "1`n" } @('dirt.flag') | Out-Null
    $Before = Get-Head $Fix
    $Run = Invoke-Integrate $Fix @('FabLrc/d-1') @('x: one') -Apply
    Assert-True ($Run.Code -ne 0) 'run fails'
    Assert-Match $Run.Out 'verify dirt' 'reports the dirt'
    Assert-True ((Get-Head $Fix) -eq $Before) 'main untouched'

    # --- conflict between two branches --------------------------------------------------------
    Write-Host 'conflicting branches'
    $Fix = New-Fixture 'conflict'
    New-Worker $Fix 'FabLrc/k-1' @{ 'a/same.txt' = "one`n" } @('a/**') | Out-Null
    New-Worker $Fix 'FabLrc/k-2' @{ 'a/same.txt' = "two`n" } @('a/**') | Out-Null
    $Before = Get-Head $Fix
    $Run = Invoke-Integrate $Fix @('FabLrc/k-1', 'FabLrc/k-2') @('x: one', 'x: two') -Apply
    Assert-True ($Run.Code -ne 0) 'run fails'
    Assert-Match $Run.Out 'merge \[k-2\]' 'names the second branch'
    Assert-True ((Get-VerifyCalls $Fix) -eq 0) 'no verify runs'
    Assert-True ((Get-Head $Fix) -eq $Before) 'main untouched'
    Assert-True (Test-NoIntegrationWorktree $Fix) 'integration worktree is gone'

    # --- a branch that does not contain main ---------------------------------------------------
    Write-Host 'rebase'
    $Fix = New-Fixture 'rebase'
    New-Worker $Fix 'FabLrc/b-1' @{ 'a/one.txt' = "1`n" } @('a/**') | Out-Null
    Write-Text (Join-Path $Fix.Main 'moved.txt') "m`n"
    G $Fix.Main @('add', 'moved.txt') | Out-Null
    G $Fix.Main @('commit', '--quiet', '-m', 'chore: main moves') | Out-Null
    $Run = Invoke-Integrate $Fix @('FabLrc/b-1') @('x: one')
    Assert-True ($Run.Code -ne 0) 'fails without -Rebase'
    Assert-Match $Run.Out 'rebase needed' 'asks for a rebase'
    $Run = Invoke-Integrate $Fix @('FabLrc/b-1') @('x: one') -Rebase
    Assert-True ($Run.Code -eq 0) '-Rebase rebases then passes'

    # --- cleanup: slot and integrate targets are refused, legacy ones removed ------------------
    Write-Host 'cleanup refuses slot* and integrate targets'
    $Fix = New-Fixture 'targets'
    $Decoy = Join-Path $Fix.Dir 'decoy'
    Write-Text (Join-Path $Decoy 'sentinel.txt') 'keep me'
    foreach ($Batch in @('slot2', 'integrate', 'legacy1')) {
        $Wt = New-Worker $Fix "FabLrc/$Batch" @{ "a/$Batch.txt" = "$Batch`n" } @('a/**')
        Write-Text (Join-Path $env:LOCALAPPDATA "dq9-target-$Batch/file.bin") 'target'
    }
    New-Item -ItemType Junction -Path (Join-Path $Fix.Dir 'wt-legacy1/tools') -Target $Decoy | Out-Null
    $Before = Get-Head $Fix
    $Run = Invoke-Integrate $Fix @('FabLrc/slot2', 'FabLrc/integrate', 'FabLrc/legacy1') @('x: one', 'x: two', 'x: three') -Apply -Cleanup
    if ($Run.Code -ne 0) { Show-Output $Run }
    Assert-True ($Run.Code -eq 0) 'apply + cleanup exits 0'
    Assert-True (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'dq9-target-slot2/file.bin')) 'dq9-target-slot2 is kept'
    Assert-True (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'dq9-target-integrate/file.bin')) 'dq9-target-integrate is kept'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'dq9-target-legacy1'))) 'legacy per-batch target is removed'
    Assert-True (Test-Path -LiteralPath (Join-Path $Decoy 'sentinel.txt')) 'the junction target survives (link removed, not followed)'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $Fix.Dir 'wt-legacy1'))) 'the worktree is removed'
    Assert-True (@(G $Fix.Main @('branch', '--list', 'FabLrc/*')).Count -eq 0) 'the branches are deleted'
    Assert-True (@(G $Fix.Main @('rev-list', '--count', "$Before..main"))[0] -eq '3') 'three commits integrated'

    # --- cleanup with and without a runs.json entry ---------------------------------------------
    Write-Host 'cleanup with and without runs.json entries'
    $Fix = New-Fixture 'runs'
    $McpLog = Join-Path $Fix.Dir 'orca.log'
    $Map = Join-Path $Fix.Dir 'orca-map.json'
    $Mock = Join-Path $Fix.Dir 'mock-orca.ps1'
    Write-Text $Mock $MockOrca
    $JunctionTarget = Join-Path $Fix.Dir 'junction-target'
    New-Item -ItemType Directory -Path $JunctionTarget -Force | Out-Null
    foreach ($Batch in @('r1', 'r2', 'r3', 'r4')) {
        $Wt = New-Worker $Fix "FabLrc/$Batch" @{ "a/$Batch.txt" = "$Batch`n" } @('a/**')
        New-Item -ItemType Junction -Path (Join-Path $Wt 'tools') -Target $JunctionTarget | Out-Null
    }
    $Runs = [ordered]@{
        r1 = @{ dispatch = 'D-R1'; slot = 1; model = 'm'; attempt = 1; started = '2026-11-18T00:00:00Z'; branch = 'FabLrc/r1' }
        other = @{ dispatch = 'D-OTHER'; slot = 2; model = 'm'; attempt = 1; started = '2026-11-18T00:00:00Z'; branch = 'FabLrc/other' }
        weird = @{ dispatch = 'D-R2'; slot = 3; model = 'm'; attempt = 1; started = '2026-11-18T00:00:00Z'; branch = 'FabLrc/r2' }
        r4 = @{ dispatch = 'D-FAIL'; slot = 4; model = 'm'; attempt = 1; started = '2026-11-18T00:00:00Z'; branch = 'FabLrc/r4' }
    }
    Write-Text (Join-Path $Fix.Main 'build/batches/runs.json') (ConvertTo-Json -InputObject $Runs -Depth 5)
    $MapObject = @{
        'D-R1' = (Join-Path $Fix.Dir 'wt-r1/tools'); 'D-R2' = (Join-Path $Fix.Dir 'wt-r2/tools')
        'D-FAIL' = (Join-Path $Fix.Dir 'wt-r4/tools')
    }
    Write-Text $Map (ConvertTo-Json -InputObject $MapObject)
    $env:DQ9_ORCA = $Mock
    $env:MOCK_ORCA_LOG = $McpLog
    $env:MOCK_ORCA_MAP = $Map
    $All = @('FabLrc/r1', 'FabLrc/r2', 'FabLrc/r3', 'FabLrc/r4')
    $Run = Invoke-Integrate $Fix $All @('x: one', 'x: two', 'x: three', 'x: four') -Apply
    Assert-True ($Run.Code -eq 0) 'integration without -Cleanup exits 0'
    Assert-True (-not (Test-Path -LiteralPath $McpLog)) 'orca is not called without -Cleanup'
    $Run = Invoke-Integrate $Fix $All -Cleanup
    if ($Run.Code -ne 0) { Show-Output $Run }
    Assert-True ($Run.Code -eq 0) 'cleanup-only exits 0 (a failed release is not fatal)'
    $Calls = @(Get-Content -LiteralPath $McpLog | Where-Object { $_ -like 'orchestration*' })
    Assert-True ($Calls.Count -eq 3) 'worker-release is called for the three batches with an entry, not for r3'
    Assert-True ($Calls -contains 'orchestration worker-release --dispatch D-R1') 'entry found by key (r1)'
    Assert-True ($Calls -contains 'orchestration worker-release --dispatch D-R2') 'entry found by its branch field (weird -> FabLrc/r2)'
    Assert-True ($Calls -contains 'orchestration worker-release --dispatch D-FAIL') 'the failing release is attempted'
    $Present = @(Get-Content -LiteralPath $McpLog | Where-Object { $_ -like 'junction-present*' })
    Assert-True ($Present.Count -eq 3 -and @($Present | Where-Object { $_ -notlike '* True' }).Count -eq 0) 'every release runs BEFORE its junction is removed'
    Assert-Match $Run.Out 'WARNING release \[r4\]' 'the failed release is reported as a warning'
    Assert-True (@(G $Fix.Main @('branch', '--list', 'FabLrc/*')).Count -eq 0) 'all four branches are deleted despite the failed release'
    $After = Get-Content -LiteralPath (Join-Path $Fix.Main 'build/batches/runs.json') -Raw | ConvertFrom-Json
    $Keys = @($After.PSObject.Properties | ForEach-Object { $_.Name })
    Assert-True (($Keys -join ',') -eq 'other') 'runs.json keeps only the unrelated entry'

    Write-Host 'cleanup without runs.json'
    $Fix = New-Fixture 'noruns'
    $Wt = New-Worker $Fix 'FabLrc/n1' @{ 'a/n1.txt' = "n`n" } @('a/**')
    Remove-Item -LiteralPath $McpLog -ErrorAction SilentlyContinue
    $Run = Invoke-Integrate $Fix @('FabLrc/n1') @('x: one') -Apply -Cleanup
    Assert-True ($Run.Code -eq 0) 'apply + cleanup exits 0'
    Assert-True (-not (Test-Path -LiteralPath $McpLog)) 'orca is not called when there is no entry'
    Assert-True (-not (Test-Path -LiteralPath $Wt)) 'the worktree is removed anyway'
} finally {
    foreach ($Name in $SavedEnv.Keys) { Set-Item -LiteralPath "Env:$Name" -Value $SavedEnv[$Name] -ErrorAction SilentlyContinue }
    if ($null -eq $SavedEnv.DQ9_ORCA) { Remove-Item Env:DQ9_ORCA -ErrorAction SilentlyContinue }
    Remove-Item Env:MOCK_ORCA_LOG, Env:MOCK_ORCA_MAP -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $Scratch) {
        # Links first, so the recursive delete never walks through one.
        Get-ChildItem -LiteralPath $Scratch -Recurse -Force -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint } |
            ForEach-Object { [System.IO.Directory]::Delete($_.FullName, $false) }
        Remove-Item -LiteralPath $Scratch -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
Write-Host ("integrate.tests.ps1: {0} assertion(s), {1} failure(s)" -f $Total, $Failures)
if ($Failures -gt 0) { exit 1 }
exit 0
