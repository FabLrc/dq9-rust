# Resolves a European address or function name to its source in the USA decompilation
# (https://github.com/DQIX/dqix-decomp, Unlicense). The checkout is not part of this
# repository: it lives under tools/dqix-decomp or is passed with -Decomp.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250
#   powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 GameState::SetDayTimer
#   powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 ov_017::0218b5a0
#   powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 SetDayTimer -List
#   powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250 -Context 80
#
# The European function comes from re/functions.txt (largest start <= address < next
# start). Its name is looked up in config/usa/**/symbols.txt to find the USA address, and
# the source file comes from that module's delinks.txt. When the symbol has no delinked
# source, src/**/*.cpp is searched; when nothing defines it, the command fails instead of
# printing a guess.
param(
    [Parameter(Position = 0)] [string] $Query,
    [string] $Decomp,
    [int] $Context = 40,
    [switch] $List
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

function Fail([string] $Message) {
    [Console]::Error.WriteLine("decomp: $Message")
    exit 1
}

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

# Minimal Itanium demangler for the symbol shapes used by dqix-decomp: _Z<len><name>,
# _ZN<...>E, constructors and destructors. Anything else is returned unchanged.
function Convert-MangledName([string] $Name) {
    if (-not $Name.StartsWith('_Z')) { return $Name }
    $rest = $Name.Substring(2)
    $nested = $false
    if ($rest.StartsWith('N')) {
        $nested = $true
        $rest = $rest.Substring(1)
        while ($rest.Length -gt 0 -and 'KVrR'.Contains([string] $rest[0])) {
            $rest = $rest.Substring(1)
        }
    }
    $parts = @()
    while ($rest.Length -gt 0 -and [char]::IsDigit($rest[0])) {
        $digits = 0
        while ($digits -lt $rest.Length -and [char]::IsDigit($rest[$digits])) { $digits++ }
        $length = [int] $rest.Substring(0, $digits)
        if ($length -le 0 -or $digits + $length -gt $rest.Length) { break }
        $parts += $rest.Substring($digits, $length)
        $rest = $rest.Substring($digits + $length)
        if (-not $nested -or $rest.StartsWith('E')) { break }
    }
    if ($parts.Count -eq 0) { return $Name }
    $last = $parts[$parts.Count - 1]
    if ($last -match '^C[123]$') {
        if ($parts.Count -ge 2) { $parts[$parts.Count - 1] = $parts[$parts.Count - 2] }
    } elseif ($last -match '^D[012]$') {
        if ($parts.Count -ge 2) { $parts[$parts.Count - 1] = '~' + $parts[$parts.Count - 2] }
    }
    return ($parts -join '::')
}

# European ARM9 address -> USA one, inverse of usa_to_eur in crates/dq9re/src/coverage.rs.
function ConvertTo-Usa([uint32] $Address) {
    if ($Address -ge 0x0200F3C0 -and $Address -lt 0x0200FB30) { return [uint32] ($Address - 8) }
    if ($Address -ge 0x0200FB30 -and $Address -lt 0x020EEBE0) { return [uint32] ($Address - 0x10) }
    return $Address
}

# Module directory of the decompilation checkout: arm9, itcm or overlays/ovNNN.
function Get-ModuleDir([string] $Module) {
    if ($Module -eq 'arm9') { return $Config }
    if ($Module -eq 'itcm') { return (Join-Path $Config 'itcm') }
    return (Join-Path $Config ('overlays/' + $Module.Replace('_', '')))
}

function Format-Address([object] $Function) {
    if ($Function.Module -eq 'arm9' -or $Function.Module -eq 'itcm') {
        return ('0x{0:x8}' -f $Function.Addr)
    }
    return ('{0}::{1:x8}' -f $Function.Module, $Function.Addr)
}

function Resolve-ByAddress([string] $Text) {
    $match = [regex]::Match($Text, '^ov_(\d{3})::(?:0x)?([0-9a-fA-F]+)$')
    if ($match.Success) {
        $module = 'ov_' + $match.Groups[1].Value
        $address = [Convert]::ToUInt32($match.Groups[2].Value, 16)
    } else {
        $match = [regex]::Match($Text, '^(?:0x|FUN_)([0-9a-fA-F]+)$')
        if (-not $match.Success) { return $null }
        $address = [Convert]::ToUInt32($match.Groups[1].Value, 16)
        if ($address -ge 0x01FF8000 -and $address -lt 0x02000000) { $module = 'itcm' }
        else { $module = 'arm9' }
    }

    $pool = @($Functions | Where-Object { $_.Module -eq $module } | Sort-Object Addr)
    if ($pool.Count -eq 0) { Fail "unknown module '$module' in $FunctionsPath" }
    $index = -1
    for ($i = 0; $i -lt $pool.Count; $i++) {
        if ($pool[$i].Addr -le $address) { $index = $i } else { break }
    }
    if ($index -lt 0) {
        Fail ('0x{0:x8} is before the first function of {1}' -f $address, $module)
    }
    $function = $pool[$index]
    $next = if ($index + 1 -lt $pool.Count) { $pool[$index + 1].Addr } else { $function.Addr + $function.Size }
    if ($address -ge $next) {
        $hint = ''
        if ($module -eq 'arm9') { $hint = ' Overlay addresses need the ov_NNN:: prefix.' }
        Fail ('0x{0:x8} is not inside a function of {1} (last function: {2} at 0x{3:x8}, ends 0x{4:x8}).{5}' -f `
                $address, $module, $function.Name, $function.Addr, $next, $hint)
    }
    return @($function)
}

function Resolve-ByName([string] $Text) {
    # Exact symbols first: a qualified name must not drag in SetDayTimerRunning.
    $exact = @($Functions | Where-Object {
        $_.Name.Equals($Text, [StringComparison]::OrdinalIgnoreCase)
    })
    if ($exact.Count -gt 0) { return $exact }
    $qualified = @()
    $methods = @()
    foreach ($function in $Functions) {
        $name = Convert-MangledName $function.Name
        if ($name.Equals($Text, [StringComparison]::OrdinalIgnoreCase)) { $qualified += $function }
        $method = if ($name.Contains('::')) { $name.Substring($name.LastIndexOf('::') + 2) } else { $name }
        if ($method.Equals($Text, [StringComparison]::OrdinalIgnoreCase)) { $methods += $function }
    }
    if ($qualified.Count -gt 0) { return $qualified }
    if ($methods.Count -gt 0) { return $methods }

    $direct = @($Functions | Where-Object {
        $_.Name.IndexOf($Text, [StringComparison]::OrdinalIgnoreCase) -ge 0
    })
    if ($direct.Count -gt 0) { return $direct }
    return @($Functions | Where-Object {
        (Convert-MangledName $_.Name).IndexOf($Text, [StringComparison]::OrdinalIgnoreCase) -ge 0
    })
}

# Symbol line and USA address of one function in its module's dsd symbols.txt.
function Get-SymbolInfo([object] $Function) {
    $dir = Get-ModuleDir $Function.Module
    $symbols = Join-Path $dir 'symbols.txt'
    if (-not (Test-Path $symbols)) { Fail "missing $symbols" }
    $usa = if ($Function.Module -eq 'arm9') { ConvertTo-Usa $Function.Addr } else { $Function.Addr }
    $byAddress = $null
    $byName = $null
    foreach ($line in [IO.File]::ReadLines($symbols)) {
        if ($line.IndexOf('kind:function', [StringComparison]::Ordinal) -lt 0) { continue }
        $match = [regex]::Match($line, 'addr:0x([0-9a-fA-F]+)')
        if (-not $match.Success) { continue }
        $address = [Convert]::ToUInt32($match.Groups[1].Value, 16)
        if ($address -eq $usa) {
            $byAddress = [pscustomobject]@{ Symbol = $line; Address = $address; Dir = $dir }
            break
        }
        $name = ($line -split '\s+', 2)[0]
        if (-not $byName -and $name -eq $Function.Name) {
            $byName = [pscustomobject]@{ Symbol = $line; Address = $address; Dir = $dir }
        }
    }
    $info = if ($byAddress) { $byAddress } else { $byName }
    if (-not $info) {
        Fail ('{0} ({1}) is not a function symbol in {2}' -f (Format-Address $Function), $Function.Name, $symbols)
    }
    return $info
}

# Source file that owns a USA address, from the module's delinks.txt (dsd layout).
function Get-DelinkedSource([string] $ModuleDir, [uint32] $Address) {
    $delinks = Join-Path $ModuleDir 'delinks.txt'
    if (-not (Test-Path $delinks)) { return $null }
    $current = $null
    foreach ($line in [IO.File]::ReadLines($delinks)) {
        if ($line.Length -eq 0) { continue }
        if (-not [char]::IsWhiteSpace($line[0])) {
            $trimmed = $line.Trim()
            if ($trimmed.EndsWith(':')) { $current = $trimmed.Substring(0, $trimmed.Length - 1) }
            continue
        }
        if (-not $current) { continue }
        $match = [regex]::Match($line, '\.(?:text|init)\s+start:0x([0-9a-fA-F]+)\s+end:0x([0-9a-fA-F]+)')
        if (-not $match.Success) { continue }
        $start = [Convert]::ToUInt32($match.Groups[1].Value, 16)
        $end = [Convert]::ToUInt32($match.Groups[2].Value, 16)
        if ($Address -ge $start -and $Address -lt $end) {
            return [pscustomobject]@{ Path = $current; Start = $start; End = $end }
        }
    }
    return $null
}

# 1-based line of a definition in a source file, 0 when the file does not define it.
# dqix-decomp writes definitions at column 0; calls are indented, so requiring column 0 and
# rejecting a first parenthesis before the name keeps calls such as switch (f(...)) out.
function Find-Definition([string] $Path, [string] $Needle) {
    $lines = [IO.File]::ReadAllLines($Path)
    for ($i = 0; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]
        $trimmed = $line.TrimStart()
        if ($trimmed.Length -eq 0 -or $trimmed.Length -ne $line.Length) { continue }
        if ($trimmed.StartsWith('//') -or $trimmed.StartsWith('*') -or
            $trimmed.StartsWith('/*') -or $trimmed.StartsWith('#')) { continue }
        if ($trimmed -match '^(if|for|while|switch|return|else|do)\b') { continue }
        $position = $line.IndexOf($Needle, [StringComparison]::Ordinal)
        if ($position -lt 0) { continue }
        $prefix = $line.Substring(0, $position)
        if ($prefix.Contains('(') -or $prefix.Contains('=') -or $prefix.TrimEnd().EndsWith('.')) { continue }
        if (-not $line.Substring($position + $Needle.Length).TrimStart().StartsWith('(')) { continue }
        if ($line.TrimEnd().EndsWith(';')) { continue }
        return $i + 1
    }
    return 0
}

# Source file and definition line, from delinks.txt first, then src/**/*.cpp.
function Get-Source([object] $Function, [object] $Symbol) {
    $qualified = Convert-MangledName $Function.Name
    $method = if ($qualified.Contains('::')) { $qualified.Substring($qualified.LastIndexOf('::') + 2) } else { $qualified }
    $needles = @($qualified)
    if ($method -ne $qualified) { $needles += $method }

    $delinked = Get-DelinkedSource $Symbol.Dir $Symbol.Address
    if ($delinked) {
        $path = Join-Path $DecompRoot $delinked.Path
        if (Test-Path $path) {
            foreach ($needle in $needles) {
                $lineNumber = Find-Definition $path $needle
                if ($lineNumber -gt 0) {
                    return [pscustomobject]@{ Path = (Resolve-Path $path).Path; Line = $lineNumber }
                }
            }
        }
    }

    foreach ($file in Get-ChildItem (Join-Path $DecompRoot 'src') -Recurse -Filter *.cpp -File) {
        $lineNumber = Find-Definition $file.FullName $qualified
        if ($lineNumber -gt 0) {
            return [pscustomobject]@{ Path = $file.FullName; Line = $lineNumber }
        }
    }
    if ($method -ne $qualified) {
        foreach ($file in Get-ChildItem (Join-Path $DecompRoot 'src') -Recurse -Filter *.cpp -File) {
            $lineNumber = Find-Definition $file.FullName $method
            if ($lineNumber -gt 0) {
                return [pscustomobject]@{ Path = $file.FullName; Line = $lineNumber }
            }
        }
    }

    $where = if ($delinked) { $delinked.Path } else { 'no delinks.txt entry' }
    Fail ('no source defines {0} ({1}, USA 0x{2:x8}, {3}) in the dqix-decomp checkout {4}' -f `
            $qualified, (Format-Address $Function), $Symbol.Address, $where, $DecompRoot)
}

function Write-FunctionSource([object] $Function) {
    $symbol = Get-SymbolInfo $Function
    $source = Get-Source $Function $symbol
    $qualified = Convert-MangledName $Function.Name
    Write-Output ('eu    {0}  0x{1:x}  {2}  {3}' -f (Format-Address $Function), $Function.Size, $Function.Category, $qualified)
    Write-Output ('usa   0x{0:x8}  {1}' -f $symbol.Address, $Function.Name)
    Write-Output ('src   {0}:{1}' -f $source.Path, $source.Line)

    $lines = [IO.File]::ReadAllLines($source.Path)
    $last = [Math]::Min($lines.Length, $source.Line - 1 + $Context)
    for ($i = $source.Line - 1; $i -lt $last; $i++) {
        Write-Output ('{0,6}: {1}' -f ($i + 1), $lines[$i])
    }
}

if ($Context -le 0) { Fail '-Context must be a positive number of lines' }

# --- decompilation checkout ---------------------------------------------------------------
if ($Decomp) {
    if (-not (Test-Path $Decomp)) { Fail "no decompilation checkout at $Decomp" }
    $DecompRoot = (Resolve-Path $Decomp).Path
} else {
    $candidates = @(Join-Path $Root 'tools/dqix-decomp')
    $main = Get-MainRoot
    if ($main -and $main -ne $Root) { $candidates += Join-Path $main 'tools/dqix-decomp' }
    $found = @($candidates | Where-Object { Test-Path (Join-Path $_ 'config/usa/arm9/symbols.txt') })
    if ($found.Count -eq 0) {
        Fail 'no dqix-decomp checkout: clone it under tools/dqix-decomp or pass -Decomp PATH'
    }
    $DecompRoot = (Resolve-Path $found[0]).Path
}
$Config = Join-Path $DecompRoot 'config/usa/arm9'
if (-not (Test-Path (Join-Path $Config 'symbols.txt'))) { Fail "not a dqix-decomp checkout: $DecompRoot" }

# --- European function list ---------------------------------------------------------------
$FunctionsPath = Join-Path $Root 're/functions.txt'
if (-not (Test-Path $FunctionsPath)) { Fail "missing $FunctionsPath" }
$functionPattern = [regex]'^(?:ov_(\d{3})::([0-9a-fA-F]+)|0x([0-9a-fA-F]+))\s+0x([0-9a-fA-F]+)\s+(\w+)\s+(\S+)\s*$'
$Functions = [System.Collections.Generic.List[object]]::new()
foreach ($line in [IO.File]::ReadLines($FunctionsPath)) {
    if ($line.Length -eq 0 -or $line[0] -eq '#') { continue }
    $match = $functionPattern.Match($line)
    if (-not $match.Success) { continue }
    if ($match.Groups[1].Success) {
        $module = 'ov_' + $match.Groups[1].Value
        $address = [Convert]::ToUInt32($match.Groups[2].Value, 16)
    } else {
        $address = [Convert]::ToUInt32($match.Groups[3].Value, 16)
        if ($address -ge 0x01FF8000 -and $address -lt 0x02000000) { $module = 'itcm' }
        else { $module = 'arm9' }
    }
    $Functions.Add([pscustomobject]@{
        Module = $module
        Addr = $address
        Size = [Convert]::ToUInt32($match.Groups[4].Value, 16)
        Category = $match.Groups[5].Value
        Name = $match.Groups[6].Value
    })
}
if ($Functions.Count -eq 0) { Fail "no function parsed from $FunctionsPath" }

# --- resolve ------------------------------------------------------------------------------
if (-not $Query) {
    if (-not $List) { Fail 'missing QUERY: pass an address (0x02010250, ov_017::0218b5a0) or a function name' }
    foreach ($function in $Functions) { Write-Output (Convert-MangledName $function.Name) }
    exit 0
}

$resolved = Resolve-ByAddress $Query
if (-not $resolved) { $resolved = Resolve-ByName $Query }
if ($resolved.Count -eq 0) {
    Fail "no function matches '$Query' in re/functions.txt"
}

if ($List) {
    foreach ($function in $resolved) {
        Write-Output ('{0}  {1}' -f (Format-Address $function), (Convert-MangledName $function.Name))
    }
    exit 0
}

if ($resolved.Count -gt 1) {
    [Console]::Error.WriteLine("decomp: $($resolved.Count) functions match '$Query'; use -List or a qualified name")
    foreach ($function in $resolved) {
        [Console]::Error.WriteLine(('  {0}  {1}' -f (Format-Address $function), (Convert-MangledName $function.Name)))
    }
    exit 2
}

Write-FunctionSource $resolved[0]
