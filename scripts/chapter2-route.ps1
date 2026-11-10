<#
.SYNOPSIS
  Plays the chapter 2 route: a new game walked by the headless pilot with the real battle
  engine, from the opening to wherever the story stops, and records what happened.

.DESCRIPTION
  Runs `dq9-app --new-game --fight --seed N --headless --auto --pilot` as a direct child process
  (not through Start-Process: its redirected result is not readable), so the script knows the
  child's PID, waits on that very process and reads its real exit code. It never stops a process
  by name: another worker's dq9-app may be running. On the timeout it stops only the child it
  started.

  With -Route EVENT the run is the route mode (`--route EVENT`, `app/auto/route.rs`): the field's
  monsters roam, the pilot rests at the inn and fights for levels after a defeat, and the run
  stops when that event ends (2750: the end of the Hexagoon's aftermath in 2.6). -Steps raises
  the step limit (60 steps per second of game time; the default of the binary is 200000).

  Files under -OutDir (default build/evidence/j4-chapter2-route/), stem -Name:
    <Name>.out, <Name>.err     journal ("stopped: ..." and the story tracks) and status lines
    <Name>-checkpoints.jsonl   the story state at every checkpoint
    <Name>-inputs.jsonl        the pad of every fixed step (replay with --replay)
    <Name>.exit.json           pid, exit code (also in hex), wall seconds, command line

  Needs the ROM (DQ9_ROM or rom/*.nds of the main checkout) and a release build of dq9-app
  (CARGO_TARGET_DIR, else target/): `cargo build -p dq9-app --release`.

.EXAMPLE
  powershell -File scripts/chapter2-route.ps1 -Name run13
  powershell -File scripts/chapter2-route.ps1 -Name route1 -Route 2750 -Steps 6000000
  powershell -File scripts/chapter2-route.ps1 -Name from-1110 -Extra '--map 1110 --chapter 2 --step 2 --substep 1'
#>
param(
    [string]$Name = 'route',
    [int]$Seed = 7,
    [string]$OutDir = 'build/evidence/j4-chapter2-route',
    [string]$Exe = '',
    # Replaces `--new-game` when given: options that start the run elsewhere, one string
    # (`--map M01 --chapter 2 --step 2 --substep 3`).
    [string]$Extra = '',
    # The route mode: the event whose end stops the run (0: the story pilot alone).
    [int]$Route = 0,
    # The step limit of the run (0: the binary's default).
    [long]$Steps = 0,
    [int]$TimeoutMinutes = 90
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

if (-not $Exe) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $Exe = Join-Path $target 'release/dq9-app.exe'
}
if (-not (Test-Path $Exe)) { throw "no release build at $Exe (cargo build -p dq9-app --release)" }
if (-not $env:DQ9_ROM -and -not (Get-ChildItem rom/*.nds -ErrorAction SilentlyContinue)) {
    throw 'no ROM: set DQ9_ROM to the .nds image'
}
New-Item -ItemType Directory -Force $OutDir | Out-Null

$arguments = @()
if (-not $Extra) { $arguments += '--new-game' } else { $arguments += ($Extra -split '\s+' | Where-Object { $_ }) }
$arguments += @(
    '--fight', '--seed', "$Seed", '--headless', '--auto', '--pilot', '--mute',
    '--checkpoints', "$OutDir/$Name-checkpoints.jsonl"
)
if ($Route -gt 0) {
    # A grind is millions of steps: the pad trace would be hundreds of megabytes, and a replay
    # does not turn the roaming on.
    $arguments += @('--route', "$Route")
} else {
    $arguments += @('--trace-inputs', "$OutDir/$Name-inputs.jsonl")
}
if ($Steps -gt 0) { $arguments += @('--headless-steps', "$Steps") }

$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = (Resolve-Path $Exe).Path
$info.Arguments = ($arguments | ForEach-Object { if ($_ -match '\s') { "`"$_`"" } else { $_ } }) -join ' '
$info.UseShellExecute = $false
$info.RedirectStandardOutput = $true
$info.RedirectStandardError = $true
$info.CreateNoWindow = $true

$clock = [System.Diagnostics.Stopwatch]::StartNew()
$child = [System.Diagnostics.Process]::Start($info)
"pid $($child.Id)"
# Both pipes are drained while the child runs: a full pipe would block it.
$stdout = $child.StandardOutput.ReadToEndAsync()
$stderr = $child.StandardError.ReadToEndAsync()
$finished = $child.WaitForExit($TimeoutMinutes * 60 * 1000)
if (-not $finished) {
    "timeout after $TimeoutMinutes min: stopping pid $($child.Id)"
    $child.Kill($true)
    $child.WaitForExit()
}
$child.WaitForExit()
$clock.Stop()
[System.IO.File]::WriteAllText("$OutDir/$Name.out", $stdout.GetAwaiter().GetResult())
[System.IO.File]::WriteAllText("$OutDir/$Name.err", $stderr.GetAwaiter().GetResult())

$code = $child.ExitCode
$summary = [ordered]@{
    name       = $Name
    pid        = $child.Id
    exit_code  = $code
    exit_hex   = ('0x{0:X8}' -f ($code -band 0xffffffff))
    timed_out  = (-not $finished)
    seconds    = [math]::Round($clock.Elapsed.TotalSeconds, 1)
    command    = "$($info.FileName) $($info.Arguments)"
}
$summary | ConvertTo-Json | Set-Content "$OutDir/$Name.exit.json"
"exit code $code ($($summary.exit_hex)) after $($summary.seconds) s"
Select-String -Path "$OutDir/$Name.out" -Pattern '^stopped:|^track 0:|scripted battle' |
    ForEach-Object { $_.Line }
exit $(if ($finished) { $code } else { 124 })
