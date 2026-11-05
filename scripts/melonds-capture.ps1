# Loads a melonDS savestate (.ml1) in a portable melonDS and saves both screens as a PNG
# (256x384): the oracle for comparing our renderer with the DS (docs/re/lighting.md).
# Usage: powershell -ExecutionPolicy Bypass -File scripts/melonds-capture.ps1 -State build/melonds/field.ml1 -Out build/scratch/ref.png [-Settle 3]
# Same working directory and input method as scripts/melonds-state.ps1 (build/melonds/, input
# posted to the window, default scale 1, vertical layout). The state is copied to slot 1
# (build/melonds/dq9.ml1), never modified. The window is closed at the end.
param(
    [Parameter(Mandatory)] [string] $State,
    [Parameter(Mandatory)] [string] $Out,
    [double] $Settle = 3
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Work = Join-Path $Root 'build/melonds'
$State = (Resolve-Path $State).Path
New-Item -ItemType Directory -Force $Work | Out-Null
Copy-Item (Join-Path $Root 'tools/melonds/melonDS.exe') $Work -Force
New-Item -ItemType File -Force (Join-Path $Work 'portable') | Out-Null
$Rom = Join-Path $Work 'dq9.nds'
if (-not (Test-Path $Rom)) {
    $src = Get-ChildItem (Join-Path $Root 'rom') -Filter *.nds | Select-Object -First 1
    if (-not $src) { throw 'no ROM in rom/' }
    Copy-Item $src.FullName $Rom
}
Copy-Item $State (Join-Path $Work 'dq9.ml1') -Force
if (-not (Test-Path (Join-Path $Work 'dq9.sav'))) { throw 'build/melonds/dq9.sav is missing (copy the demo save)' }
$Out = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Out))
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null

Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
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
    public static List<IntPtr> Popups(int pid, IntPtr main) {
        var found = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && h != main && IsWindowVisible(h)) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found;
    }
    // Saves the client area's top `rows` rows (the top screen sits right under the menu bar).
    public static void Save(IntPtr h, string path, int skipTop, int w, int rows) {
        Size s = Client(h);
        using (Bitmap b = new Bitmap(s.Width, s.Height)) {
            using (Graphics g = Graphics.FromImage(b)) { IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 3); g.ReleaseHdc(dc); }
            using (Bitmap c = b.Clone(new Rectangle(0, skipTop, w, rows), PixelFormat.Format32bppArgb)) { c.Save(path, ImageFormat.Png); }
        }
    }
}
'@
[Win]::SetProcessDPIAware() | Out-Null

$proc = Start-Process (Join-Path $Work 'melonDS.exe') -ArgumentList 'dq9.nds' -WorkingDirectory $Work -PassThru
try {
    $h = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(30)
    while ($h -eq [IntPtr]::Zero -or [Win]::Client($h).Height -lt 384) {
        if ((Get-Date) -gt $deadline) { throw 'melonDS window did not appear' }
        Start-Sleep -Milliseconds 300
        $h = (Get-Process -Id $proc.Id).MainWindowHandle
    }
    Start-Sleep -Seconds 2
    # File > Load state > 1 (one item below "Save state").
    [Win]::Tap($h, 14, 12)
    $deadline = (Get-Date).AddSeconds(5)
    while ([Win]::Popups($proc.Id, $h).Count -lt 1) { if ((Get-Date) -gt $deadline) { throw 'File menu' }; Start-Sleep -Milliseconds 200 }
    $file = [Win]::Popups($proc.Id, $h)[0]
    1..10 | ForEach-Object { [Win]::Key($file, 0x28) }
    [Win]::Key($file, 0x27)
    $deadline = (Get-Date).AddSeconds(5)
    while ([Win]::Popups($proc.Id, $h).Count -lt 2) { if ((Get-Date) -gt $deadline) { throw 'Load state submenu' }; Start-Sleep -Milliseconds 200 }
    $sub = [Win]::Popups($proc.Id, $h) | Where-Object { $_ -ne $file } | Select-Object -First 1
    [Win]::Key($sub, 0x0D)
    Start-Sleep -Seconds $Settle
    $client = [Win]::Client($h)
    $top = $client.Height - 384
    [Win]::Save($h, $Out, $top, 256, 384)
    Write-Host "wrote $Out (client $($client.Width)x$($client.Height))"
} finally {
    if (-not $proc.HasExited) {
        [Win]::PostMessage($proc.MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        if (-not $proc.WaitForExit(10000)) { $proc.Kill() }
    }
}
