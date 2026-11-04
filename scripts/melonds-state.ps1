# Produces a melonDS savestate of the game resumed from a save file, then prints `dq9re state`.
# Usage: powershell -File scripts/melonds-state.ps1 -Save build/sav/demo.sav [-Out partie.ml1]
#        [-Settle 8] [-Keep]
# Boots the ROM in a portable melonDS (build/melonds/, from tools/melonds/), taps START on the
# title screen and "Continue your adventure" twice, waits -Settle seconds in the field, saves
# state slot 1 (Shift+F1) and closes melonDS (unless -Keep). The .sav is copied, never written.
# Assumes the default window (scale 1, vertical layout): do not resize it while the script runs.
# melonDS maps no DS key by default: for manual play, set them once in Config > Input and
# hotkeys (kept in build/melonds/melonDS.toml). Savestate hotkeys work without it.
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

# --- Window input and capture -------------------------------------------------------------
Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public static class Win {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
    struct RECT { public int l, t, r, b; }
    struct POINT { public int x, y; }
    public static Size Client(IntPtr h) { RECT r; GetClientRect(h, out r); return new Size(r.r, r.b); }
    public static void Focus(IntPtr h) { ShowWindow(h, 9); SetForegroundWindow(h); System.Threading.Thread.Sleep(150); }
    public static void Tap(IntPtr h, int x, int y) {
        Focus(h); POINT p = new POINT { x = x, y = y }; ClientToScreen(h, ref p); SetCursorPos(p.x, p.y);
        System.Threading.Thread.Sleep(50); mouse_event(2, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(120); mouse_event(4, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Keys(IntPtr h, byte[] vks) {
        Focus(h);
        foreach (byte k in vks) keybd_event(k, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(120);
        for (int i = vks.Length - 1; i >= 0; i--) keybd_event(vks[i], 0, 2, UIntPtr.Zero);
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

$proc = Start-Process (Join-Path $Work 'melonDS.exe') -ArgumentList 'dq9.nds' -WorkingDirectory $Work -PassThru
try {
    $deadline = (Get-Date).AddSeconds(30)
    while (($h = (Get-Process -Id $proc.Id).MainWindowHandle) -eq [IntPtr]::Zero) {
        if ((Get-Date) -gt $deadline) { throw 'melonDS window did not appear' }
        Start-Sleep -Milliseconds 300
    }
    # Touch screen = bottom 256x192 of the client area (the menu bar is part of it).
    while ([Win]::Client($h).Height -lt 384) {
        if ((Get-Date) -gt $deadline) {
            throw "melonDS window has no screen area (client $([Win]::Client($h)))"
        }
        Start-Sleep -Milliseconds 300
        $h = (Get-Process -Id $proc.Id).MainWindowHandle
    }
    Start-Sleep -Seconds 1
    $top = [Win]::Client($h).Height - 192
    function Test-StartButton {
        $c = [Win]::Pixel($h, 110, $top + 160)
        $c.R -gt 200 -and $c.G -gt 100 -and $c.G -lt 200 -and $c.B -lt 120
    }
    # Intro: the orange START button stays until the title menu replaces it.
    $deadline = (Get-Date).AddSeconds(60)
    while (-not (Test-StartButton)) {
        if ((Get-Date) -gt $deadline) { throw 'title screen START button not seen' }
        Start-Sleep -Milliseconds 500
    }
    while (Test-StartButton) {
        if ((Get-Date) -gt $deadline) { throw 'title menu did not open' }
        [Win]::Tap($h, 127, $top + 160)
        Start-Sleep -Seconds 2
    }
    # "Continue your adventure": first tap selects, second confirms.
    [Win]::Tap($h, 70, $top + 19); Start-Sleep -Seconds 1.5
    [Win]::Tap($h, 70, $top + 19); Start-Sleep -Seconds $Settle
    [Win]::Keys($h, [byte[]](0x10, 0x70))   # Shift+F1: save state slot 1
    $deadline = (Get-Date).AddSeconds(10)
    while (-not (Test-Path $State)) {
        if ((Get-Date) -gt $deadline) { throw 'savestate not written' }
        Start-Sleep -Milliseconds 300
    }
    Start-Sleep -Seconds 1
} finally {
    if (-not $Keep -and -not $proc.HasExited) {
        [Win]::PostMessage($proc.MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        if (-not $proc.WaitForExit(10000)) { $proc.Kill() }
    }
}
if ($Out) { Copy-Item $State $Out -Force; $State = $Out }
Push-Location $Root
try { cargo dq9re state $State } finally { Pop-Location }
