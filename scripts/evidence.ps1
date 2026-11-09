# Writes the evidence manifest of a batch: build/evidence/<batch>/manifest.json lists every file
# of the batch directory with its size and SHA-256, plus the commit it was produced from.
# Docs cite `build/evidence/<batch>/<file>` and the first 12 characters of its sha256;
# scripts/prune-build.ps1 never deletes a file listed in a manifest.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/evidence.ps1 -Batch j4-battle
#   powershell -ExecutionPolicy Bypass -File scripts/evidence.ps1 -Batch j4-battle `
#       -Command 'cargo run -p dq9-headless -- ...' -Note 'battle 4135, loot differs (random)'
#
# The batch directory is created when missing; an empty directory is refused (put the evidence
# files there first). The manifest (UTF-8 without BOM, files sorted by path) has the fields
# batch, commit (HEAD SHA), dirty (uncommitted changes), created (ISO 8601), command,
# note and files: [{ path (relative to the batch directory, `/` separator), bytes, sha256 }].
param(
    [Parameter(Mandatory = $true)] [string] $Batch,
    [string] $Command = '',
    [string] $Note = ''
)
$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot
if ($Batch -notmatch '^[A-Za-z0-9._-]+$' -or $Batch -eq '.' -or $Batch -eq '..') {
    throw "invalid batch name '$Batch' (letters, digits, '.', '_', '-')"
}
$BatchDir = Join-Path (Join-Path (Join-Path $Repo 'build') 'evidence') $Batch
if (-not (Test-Path -LiteralPath $BatchDir -PathType Container)) {
    New-Item -ItemType Directory -Path $BatchDir | Out-Null
    Write-Output "created $BatchDir"
}
$BatchDir = (Resolve-Path -LiteralPath $BatchDir).Path
$ManifestPath = Join-Path $BatchDir 'manifest.json'

$Commit = (& git -C $Repo rev-parse HEAD)
if ($LASTEXITCODE -ne 0) { throw 'git rev-parse HEAD failed' }
$Status = @(& git -C $Repo status --porcelain)
if ($LASTEXITCODE -ne 0) { throw 'git status failed' }
$Dirty = $Status.Count -gt 0

$Files = @(Get-ChildItem -LiteralPath $BatchDir -Recurse -File |
    Where-Object { $_.FullName -ne $ManifestPath } |
    ForEach-Object {
        [pscustomobject][ordered]@{
            path = $_.FullName.Substring($BatchDir.Length).TrimStart('\', '/').Replace('\', '/')
            bytes = $_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    } | Sort-Object { $_.path } -CaseSensitive)
if ($Files.Count -eq 0) {
    Write-Output "no evidence in $BatchDir; put the files there, then rerun"
    exit 1
}

$Manifest = [ordered]@{
    batch = $Batch
    commit = "$Commit".Trim()
    dirty = $Dirty
    created = (Get-Date).ToString('o')
    command = $Command
    note = $Note
    files = $Files
}
$Json = ConvertTo-Json -InputObject $Manifest -Depth 5
[System.IO.File]::WriteAllText($ManifestPath, $Json + "`n", (New-Object System.Text.UTF8Encoding($false)))

Write-Output ("manifest {0}: {1} file(s), commit {2}{3}" -f `
    $ManifestPath, $Files.Count, $Manifest.commit.Substring(0, 12), $(if ($Dirty) { ' (dirty)' } else { '' }))
foreach ($File in $Files) {
    Write-Output ("  {0}  {1,10}  {2}" -f $File.sha256.Substring(0, 12), $File.bytes, $File.path)
}
