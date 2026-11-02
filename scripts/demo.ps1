#requires -Version 5.0
<#
Records the raw footage for the demo video: closes LockIn, runs `lockin --demo` (a scripted ~80 s
showcase on a throwaway data folder; your settings, stats and tasks are not touched; it covers the
screen with its own plain backdrop first, so nothing of your desktop is recorded), captures the
screen with FFmpeg meanwhile, then starts your LockIn again.

Sit in front of the camera and look at the screen while it runs: the main window shows your gaze
tracking (lines only, never the camera picture), and the calibration step asks you to follow the
ball with your eyes, then nod.

Output (in -Out): raw.mp4, marks.json (caption per scene, demo clock), capture.json (sync).
#>
[CmdletBinding()]
param(
    [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target-fast\release\lockin.exe'),
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) 'demo-video\assets\footage')
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Exe)) { throw "LockIn exe not found: $Exe (build with: cargo build --release --target-dir target-fast)" }
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'ffmpeg not found on PATH (winget install Gyan.FFmpeg, then open a new terminal)' }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

# Physical pixels of the primary monitor (FFmpeg's gdigrab works in physical pixels).
Add-Type @'
using System.Runtime.InteropServices;
public static class DemoDpi {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
'@
[DemoDpi]::SetProcessDPIAware() | Out-Null
$w = [DemoDpi]::GetSystemMetrics(0)
$h = [DemoDpi]::GetSystemMetrics(1)
# x264 needs even sizes.
$w -= $w % 2; $h -= $h % 2

$wasRunning = [bool](Get-Process lockin -ErrorAction SilentlyContinue)
Get-Process lockin -ErrorAction SilentlyContinue | Stop-Process
Start-Sleep -Milliseconds 800

Write-Host "Sit in front of your camera and look at the screen. Starting in 5 s; it takes about 90 s. Hands off the mouse."
Start-Sleep 5

# The demo puts its backdrop up on its first frame and waits 2.5 s before its first step, so the
# capture starts once the desktop is already covered.
$demo = Start-Process $Exe -PassThru -ArgumentList @('--demo', "`"$Out\marks.json`"")
Start-Sleep -Milliseconds 1200

$ffmpeg = New-Object System.Diagnostics.Process
$ffmpeg.StartInfo.FileName = (Get-Command ffmpeg).Source
$ffmpeg.StartInfo.Arguments = "-y -loglevel error -f gdigrab -framerate 30 -draw_mouse 1 -offset_x 0 -offset_y 0 -video_size ${w}x${h} -i desktop -c:v libx264 -preset ultrafast -crf 16 -pix_fmt yuv420p `"$Out\raw.mp4`""
$ffmpeg.StartInfo.UseShellExecute = $false
$ffmpeg.StartInfo.RedirectStandardInput = $true
$ffmpeg.StartInfo.CreateNoWindow = $true
$ffmpeg.Start() | Out-Null
$captureStart = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()

try {
    if (-not $demo.WaitForExit(180000)) { $demo | Stop-Process; throw 'The demo did not finish within 3 minutes' }
} finally {
    # Stop recording the moment the demo (and its backdrop) is gone.
    $ffmpeg.StandardInput.Write('q')
    $ffmpeg.StandardInput.Close()
    if (-not $ffmpeg.WaitForExit(30000)) { $ffmpeg.Kill() }
    @{ capture_start_unix_ms = "$captureStart"; width = $w; height = $h } | ConvertTo-Json | Set-Content "$Out\capture.json"
    if ($wasRunning) { Start-Process $Exe -WorkingDirectory (Split-Path -Parent $Exe) }
}
Write-Host "Footage: $Out\raw.mp4"
