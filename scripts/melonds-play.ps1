# Drives a portable melonDS with posted window messages, for the differential oracle
# (docs/re/melonds-state.md, § « Oracle différentiel »). No focus needed; do not resize the
# window (default scale 1, screens stacked).
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/melonds-play.ps1 -Fresh `
#       -Actions "waitstart,tap:127:377,wait:8,key:a,wait:3,...,save:start,shot:start"
#   powershell -ExecutionPolicy Bypass -File scripts/melonds-play.ps1 `
#       -LoadState build/melonds-oracle/observatory.ml1 -Actions "wait:3,loadstate,wait:2,play:120:2:20:pro"
#
# Actions (comma separated, executed in order):
#   wait:SECONDS              sleep
#   waitstart[:SECONDS]       wait until the title screen's orange START button is at (110, touch+160)
#   tap:X:Y                   touch at client coordinates (X, Y)
#   tapscreen:X:Y             post the touch without the menu-bar offset (raw)
#   key:NAME[:MS]             press a DS key (up, down, left, right, a, b, start) for MS (80)
#   hold:NAME[+NAME...]:SEC   hold one or more DS keys for SEC seconds
#   shot:NAME                 save the whole client area to NAME.png
#   save[:NAME]               File > Save state > 1, wait for the file, copy it to NAME.ml1
#   loadstate                 File > Load state > 1 (the state copied by -LoadState)
#   spam:SECONDS[:INTERVAL]   press A every INTERVAL seconds (2)
#   play:SECONDS:INTERVAL:EVERY:PREFIX
#                             press A every INTERVAL seconds and save a state every EVERY
#                             seconds to PREFIX1.ml1, PREFIX2.ml1...
#   autosave:SECONDS:INTERVAL:PREFIX
#   pause                     press the Pause hotkey (melonDS.toml HK_Pause)
#   frame:N                   press the Frame step hotkey N times (HK_FrameStep). melonDS 1.1
#                             advances about 0.5 frame per press under posted keys: measure the
#                             VBlank delta, this is not yet exact enough for the J2 criterion.
#   log                       print the client size
#
# The ROM comes from -Rom, else DQ9_ROM, else the only *.nds in rom/. The working directory is
# build/melonds-oracle (ignored by git): melonDS.exe, `portable`, a hard link to the ROM and
# dq9.sav. melonDS 1.1 maps no DS key by default: melonDS.toml is written with the mappings
# this script posts (arrows, A = X, B = Z, Start = Return).
param(
    [string] $Rom = $env:DQ9_ROM,
    [string] $Actions = '',
    [string] $Work = '',
    [string] $MelonDS = '',
    [switch] $Fresh,
    [switch] $Keep,
    [string] $LoadState = ''
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Work) { $Work = Join-Path $Root 'build/melonds-oracle' }
if (-not $MelonDS) { $MelonDS = Join-Path $Root 'tools/melonds/melonDS.exe' }
if (-not (Test-Path $MelonDS)) { throw "melonDS.exe not found at $MelonDS (run scripts/setup-re-tools.ps1)" }
if (-not $Rom) {
    $Rom = @(Get-ChildItem (Join-Path $Root 'rom') -Filter *.nds -File -ErrorAction SilentlyContinue)[0].FullName
}
if (-not $Rom -or -not (Test-Path $Rom)) { throw 'no ROM: pass -Rom or set DQ9_ROM' }

New-Item -ItemType Directory -Force $Work | Out-Null
Copy-Item $MelonDS $Work -Force
New-Item -ItemType File -Force (Join-Path $Work 'portable') | Out-Null
$Dst = Join-Path $Work 'dq9.nds'
if (-not (Test-Path $Dst)) {
    try { New-Item -ItemType HardLink -Path $Dst -Target $Rom | Out-Null }
    catch { Copy-Item $Rom $Dst }
}
if (-not (Test-Path (Join-Path $Work 'melonDS.toml'))) {
    @'
[Emu]
DirectBoot = true

[Instance0]
EnableCheats = false
SaveFilePath = ""
JoystickID = 0
SavestatePath = ""
CheatFilePath = ""

[Instance0.Keyboard]
Up = 16777235
Down = 16777237
Left = 16777234
Right = 16777236
A = 88
B = 90
Start = 16777220
HK_Pause = 80
HK_FrameStep = 70
'@ | Set-Content (Join-Path $Work 'melonDS.toml') -Encoding ascii
}
if ($Fresh) { Remove-Item (Join-Path $Work 'dq9.sav') -ErrorAction SilentlyContinue }
if ($LoadState) { Copy-Item (Resolve-Path $LoadState).Path (Join-Path $Work 'dq9.ml1') -Force }

Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Threading;
public static class W {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    struct RECT { public int l, t, r, b; }
    public static Size Client(IntPtr h) { RECT r; GetClientRect(h, out r); return new Size(r.r, r.b); }
    static IntPtr XY(int x, int y) { return (IntPtr)((y << 16) | x); }
    public static void Tap(IntPtr h, int x, int y) {
        PostMessage(h, 0x200, IntPtr.Zero, XY(x, y)); Thread.Sleep(30);
        PostMessage(h, 0x201, (IntPtr)1, XY(x, y)); Thread.Sleep(120);
        PostMessage(h, 0x202, IntPtr.Zero, XY(x, y));
    }
    // Arrow keys carry the "extended key" bit; without it Qt reads them as keypad keys.
    static long Extended(uint vk) { return vk >= 0x25 && vk <= 0x28 ? 0x01000000L : 0; }
    public static void KeyDown(IntPtr h, uint vk) {
        long scan = MapVirtualKey(vk, 0) << 16;
        PostMessage(h, 0x100, (IntPtr)vk, (IntPtr)(scan | 1 | Extended(vk)));
    }
    public static void KeyUp(IntPtr h, uint vk) {
        long scan = MapVirtualKey(vk, 0) << 16;
        PostMessage(h, 0x101, (IntPtr)vk, (IntPtr)(scan | 0xC0000001L | Extended(vk)));
    }
    public static void Key(IntPtr h, uint vk, int holdMs) {
        KeyDown(h, vk); Thread.Sleep(holdMs); KeyUp(h, vk);
    }
    // Visible top-level windows of a process other than `main` (Qt menu popups).
    public static List<IntPtr> Popups(int pid, IntPtr main) {
        var found = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && h != main && IsWindowVisible(h)) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static void Save(IntPtr h, string path) {
        Size s = Client(h);
        using (Bitmap b = new Bitmap(s.Width, s.Height)) {
            using (Graphics g = Graphics.FromImage(b)) { IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 3); g.ReleaseHdc(dc); }
            b.Save(path, ImageFormat.Png);
        }
    }
    public static Color Pixel(IntPtr h, int x, int y) {
        Size s = Client(h);
        using (Bitmap b = new Bitmap(s.Width, s.Height)) {
            using (Graphics g = Graphics.FromImage(b)) { IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 3); g.ReleaseHdc(dc); }
            return b.GetPixel(x, y);
        }
    }
}
'@
[W]::SetProcessDPIAware() | Out-Null

$proc = Start-Process (Join-Path $Work 'melonDS.exe') -ArgumentList 'dq9.nds' -WorkingDirectory $Work -PassThru
try {
    $h = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(30)
    while ($h -eq [IntPtr]::Zero -or [W]::Client($h).Height -lt 384) {
        if ((Get-Date) -gt $deadline) { throw 'melonDS window did not appear' }
        Start-Sleep -Milliseconds 300
        $h = (Get-Process -Id $proc.Id).MainWindowHandle
    }
    # The touch screen is the bottom 192 rows; the DS screens are 384 rows under the menu bar.
    $touch = [W]::Client($h).Height - 192
    $vk = @{ up = 0x26; down = 0x28; left = 0x25; right = 0x27; a = 0x58; b = 0x5A; start = 0x0D }

    # Qt menu popups are separate windows; the number of Down presses reaches "Save state"
    # (9) or "Load state" (10). Retried once because a posted tap can race the popup.
    function Open-Menu([int] $downs) {
        for ($attempt = 1; $attempt -le 2; $attempt++) {
            try {
                [W]::Tap($h, 14, 12)
                $deadline = (Get-Date).AddSeconds(5)
                while ([W]::Popups($proc.Id, $h).Count -lt 1) {
                    if ((Get-Date) -gt $deadline) { throw 'File menu did not open' }
                    Start-Sleep -Milliseconds 100
                }
                $file = [W]::Popups($proc.Id, $h)[0]
                1..$downs | ForEach-Object { [W]::Key($file, 0x28, 30) }
                [W]::Key($file, 0x27, 30)
                $deadline = (Get-Date).AddSeconds(5)
                while ([W]::Popups($proc.Id, $h).Count -lt 2) {
                    if ((Get-Date) -gt $deadline) { throw 'state submenu did not open' }
                    Start-Sleep -Milliseconds 100
                }
                $sub = [W]::Popups($proc.Id, $h) | Where-Object { $_ -ne $file } | Select-Object -First 1
                [W]::Key($sub, 0x0D, 30)
                return
            } catch {
                if ($attempt -eq 2) { throw }
                [W]::Key($h, 0x1B, 30)
                Start-Sleep -Milliseconds 700
            }
        }
    }

    function Save-State([string] $name) {
        $stateFile = Join-Path $Work 'dq9.ml1'
        $before = if (Test-Path $stateFile) { (Get-Item $stateFile).LastWriteTimeUtc } else { [datetime]::MinValue }
        Open-Menu 9
        $deadline = (Get-Date).AddSeconds(10)
        while ($true) {
            if ((Test-Path $stateFile) -and (Get-Item $stateFile).LastWriteTimeUtc -gt $before) { break }
            if ((Get-Date) -gt $deadline) { throw 'the save state file did not change' }
            Start-Sleep -Milliseconds 100
        }
        Start-Sleep -Milliseconds 300
        if ($name) {
            if (-not $name.EndsWith('.ml1')) { $name += '.ml1' }
            Copy-Item $stateFile (Join-Path $Work $name) -Force
        }
    }

    foreach ($action in $Actions.Split(',') | Where-Object { $_ }) {
        $p = $action.Trim().Split(':')
        switch ($p[0]) {
            'wait' { Start-Sleep -Milliseconds ([int]([double]$p[1] * 1000)) }
            'waitstart' {
                $timeout = if ($p.Count -gt 1) { [double]$p[1] } else { 90 }
                $deadline = (Get-Date).AddSeconds($timeout)
                while ($true) {
                    $c = [W]::Pixel($h, 110, $touch + 160)
                    if ($c.R -gt 200 -and $c.G -gt 100 -and $c.G -lt 200 -and $c.B -lt 120) { break }
                    if ((Get-Date) -gt $deadline) { throw 'no START button on the title screen' }
                    Start-Sleep -Milliseconds 400
                }
            }
            'tap' { [W]::Tap($h, [int]$p[1], $touch + [int]$p[2]) }
            'tapscreen' { [W]::Tap($h, [int]$p[1], [int]$p[2]) }
            'key' {
                $ms = if ($p.Count -gt 2) { [int]$p[2] } else { 80 }
                [W]::Key($h, [uint32]$vk[$p[1]], $ms)
            }
            'hold' {
                $held = $p[1].Split('+') | ForEach-Object { [uint32]$vk[$_] }
                $held | ForEach-Object { [W]::KeyDown($h, $_) }
                Start-Sleep -Milliseconds ([int]([double]$p[2] * 1000))
                $held | ForEach-Object { [W]::KeyUp($h, $_) }
            }
            'shot' {
                $name = if ($p.Count -gt 1) { $p[1] } else { 'shot' }
                if (-not $name.EndsWith('.png')) { $name += '.png' }
                [W]::Save($h, (Join-Path $Work $name))
            }
            'save' { Save-State $(if ($p.Count -gt 1) { $p[1] } else { '' }) }
            'loadstate' { Open-Menu 10; Start-Sleep -Seconds 2 }
            'spam' {
                $duration = [double]$p[1]
                $interval = if ($p.Count -gt 2) { [double]$p[2] } else { 2.0 }
                $deadline = (Get-Date).AddSeconds($duration)
                while ((Get-Date) -lt $deadline) {
                    [W]::Key($h, [uint32]$vk['a'], 120)
                    Start-Sleep -Milliseconds ([int]($interval * 1000))
                }
            }
            'play' {
                $duration = [double]$p[1]; $interval = [double]$p[2]
                $every = [double]$p[3]; $prefix = $p[4]
                $next = (Get-Date).AddSeconds($every); $n = 0
                $deadline = (Get-Date).AddSeconds($duration)
                while ((Get-Date) -lt $deadline) {
                    [W]::Key($h, [uint32]$vk['a'], 120)
                    Start-Sleep -Milliseconds ([int]($interval * 1000))
                    if ((Get-Date) -ge $next) {
                        Save-State ''
                        $n++
                        Copy-Item (Join-Path $Work 'dq9.ml1') (Join-Path $Work "$prefix$n.ml1") -Force
                        $next = (Get-Date).AddSeconds($every)
                    }
                }
            }
            'autosave' {
                $duration = [double]$p[1]; $interval = [double]$p[2]; $prefix = $p[3]
                $n = 0
                $deadline = (Get-Date).AddSeconds($duration)
                while ((Get-Date) -lt $deadline) {
                    Start-Sleep -Milliseconds ([int]($interval * 1000))
                    Save-State ''
                    $n++
                    Copy-Item (Join-Path $Work 'dq9.ml1') (Join-Path $Work "$prefix$n.ml1") -Force
                }
            }
            'pause' { [W]::Key($h, 0x50, 80) }
            'frame' {
                $n = [int]$p[1]
                1..$n | ForEach-Object { [W]::Key($h, 0x46, 30); Start-Sleep -Milliseconds 120 }
            }
            'log' { Write-Host "client $([W]::Client($h).Width)x$([W]::Client($h).Height), touch top $touch" }
            default { throw "unknown action $action" }
        }
    }
} finally {
    if (-not $Keep -and -not $proc.HasExited) {
        [W]::PostMessage($proc.MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        if (-not $proc.WaitForExit(10000)) { $proc.Kill() }
    }
}
