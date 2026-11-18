# Prints a function block from the decompilation corpus (scripts/ghidra-corpus.ps1), with no
# Ghidra run: the block is read from <decomp>/<module>.c through <decomp>/index.txt, so an
# answer costs milliseconds instead of the 8-30 s of an analyzeHeadless query.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 ov_017::0218c0f4
#        powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 Agent_Update
#        powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 0x020d91ec -Callers
#        powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 0x020d91ec -Callees
# An address inside a function prints the containing function, like Query.java decomp does.
# The corpus directory is -Dir, else DQ9_DECOMP, else <main checkout>/build/decomp: a linked
# worktree reads the main checkout's corpus (ROM-derived, never committed, never in a worktree).
# -Callers/-Callees print one line per related function (`<addr> <name> <file>:<line>`) from
# calls.txt; pass that address back to this script for its block.
param(
    [Parameter(Mandatory = $true, Position = 0)][string] $Target,
    [switch] $Callers,
    [switch] $Callees,
    [string] $Dir
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

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

if (-not $Dir) {
    if ($env:DQ9_DECOMP) { $Dir = $env:DQ9_DECOMP } else { $Dir = Join-Path (Get-MainRoot) 'build/decomp' }
}
$Dir = [IO.Path]::GetFullPath($Dir)
$indexPath = Join-Path $Dir 'index.txt'
$callsPath = Join-Path $Dir 'calls.txt'
if (-not (Test-Path $indexPath)) {
    throw "decomp-find: no corpus index at $indexPath; run scripts/ghidra-corpus.ps1"
}

$headerRegex = [regex]'^// (?<addr>0x[0-9a-f]+|ov_\d+::[0-9a-f]+) 0x(?<size>[0-9a-f]+) (?<name>\S+)\s*$'

# --- index: address -> name and file:line, name -> address, and the starts of each module ----
$byAddress = [System.Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
$byName = [System.Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
$duplicateNames = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$moduleStarts = @{}
$moduleEntries = @{}
foreach ($line in [IO.File]::ReadAllLines($indexPath)) {
    if ($line.Trim() -eq '') { continue }
    $parts = $line.Split(' ')
    if ($parts.Count -lt 3) { continue }
    $entry = [pscustomobject]@{ Address = $parts[0]; Name = $parts[1]; Location = $parts[2] }
    $byAddress[$entry.Address] = $entry
    if ($byName.ContainsKey($entry.Name)) {
        $duplicateNames.Add($entry.Name) | Out-Null
    } else {
        $byName[$entry.Name] = $entry
    }
    # index.txt is written in map order, which is ascending within a module.
    $module = if ($entry.Address.Contains('::')) {
        $entry.Address.Substring(0, $entry.Address.IndexOf('::'))
    } else {
        'arm9'
    }
    if (-not $moduleStarts.ContainsKey($module)) {
        $moduleStarts[$module] = [System.Collections.Generic.List[long]]::new()
        $moduleEntries[$module] = [System.Collections.Generic.List[object]]::new()
    }
    $offset = $entry.Address.Substring($entry.Address.LastIndexOf(':') + 1)
    $moduleStarts[$module].Add([Convert]::ToInt64($offset, 16))
    $moduleEntries[$module].Add($entry)
}
$startArrays = @{}
$entryArrays = @{}
foreach ($module in $moduleStarts.Keys) {
    $startArrays[$module] = $moduleStarts[$module].ToArray()
    $entryArrays[$module] = $moduleEntries[$module].ToArray()
}

# `re/symbols.txt` names are what the bodies use and what a worker knows ("Gpc2_LoadIndex"),
# while index.txt carries re/functions.txt names (mangled): accept them as aliases when the
# address they name is a corpus function.
$aliasByName = [System.Collections.Generic.Dictionary[string, string]]::new([StringComparer]::Ordinal)
$symbolsPath = Join-Path $Root 're/symbols.txt'
if (Test-Path $symbolsPath) {
    foreach ($line in [IO.File]::ReadAllLines($symbolsPath)) {
        $text = $line.Trim()
        if ($text -eq '' -or $text.StartsWith('#')) { continue }
        $parts = $text -split '\s+'
        if ($parts.Count -lt 2) { continue }
        if (-not $aliasByName.ContainsKey($parts[1])) { $aliasByName[$parts[1]] = $parts[0] }
    }
}

# --- resolve the target ---------------------------------------------------------------------
$query = $Target.Trim()
# Ghidra's own overlay syntax is `ov_017:0218c0f4`; the repository writes `ov_017::0218c0f4`.
if ($query -match '^(ov_\d+):([0-9a-fA-F]{1,8})$') { $query = $Matches[1] + '::' + $Matches[2] }
$addressForm = $false
$module = $null
$value = 0
if ($query -match '^(?<space>ov_\d+)::(?<offset>[0-9a-fA-F]+)$') {
    $module = $Matches['space']
    $value = [Convert]::ToInt64($Matches['offset'], 16)
    $query = $module + '::' + $Matches['offset'].ToLowerInvariant()
    $addressForm = $true
} elseif ($query -match '^0[xX](?<offset>[0-9a-fA-F]+)$') {
    $module = 'arm9'
    $value = [Convert]::ToInt64($Matches['offset'], 16)
    $query = '0x' + $Matches['offset'].ToLowerInvariant()
    $addressForm = $true
}

$entry = $null
$noPredecessor = $false
if ($byAddress.ContainsKey($query)) {
    $entry = $byAddress[$query]
} elseif ($byName.ContainsKey($query)) {
    $entry = $byName[$query]
} elseif ($aliasByName.ContainsKey($query) -and $byAddress.ContainsKey($aliasByName[$query])) {
    $entry = $byAddress[$aliasByName[$query]]
    [Console]::Error.WriteLine(('decomp-find: {0} is {1} in re/symbols.txt' -f $query, $entry.Address))
} elseif ($addressForm) {
    if (-not $startArrays.ContainsKey($module)) {
        throw "decomp-find: the corpus has no function of module $module; see $indexPath"
    }
    # Inside a function? Take the last start at or before the address (index.txt is sorted).
    $starts = [long[]] $startArrays[$module]
    $index = [Array]::BinarySearch($starts, [long] $value)
    if ($index -lt 0) { $index = (-bnot $index) - 1 }
    if ($index -ge 0) { $entry = $entryArrays[$module][$index] } else { $noPredecessor = $true }
}
if (-not $entry -and $noPredecessor) {
    throw (('decomp-find: no function of {0} starts at or before {1}; the corpus covers the game ' +
        'functions of re/functions.txt only') -f $module, $query)
}
if (-not $entry) {
    $needle = $query.ToLowerInvariant()
    $candidates = @($byAddress.Values | Where-Object { $_.Name.ToLowerInvariant().Contains($needle) } |
        Sort-Object -Property Address | Select-Object -First 10)
    if ($candidates.Count -eq 0) {
        # re/symbols.txt names are not in the index: match them, then resolve their address.
        $candidates = @($aliasByName.Keys | Where-Object { $_.ToLowerInvariant().Contains($needle) } |
            ForEach-Object { $alias = $aliasByName[$_]; if ($byAddress.ContainsKey($alias)) { $byAddress[$alias] } } |
            Sort-Object -Property Address -Unique | Select-Object -First 10)
    }
    if ($candidates.Count -eq 0) {
        throw "decomp-find: '$Target' is neither an address of the corpus nor part of a name; see $indexPath"
    }
    if ($candidates.Count -eq 1) {
        # One substring match is unambiguous: accept it (demangled names, mangled symbols).
        $entry = $candidates[0]
        [Console]::Error.WriteLine(('decomp-find: {0} matched {1} {2}' -f $query, $entry.Address,
                $entry.Name))
    } else {
        $listing = ($candidates | ForEach-Object { '  ' + $_.Address + ' ' + $_.Name + ' ' + $_.Location }) -join "`n"
        throw "decomp-find: '$Target' is ambiguous; did you mean:`n$listing"
    }
}
if ($duplicateNames.Contains($entry.Name)) {
    [Console]::Error.WriteLine("decomp-find: warning: several functions share the name $($entry.Name)")
}

$location = $entry.Location.Split(':')
$file = Join-Path $Dir $location[0]
$lineNumber = [int] $location[1]
if (-not (Test-Path $file)) { throw "decomp-find: $file is missing; rerun scripts/ghidra-corpus.ps1" }

# --- the block ------------------------------------------------------------------------------
$lines = [IO.File]::ReadAllLines($file)
if ($lineNumber -lt 1 -or $lineNumber -gt $lines.Count) {
    throw "decomp-find: $($entry.Location) is outside $file; the index is stale, rerun scripts/ghidra-corpus.ps1"
}
$header = $headerRegex.Match($lines[$lineNumber - 1])
if ($addressForm) {
    $start = [Convert]::ToInt64($entry.Address.Substring($entry.Address.LastIndexOf(':') + 1), 16)
    $size = if ($header.Success) { [Convert]::ToInt64($header.Groups['size'].Value, 16) } else { 0 }
    if ($value -lt $start -or $value -ge ($start + $size)) {
        throw ("decomp-find: no function of {0} contains the address {1}; nearest start is {2} {3}" -f `
                $module, $entry.Address, $entry.Address, $entry.Name)
    }
    if ($value -ne $start) {
        [Console]::Error.WriteLine(('decomp-find: {0} is {1} {2} +0x{3:x}' -f $query,
                $entry.Address, $entry.Name, ($value - $start)))
    }
}
for ($i = $lineNumber - 1; $i -lt $lines.Count; $i++) {
    if ($i -gt $lineNumber - 1 -and $headerRegex.IsMatch($lines[$i])) { break }
    Write-Output $lines[$i]
}

# --- relations ------------------------------------------------------------------------------
# An edge line is `<caller> <callee>`. Callers of X are the lines whose *second* field is X and
# whose first field is the related function; callees the other way round.
function Show-Relation([string] $Kind, [string] $Pattern, [int] $Field) {
    if (-not (Test-Path $callsPath)) {
        [Console]::Error.WriteLine("decomp-find: no $callsPath; rerun scripts/ghidra-corpus.ps1")
        return
    }
    # Select-String scans calls.txt natively; a PowerShell line loop cost ~1 s per call.
    $related = @(Select-String -Path $callsPath -Pattern $Pattern -CaseSensitive |
        ForEach-Object { $_.Line })
    if ($related.Count -eq 0) {
        Write-Output ''
        Write-Output ('# {0} of {1} {2}: none in calls.txt' -f $Kind, $entry.Address, $entry.Name)
        return
    }
    Write-Output ''
    Write-Output ('# {0} of {1} {2} ({3}):' -f $Kind, $entry.Address, $entry.Name, $related.Count)
    foreach ($edge in $related) {
        $address = $edge.Split(' ')[$Field]
        $known = if ($byAddress.ContainsKey($address)) { $byAddress[$address] } else { $null }
        if ($known) {
            Write-Output ('  ' + $known.Address + ' ' + $known.Name + ' ' + $known.Location)
        } else {
            Write-Output ('  ' + $address)
        }
    }
}

$escaped = [regex]::Escape($entry.Address)
if ($Callers) { Show-Relation 'callers' (' ' + $escaped + '$') 0 }
if ($Callees) { Show-Relation 'callees' ('^' + $escaped + ' ') 1 }
