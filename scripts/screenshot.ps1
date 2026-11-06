# Launch TurretCAM with arguments, capture its main window to a PNG, close it.
# usage: powershell -File scripts\screenshot.ps1 -Exe target\release\turretcam.exe -Out shot.png -AppArgs "examples\x.tcam.json --tab program --generate"
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$AppArgs = "",
    [string]$Title = "TurretCAM",
    [int]$WaitMs = 3000
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr p);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public static IntPtr Find(uint pid, string title) {
    IntPtr best = IntPtr.Zero; int area = 0;
    EnumWindows((h, p) => {
      uint wp; GetWindowThreadProcessId(h, out wp);
      if (wp != pid || !IsWindowVisible(h)) return true;
      var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
      RECT r; GetWindowRect(h, out r);
      int a = (r.R - r.L) * (r.B - r.T);
      if (sb.ToString().Contains(title) && a > area) { area = a; best = h; }
      return true;
    }, IntPtr.Zero);
    return best;
  }
}
"@
[W]::SetProcessDPIAware() | Out-Null
$root = Split-Path -Parent $PSScriptRoot
$p = Start-Process -FilePath $Exe -ArgumentList $AppArgs -WorkingDirectory $root -PassThru
try {
    $deadline = (Get-Date).AddSeconds(25)
    $h = [IntPtr]::Zero
    while ($h -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250; $h = [W]::Find([uint32]$p.Id, $Title) }
    if ($h -eq [IntPtr]::Zero) { throw "window did not appear" }
    [W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds $WaitMs
    $r = New-Object W+RECT
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $hh))
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Output "saved $Out ($w x $hh)"
}
finally {
    if ($p -and -not $p.HasExited) { $p.Kill() }
}
