# Builds the decompilation corpus of the `game` functions of re/functions.txt, plus its indexes:
#   <Out>/<module>.c   one block per function: `// <addr> <size> <name>` then the decompiled C
#   <Out>/index.txt    `<addr> <name> <file>:<line>`
#   <Out>/calls.txt    `<caller addr> <callee addr or name>`, deduplicated
#   <Out>/failures.txt `<addr> <name> <file> <reason>` for blocks whose body is a failure
# Reads with `o` note: `scripts/decomp-find.ps1 <addr|name>` answers one function without Ghidra.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/ghidra-corpus.ps1
#        powershell -ExecutionPolicy Bypass -File scripts/ghidra-corpus.ps1 -Only ov_017 -Refresh
# Resumable: a module is skipped when its .c is newer than re/functions.txt, re/symbols.txt, the
# ROM (ROM/decompiled text changed => everything is stale) and DumpCorpus.java; -Refresh dumps
# everything again. Ghidra locks a project while it runs, so -Jobs copies re/ghidra that many
# times under build/ghidra-jobs/ and deletes the copies at the end.
#
# The output is ROM-derived data: build/decomp/ is ignored by git. Never commit it, never copy
# it into committed evidence.
param(
    [string]$Out,
    [int]$Jobs = 3,
    [switch]$Refresh,
    [string[]]$Only,
    [int]$Timeout = 60,
    [int]$ModuleTimeout = 1800
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

# Main checkout when this script runs from a linked worktree, whose .git is a file.
function Get-MainRoot {
    $git = Join-Path $Root '.git'
    if (-not (Test-Path $git -PathType Leaf)) { return $Root }
    $line = Get-Content $git -TotalCount 1
    if ($line -match '^gitdir:\s*(.+)$') {
        $dir = $Matches[1].Trim()
        if (-not [IO.Path]::IsPathRooted($dir)) { $dir = Join-Path $Root $dir }
        $dir = [IO.Path]::GetFullPath($dir)
        $dir = Split-Path -Parent (Split-Path -Parent $dir)
        if ($dir) { return (Split-Path -Parent $dir) }
    }
    return $Root
}

function Resolve-SharedPath([string] $Relative) {
    $local = Join-Path $Root $Relative
    if (Test-Path $local) { return $local }
    $main = Get-MainRoot
    if ($main -and $main -ne $Root) {
        $shared = Join-Path $main $Relative
        if (Test-Path $shared) { return $shared }
    }
    return $local
}

# ROM the Ghidra project was imported from: DQ9_ROM, rom/*.nds or *.nds at either checkout root.
function Get-RomPath {
    if ($env:DQ9_ROM -and (Test-Path $env:DQ9_ROM -PathType Leaf)) { return $env:DQ9_ROM }
    $roots = @($Root)
    $main = Get-MainRoot
    if ($main -and $main -ne $Root) { $roots += $main }
    foreach ($root in $roots) {
        $found = @(Get-ChildItem -Path (Join-Path $root 'rom') -Filter '*.nds' -File -ErrorAction SilentlyContinue) +
            @(Get-ChildItem -Path $root -Filter '*.nds' -File -ErrorAction SilentlyContinue)
        if ($found.Count -gt 0) { return $found[0].FullName }
    }
    return $null
}

function Format-Size([long] $Bytes) {
    if ($Bytes -ge 1GB) { return ('{0:N1} GB' -f ($Bytes / 1GB)) }
    if ($Bytes -ge 1MB) { return ('{0:N1} MB' -f ($Bytes / 1MB)) }
    return ('{0:N0} KB' -f ($Bytes / 1KB))
}

function Get-DirectorySize([string] $Path) {
    if (-not (Test-Path $Path)) { return 0 }
    return [long] ((Get-ChildItem -Path $Path -Recurse -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum)
}

function Quote-Argument([string] $Text) {
    if ($Text -match '^[^\s"]+$') { return $Text }
    return '"' + ($Text -replace '"', '\"') + '"'
}

# A log file stays locked for a moment after its process exits (redirect handle, antivirus):
# retry instead of failing the whole run on a transient IOException.
function Read-TextLines([string] $Path) {
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        try { return [IO.File]::ReadAllLines($Path) } catch { Start-Sleep -Milliseconds 100 }
    }
    [Console]::Error.WriteLine("corpus: cannot read $Path")
    return @()
}

$HeaderRegex = [regex]'^// (?<addr>0x[0-9a-f]+|ov_\d+::[0-9a-f]+) 0x(?<size>[0-9a-f]+) (?<name>\S+)\s*$'
$CallRegex = [regex]'(?<![A-Za-z0-9_])(?<name>[A-Za-z_][A-Za-z0-9_]*)\s*\('
$FailurePrefix = '// decompile failed: '
# C keywords and decompiler artefacts that take a parenthesis: never a call target.
$NotCallees = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($word in @('if', 'while', 'for', 'switch', 'return', 'sizeof', 'catch', 'defined',
        'do', 'else', 'case', 'goto', 'break', 'continue', 'typedef', 'static_cast',
        'reinterpret_cast', 'const_cast', 'dynamic_cast', 'throw', 'new', 'delete',
        'WARNING', 'SCRIPT', 'CONCAT', 'SUB', 'SEXT', 'ZEXT', 'abs', 'func')) {
    $NotCallees.Add($word) | Out-Null
}

# --- inputs -------------------------------------------------------------------------------
$Map = Resolve-SharedPath 're/functions.txt'
$Symbols = Resolve-SharedPath 're/symbols.txt'
$CorpusScript = Join-Path $Root 're/ghidra_scripts/DumpCorpus.java'
$Headless = Resolve-SharedPath 'tools/ghidra/support/analyzeHeadless.bat'
$env:JAVA_HOME = Resolve-SharedPath 'tools/jdk'
$Project = if ($env:DQ9_GHIDRA_PROJECT) { $env:DQ9_GHIDRA_PROJECT } else { Resolve-SharedPath 're/ghidra' }
if (-not (Test-Path $Map)) { throw "ghidra-corpus: missing $Map" }
if (-not (Test-Path $CorpusScript)) { throw "ghidra-corpus: missing $CorpusScript" }
if (-not (Test-Path $Headless)) { throw "ghidra-corpus: missing $Headless; run scripts/setup-re-tools.ps1" }
if (-not (Test-Path $Project)) { throw "ghidra-corpus: missing Ghidra project $Project" }
if (-not $Out) { $Out = Join-Path (Get-MainRoot) 'build/decomp' }
$Out = [IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $Out | Out-Null

# --- modules and staleness ----------------------------------------------------------------
$mapLines = [IO.File]::ReadAllLines($Map)
$counts = [ordered]@{}
foreach ($line in $mapLines) {
    $text = $line.Trim()
    if ($text -eq '' -or $text.StartsWith('#')) { continue }
    $parts = $text -split '\s+'
    if ($parts.Count -lt 4 -or $parts[2] -ne 'game') { continue }
    $module = if ($parts[0].Contains('::')) { $parts[0].Substring(0, $parts[0].IndexOf('::')) } else { 'arm9' }
    if (-not $counts.Contains($module)) { $counts[$module] = 0 }
    $counts[$module]++
}
$allModules = @($counts.Keys)
if ($Only) {
    foreach ($module in $Only) {
        if (-not $counts.Contains($module)) {
            throw "ghidra-corpus: unknown module '$module' (known: $($allModules -join ', '))"
        }
    }
    $modules = @($allModules | Where-Object { $Only -contains $_ })
} else {
    $modules = $allModules
}

$stampInputs = @($Map, $Symbols, $CorpusScript)
$rom = Get-RomPath
if ($rom) { $stampInputs += $rom }
$stamp = ($stampInputs | ForEach-Object { (Get-Item $_).LastWriteTimeUtc } |
    Measure-Object -Maximum).Maximum

$work = @()
$skipped = 0
foreach ($module in $modules) {
    $file = Join-Path $Out ($module + '.c')
    if (-not $Refresh -and (Test-Path $file -PathType Leaf) -and (Get-Item $file).Length -gt 0 `
            -and (Get-Item $file).LastWriteTimeUtc -ge $stamp) {
        Write-Host ("corpus: skip {0} (up to date, {1} functions)" -f $module, $counts[$module])
        $skipped++
        continue
    }
    # A killed run leaves <module>.c.tmp: it is not a corpus file, drop it before dumping.
    $partial = $file + '.tmp'
    if (Test-Path $partial) { Remove-Item -Force $partial -ErrorAction SilentlyContinue }
    $work += [pscustomobject]@{ Module = $module; Functions = $counts[$module]; Log = $null; Err = $null }
}
Write-Host ("corpus: {0} module(s) to dump, {1} skipped, output {2}" -f $work.Count, $skipped, $Out)

# --- Ghidra project copies ------------------------------------------------------------------
$slots = [Math]::Max(1, [Math]::Min($Jobs, [Math]::Max(1, $work.Count)))
$projects = @($Project)
if ($slots -gt 1) {
    $copyRoot = Join-Path $Root 'build/ghidra-jobs'
    if (Test-Path $copyRoot) {
        for ($attempt = 0; $attempt -lt 5 -and (Test-Path $copyRoot); $attempt++) {
            if ($attempt -gt 0) { Start-Sleep -Milliseconds 500 }
            try { Remove-Item -Recurse -Force $copyRoot -ErrorAction Stop } catch { }
        }
        if (Test-Path $copyRoot) {
            throw "ghidra-corpus: $copyRoot is still locked by a Ghidra run; close it or delete it"
        }
    }
    New-Item -ItemType Directory -Force $copyRoot | Out-Null
    $projectSize = Get-DirectorySize $Project
    $drive = New-Object IO.DriveInfo ([IO.Path]::GetPathRoot($Root))
    $needed = $projectSize * $slots
    Write-Host ("corpus: {0} project copies of {1} each ({2} asked), {3} free on {4}" -f `
            $slots, (Format-Size $projectSize), (Format-Size $needed), `
            (Format-Size $drive.AvailableFreeSpace), $drive.Name)
    if ($drive.AvailableFreeSpace -lt ($needed + 1GB)) {
        throw "ghidra-corpus: not enough free space for $slots project copies"
    }
    $projects = @()
    for ($index = 0; $index -lt $slots; $index++) {
        $copy = Join-Path $copyRoot ("job{0}" -f $index)
        robocopy $Project $copy /E /NFL /NDL /NJH /NJS /NP | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "ghidra-corpus: robocopy failed for $copy" }
        # robocopy reports 1 for "files copied": it must not become the script's exit code.
        $global:LASTEXITCODE = 0
        $projects += $copy
    }
}

# --- dump in parallel -----------------------------------------------------------------------
$logRoot = Join-Path $Out 'logs'
New-Item -ItemType Directory -Force $logRoot | Out-Null
$queue = [System.Collections.Generic.Queue[object]]::new()
foreach ($item in $work) { $queue.Enqueue($item) }
$running = @{}
$results = @()
$scriptPath = Join-Path $Root 're/ghidra_scripts'
$started = Get-Date
try {
    while ($queue.Count -gt 0 -or $running.Count -gt 0) {
        while ($running.Count -lt $slots -and $queue.Count -gt 0) {
            $free = 0
            while ($running.ContainsKey($free)) { $free++ }
            $item = $queue.Dequeue()
            $outFile = Join-Path $Out ($item.Module + '.c')
            $log = Join-Path $logRoot ($item.Module + '.log')
            $err = Join-Path $logRoot ($item.Module + '.err.log')
            $item.Log = $log
            $item.Err = $err
            $arguments = @($projects[$free], 'dq9', '-process', 'arm9.bin', '-noanalysis',
                '-readOnly', '-scriptPath', (Quote-Argument $scriptPath), '-postScript',
                'DumpCorpus.java', (Quote-Argument $outFile), (Quote-Argument $Map), $item.Module,
                $Timeout) | ForEach-Object { Quote-Argument ([string] $_) }
            Write-Host ("corpus: start {0} ({1} functions) with {2}" -f $item.Module,
                $item.Functions, $projects[$free])
            $process = Start-Process -FilePath $Headless -ArgumentList ($arguments -join ' ') `
                -RedirectStandardOutput $log -RedirectStandardError $err -NoNewWindow -PassThru
            $running[$free] = [pscustomobject]@{
                Item = $item; Process = $process; Started = Get-Date; Project = $projects[$free]
            }
        }
        Start-Sleep -Milliseconds 500
        foreach ($slot in @($running.Keys)) {
            $job = $running[$slot]
            $exited = $job.Process.HasExited
            $elapsed = ((Get-Date) - $job.Started).TotalSeconds
            if (-not $exited -and $elapsed -gt $ModuleTimeout) {
                Write-Host ("corpus: timeout {0} after {1:N0} s, killed" -f $job.Item.Module, $elapsed)
                try { $job.Process.Kill() } catch { }
                $exited = $true
            }
            if (-not $exited) { continue }
            # A finished process can still refuse ExitCode for a moment; the log decides success.
            $code = -1
            try { $code = $job.Process.ExitCode } catch { }
            # Start-Process keeps the redirection handles open until the Process is disposed:
            # the log is unreadable before that (IOException "in use by another process").
            try { $job.Process.Dispose() } catch { }
            $summary = $null
            $failed = 0
            if ((Test-Path $job.Item.Log) -and (Test-Path $job.Item.Err)) {
                $lines = (Read-TextLines $job.Item.Log) + (Read-TextLines $job.Item.Err)
                foreach ($line in $lines) {
                    if ($line -notmatch 'DumpCorpus\.java> ') { continue }
                    $text = ($line -replace '^.*?DumpCorpus\.java> ', '') -replace ' \(GhidraScript\)\s*$', ''
                    if ($text -match '^corpus: done ') { $summary = $text }
                    if ($text -match '^corpus: failed ') { $failed++ }
                }
            }
            if ($summary) {
                Write-Host ("{0} ({1:N1} s wall)" -f $summary, $elapsed)
                $results += [pscustomobject]@{ Module = $job.Item.Module; Summary = $summary
                    Wall = $elapsed; Failed = $failed }
            } else {
                $temporary = (Join-Path $Out ($job.Item.Module + '.c')) + '.tmp'
                if (Test-Path $temporary) { Remove-Item -Force $temporary }
                Write-Host ("corpus: FAILED {0} (exit {1}, {2:N1} s): see {3}" -f `
                        $job.Item.Module, $code, $elapsed, $job.Item.Log)
                $results += [pscustomobject]@{ Module = $job.Item.Module; Summary = 'FAILED'
                    Wall = $elapsed; Failed = $job.Item.Functions }
            }
            $running.Remove($slot)
        }
    }
} catch {
    Write-Host ("corpus: ERROR " + $_.Exception.Message)
    throw
} finally {
    foreach ($slot in @($running.Keys)) {
        $process = $running[$slot].Process
        try { $process.Kill() } catch { }
        try { [void] $process.WaitForExit(15000) } catch { }
        try {
            if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction Stop }
        } catch { }
    }
    $copyRoot = Join-Path $Root 'build/ghidra-jobs'
    if ($slots -gt 1 -and (Test-Path $copyRoot)) {
        # A killed JVM may still hold its project file for a moment: retry, then leave it.
        for ($attempt = 0; $attempt -lt 10 -and (Test-Path $copyRoot); $attempt++) {
            if ($attempt -gt 0) { Start-Sleep -Milliseconds 500 }
            try { Remove-Item -Recurse -Force $copyRoot -ErrorAction Stop } catch { }
        }
        if (Test-Path $copyRoot) {
            [Console]::Error.WriteLine("corpus: could not delete $copyRoot; delete it by hand")
        } else {
            Write-Host 'corpus: project copies deleted'
        }
    }
}
$total = ((Get-Date) - $started).TotalSeconds
$failedModules = @($results | Where-Object { $_.Summary -eq 'FAILED' })
if ($failedModules.Count -gt 0) {
    throw ("ghidra-corpus: {0} module(s) failed: {1}" -f $failedModules.Count,
        (($failedModules | ForEach-Object { $_.Module }) -join ', '))
}

# --- indexes --------------------------------------------------------------------------------
# Name -> address, from both maps: the corpus body carries the program's names (symbols.txt
# wins where it has one), the headers carry functions.txt names. An ambiguous name is dropped
# rather than resolved to the wrong function.
$byName = [System.Collections.Generic.Dictionary[string, string]]::new([StringComparer]::Ordinal)
$ambiguous = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($source in @(@{ Path = $Symbols; HasCategory = $false }, @{ Path = $Map; HasCategory = $true })) {
    if (-not (Test-Path $source.Path)) { continue }
    foreach ($line in [IO.File]::ReadAllLines($source.Path)) {
        $text = $line.Trim()
        if ($text -eq '' -or $text.StartsWith('#')) { continue }
        $parts = $text -split '\s+'
        $address = $parts[0]
        $name = if ($source.HasCategory) { if ($parts.Count -lt 4) { $null } else { $parts[3] } }
            else { if ($parts.Count -lt 2) { $null } else { $parts[1] } }
        if (-not $name) { continue }
        if ($byName.ContainsKey($name)) {
            if ($byName[$name] -ne $address) { $ambiguous.Add($name) | Out-Null }
            continue
        }
        $byName[$name] = $address
    }
}

# Address value -> qualified addresses. A call the decompiler prints as `func_0x0218b5b0` or
# `FUN_02000298` is a call whose target it could not name (another address space, or a function
# outside re/functions.txt): resolve it when exactly one listed function starts there, keep the
# name when several modules overlap on that address (0x021536e0 is the base of ov_000..ov_006).
$byValue = [System.Collections.Generic.Dictionary[long, object]]::new()
foreach ($line in $mapLines) {
    $text = $line.Trim()
    if ($text -eq '' -or $text.StartsWith('#')) { continue }
    $parts = $text -split '\s+'
    if ($parts.Count -lt 4) { continue }
    $address = $parts[0]
    $value = [Convert]::ToInt64($address.Substring($address.Length - 8), 16)
    if ($byValue.ContainsKey($value)) {
        $byValue[$value].Addresses.Add($address) | Out-Null
    } else {
        $byValue[$value] = [pscustomobject]@{
            Addresses = [System.Collections.Generic.List[string]]::new()
        }
        $byValue[$value].Addresses.Add($address) | Out-Null
    }
}

$indexLines = [System.Collections.Generic.List[string]]::new()
$callEdges = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$failureLines = [System.Collections.Generic.List[string]]::new()
$corpusModules = @($allModules | Where-Object { Test-Path (Join-Path $Out ($_ + '.c')) })
foreach ($module in $corpusModules) {
    $fileName = $module + '.c'
    $lines = [IO.File]::ReadAllLines((Join-Path $Out $fileName))
    $current = $null
    # A decompiled block starts with the header, then blank lines, `/* WARNING */` comments and
    # the C signature (possibly several lines), then the body's opening brace alone on its line.
    # The signature is a declaration, not a call: counting it made functions call themselves.
    $inPreamble = $false
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        $header = $HeaderRegex.Match($line)
        if ($header.Success) {
            $current = [pscustomobject]@{ Address = $header.Groups['addr'].Value
                Name = $header.Groups['name'].Value }
            $indexLines.Add(('{0} {1} {2}:{3}' -f $current.Address, $current.Name, $fileName, ($i + 1)))
            $inPreamble = $true
            continue
        }
        if (-not $current) { continue }
        if ($line.StartsWith($FailurePrefix)) {
            $failureLines.Add(('{0} {1} {2} {3}' -f $current.Address, $current.Name, $fileName,
                $line.Substring($FailurePrefix.Length)))
            $inPreamble = $false
            continue
        }
        if ($inPreamble) {
            if ($line.Trim() -eq '{') { $inPreamble = $false }
            continue
        }
        # Decompiler comments (`/* WARNING: Removing unreachable block (ram,0x...) */`) look like
        # a call to the regex; they are not code.
        $trimmed = $line.Trim()
        if ($trimmed.StartsWith('/*') -or $trimmed.StartsWith('//')) { continue }
        if ($line.IndexOf('(') -lt 0) { continue }
        foreach ($match in $CallRegex.Matches($line)) {
            $name = $match.Groups['name'].Value
            if ($NotCallees.Contains($name) -or $ambiguous.Contains($name)) { continue }
            $callee = $null
            if ($byName.ContainsKey($name)) {
                $callee = $byName[$name]
            } elseif ($name -match '^(?:func_0x|FUN_)(?:ov_\d+__)?(?<hex>[0-9a-f]{1,8})$') {
                $value = [Convert]::ToInt64($Matches['hex'], 16)
                if ($byValue.ContainsKey($value) -and $byValue[$value].Addresses.Count -eq 1) {
                    $callee = $byValue[$value].Addresses[0]
                }
            } elseif ($name -cmatch '^[A-Z][A-Z0-9_]+$') {
                # An all-caps token outside both maps is a decompiler intrinsic (SBORROW4,
                # CONCAT44, CARRY4, LZCOUNT), not a function.
                continue
            }
            if (-not $callee) { $callee = $name }
            $callEdges.Add(('{0} {1}' -f $current.Address, $callee)) | Out-Null
        }
    }
}
# LF endings, like the corpus itself: `rg`/`Select-String` line anchors then behave the same
# on every platform, and `grep -c` counts read cleanly.
function Write-Lines([string] $Path, [string[]] $Lines) {
    $text = if ($Lines.Count -eq 0) { '' } else { ($Lines -join "`n") + "`n" }
    $temporary = $Path + '.tmp'
    [IO.File]::WriteAllText($temporary, $text, (New-Object Text.UTF8Encoding($false)))
    Move-Item -Force $temporary $Path
}

Write-Lines (Join-Path $Out 'index.txt') ([string[]] $indexLines)
$sorted = [string[]] @($callEdges)
[Array]::Sort($sorted, [StringComparer]::Ordinal)
Write-Lines (Join-Path $Out 'calls.txt') $sorted
Write-Lines (Join-Path $Out 'failures.txt') ([string[]] $failureLines)

$functions = $indexLines.Count
$failedFunctions = $failureLines.Count
Write-Host ('corpus: {0} functions in {1} module(s), {2} calls, {3} failed, {4} total, {5} wall, {6} on disk' -f `
        $functions, $corpusModules.Count, $sorted.Count, $failedFunctions,
        (Format-Size ((Get-ChildItem -Path $Out -Filter '*.c' -File |
            Measure-Object -Property Length -Sum).Sum)), ('{0:N0} s' -f $total),
        (Format-Size (Get-DirectorySize $Out)))
if ($failedFunctions -gt 0) {
    Write-Host ("corpus: failures listed in {0}" -f (Join-Path $Out 'failures.txt'))
}
