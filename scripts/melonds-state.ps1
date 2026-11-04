# Produces a melonDS savestate of the game resumed from a save file, then checks it with
# `dq9re state --save` (resident code vs the ROM, save copies vs RAM).
# Usage: powershell -ExecutionPolicy Bypass -File scripts/melonds-state.ps1 -Save build/sav/demo.sav
#        [-Out partie.ml1] [-Settle 8] [-Keep]
# Boots the ROM in a portable melonDS (build/melonds/, from tools/melonds/), taps START on the
# title screen, then "Continue your adventure" until the title screen is gone, waits -Settle
# seconds in the field, saves state slot 1 (File > Save state > 1) and closes melonDS (unless
# -Keep). Input is posted to the window: it may stay behind other windows, but do not resize
# it (default scale 1, vertical layout). The .sav is copied, never written.
# melonDS maps no DS key by default: for manual play, set them once in Config > Input and
# hotkeys (kept in build/melonds/melonDS.toml).
param(
    [Parameter(Mandatory)] [string] $Save,
    [string] $Out,
    [double] $Settle = 8,
    [switch] $Keep
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Work = Join-Path $Root 'build/melonds'
$Save = (Resolve-Path $Save).Path

# --- Portable melonDS working directory ---------------------------------------------------
New-Item -ItemType Directory -Force $Work | Out-Null
Copy-Item (Join-Path $Root 'tools/melonds/melonDS.exe') $Work -Force
New-Item -ItemType File -Force (Join-Path $Work 'portable') | Out-Null
$Rom = Join-Path $Work 'dq9.nds'
if (-not (Test-Path $Rom)) {
    $src = if ($env:DQ9_ROM) { Get-Item $env:DQ9_ROM } else {
        @(Get-ChildItem (Join-Path $Root 'rom'), $Root -Filter *.nds -File)[0]
    }
    if (-not $src) { throw 'no ROM: put it in rom/ or set DQ9_ROM' }
    try { New-Item -ItemType HardLink -Path $Rom -Target $src.FullName | Out-Null }
    catch { Copy-Item $src.FullName $Rom }
}
Copy-Item $Save (Join-Path $Work 'dq9.sav') -Force
$State = Join-Path $Work 'dq9.ml1'
Remove-Item $State -ErrorAction SilentlyContinue

# --- Window input and capture (posted messages: no focus needed) --------------------------
Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Threading;
public static class Win {
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
    public static void Key(IntPtr h, uint vk) {
        long scan = MapVirtualKey(vk, 0) << 16;
        PostMessage(h, 0x100, (IntPtr)vk, (IntPtr)(scan | 1)); Thread.Sleep(80);
        PostMessage(h, 0x101, (IntPtr)vk, (IntPtr)(scan | 0xC0000001L)); Thread.Sleep(250);
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
    public static Color Pixel(IntPtr h, int x, int y) {
        Size s = Client(h);
        using (Bitmap b = new Bitmap(s.Width, s.Height)) {
            using (Graphics g = Graphics.FromImage(b)) { IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 3); g.ReleaseHdc(dc); }
            return b.GetPixel(x, y);
        }
    }
}
'@
[Win]::SetProcessDPIAware() | Out-Null

function Wait-Until([scriptblock] $Test, [double] $Seconds, [string] $What) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while (-not (& $Test)) {
        if ((Get-Date) -gt $deadline) { throw "timed out: $What" }
        Start-Sleep -Milliseconds 300
    }
}

$proc = Start-Process (Join-Path $Work 'melonDS.exe') -ArgumentList 'dq9.nds' -WorkingDirectory $Work -PassThru
try {
    $script:h = [IntPtr]::Zero
    Wait-Until {
        $script:h = (Get-Process -Id $proc.Id).MainWindowHandle
        $script:h -ne [IntPtr]::Zero -and [Win]::Client($script:h).Height -ge 384
    } 30 'melonDS window'
    $h = $script:h
    Start-Sleep -Seconds 1
    # Touch screen = bottom 256x192 of the client area (the menu bar is part of it).
    $top = [Win]::Client($h).Height - 192
    function Test-StartButton {
        $c = [Win]::Pixel($h, 110, $top + 160)
        $c.R -gt 200 -and $c.G -gt 100 -and $c.G -lt 200 -and $c.B -lt 120
    }
    # A few pixels of both screens, to notice when the title menu goes away.
    $probes = @(@(128, 60), @(128, 150), @(200, $top + 130), @(40, $top + 60))
    function Get-Signature { ($probes | ForEach-Object { [Win]::Pixel($h, $_[0], $_[1]).ToArgb() }) -join ',' }

    # Intro: the orange START button stays until the title menu replaces it.
    Wait-Until { Test-StartButton } 60 'title screen START button'
    $deadline = (Get-Date).AddSeconds(60)
    while (Test-StartButton) {
        if ((Get-Date) -gt $deadline) { throw 'title menu did not open' }
        [Win]::Tap($h, 127, $top + 160)
        Start-Sleep -Seconds 2
    }
    Start-Sleep -Seconds 1.5
    $title = Get-Signature
    # "Continue your adventure": the first tap selects it, a later one confirms. Tap until the
    # screens change: the earlier version saved the title screen when a confirmation was lost.
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Signature) -eq $title) {
        if ((Get-Date) -gt $deadline) { throw 'the game did not leave the title menu' }
        [Win]::Tap($h, 70, $top + 19)
        Start-Sleep -Seconds 2.5
    }
    Start-Sleep -Seconds $Settle

    # File > Save state > 1, with the keyboard inside Qt's popups (9 enabled items above it).
    [Win]::Tap($h, 14, 12)
    Wait-Until { [Win]::Popups($proc.Id, $h).Count -ge 1 } 5 'File menu'
    $file = [Win]::Popups($proc.Id, $h)[0]
    1..9 | ForEach-Object { [Win]::Key($file, 0x28) }   # Down to "Save state"
    [Win]::Key($file, 0x27)                             # Right: open the submenu
    Wait-Until { [Win]::Popups($proc.Id, $h).Count -ge 2 } 5 'Save state submenu'
    $sub = [Win]::Popups($proc.Id, $h) | Where-Object { $_ -ne $file } | Select-Object -First 1
    [Win]::Key($sub, 0x0D)                              # Return on "1"
    Wait-Until { Test-Path $State } 10 'savestate file'
    Start-Sleep -Seconds 1
} finally {
    if (-not $Keep -and -not $proc.HasExited) {
        [Win]::PostMessage($proc.MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        if (-not $proc.WaitForExit(10000)) { $proc.Kill() }
    }
}
if ($Out) { Copy-Item $State $Out -Force; $State = $Out }
Push-Location $Root
try { cargo dq9re state $State --save $Save } finally { Pop-Location }
