# Installs the reverse-engineering toolchain into ./tools (git-ignored).
#   - Temurin JDK 25  (required by Ghidra 12.2)
#   - Ghidra 12.2     (static analysis / decompilation of ARM9 code + overlays)
#   - melonDS 1.1     (reference emulator, GDB stub for dynamic analysis)
# Every archive is verified against a pinned SHA-256 before extraction.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/setup-re-tools.ps1
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Root  = Split-Path -Parent $PSScriptRoot
$Tools = Join-Path $Root 'tools'
$Cache = Join-Path $Tools '.downloads'
New-Item -ItemType Directory -Force -Path $Cache | Out-Null

$Packages = @(
    @{
        Name   = 'jdk'
        Url    = 'https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.5%2B8/OpenJDK25U-jdk_x64_windows_hotspot_25.0.5_8.zip'
        Sha256 = '636bdc13dc649ddc7d5921c5c7d23cca6e44fd19d662b45cc9c02135a6c96c42'
        Marker = 'bin/java.exe'
    },
    @{
        Name   = 'ghidra'
        Url    = 'https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_12.2_build/ghidra_12.2_PUBLIC_20261016.zip'
        Sha256 = '2c2cd776db3c1f6a097ba418922378ac3649cadd5e844a6f5f088ae628a7be67'
        Marker = 'support/analyzeHeadless.bat'
    },
    @{
        Name   = 'melonds'
        Url    = 'https://github.com/melonDS-emu/melonDS/releases/download/1.1/melonDS-1.1-windows-x86_64.zip'
        Sha256 = '9f3f8a244103be20b5b657af5b0ed1b2a66bb20a7181476a6d294c9a53d4f8c8'
        Marker = 'melonDS.exe'
    }
)

foreach ($p in $Packages) {
    $dest = Join-Path $Tools $p.Name
    # The stamp records the archive hash, so changing a pin reinstalls that package.
    $stamp = Join-Path $dest '.installed-sha256'
    if ((Test-Path $stamp) -and (Get-Content $stamp) -eq $p.Sha256) {
        Write-Host "[ok]   $($p.Name) already installed"
        continue
    }
    $zip = Join-Path $Cache ([IO.Path]::GetFileName(([Uri]$p.Url).LocalPath))
    if (-not (Test-Path $zip)) {
        Write-Host "[get]  $($p.Url)"
        & curl.exe -sSfL -o $zip $p.Url
        if ($LASTEXITCODE -ne 0) { throw "download failed: $($p.Url)" }
    }
    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    if ($hash -ne $p.Sha256) {
        Remove-Item $zip
        throw "checksum mismatch for $zip (got $hash)"
    }

    # Extract into a staging dir, then flatten a single top-level folder.
    $stage = Join-Path $Cache "$($p.Name).stage"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Path $stage | Out-Null
    & tar.exe -xf $zip -C $stage
    if ($LASTEXITCODE -ne 0) { throw "extraction failed: $zip" }
    $entries = @(Get-ChildItem $stage)
    $src = if ($entries.Count -eq 1 -and $entries[0].PSIsContainer) { $entries[0].FullName } else { $stage }
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    Move-Item $src $dest
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    if (-not (Test-Path (Join-Path $dest $p.Marker))) { throw "$($p.Name): missing $($p.Marker) after extraction" }
    Set-Content -Path $stamp -Value $p.Sha256
    Write-Host "[ok]   $($p.Name) -> $dest"
}

# Point Ghidra at the bundled JDK so it never prompts for one.
$launch = Join-Path $Tools 'ghidra/support/launch.properties'
$jdk = (Join-Path $Tools 'jdk') -replace '\\', '/'
$line = "JAVA_HOME_OVERRIDE=$jdk"
$content = Get-Content $launch
if ($content -notcontains $line) {
    ($content | Where-Object { $_ -notmatch '^JAVA_HOME_OVERRIDE=' }) + $line | Set-Content $launch
}
Write-Host "Done. GHIDRA_INSTALL_DIR=$(Join-Path $Tools 'ghidra')"
