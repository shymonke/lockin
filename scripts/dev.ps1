#requires -Version 5.0
[CmdletBinding()]
param([switch]$Release, [switch]$Once)

$ErrorActionPreference = 'Stop'
# Some launchers pass down the Win32 "ignore Ctrl+C" flag. Clear it so the
# PowerShell host can stop the pipeline and run our finally block normally.
if (-not ('LockInDevConsole' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class LockInDevConsole {
    [DllImport("kernel32.dll")]
    public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
}
'@
}
$null = [LockInDevConsole]::SetConsoleCtrlHandler([IntPtr]::Zero, $false)
$repo = Split-Path -Parent $PSScriptRoot
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = 'C:\Users\monke\.cargo\bin;C:\Users\monke\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;' + $env:PATH
}
$cargo = (Get-Command cargo -ErrorAction Stop).Source
$profile = if ($Release) { 'release' } else { 'debug' }
$runDir = Join-Path $repo 'run'
$exe = Join-Path $runDir 'lockin.exe'
$app = $null
$build = $null
$watchers = @{}
$prefix = 'LockIn.Dev.' + [Guid]::NewGuid().ToString('N')
$stdout = [IO.Path]::GetTempFileName()
$stderr = [IO.Path]::GetTempFileName()

function Stop-OwnedProcess($process) {
    if ($null -ne $process) {
        if (-not $process.HasExited) {
            # Cargo's rustup shim can have cargo/rustc/linker children.
            $killer = Start-Process -FilePath "$env:SystemRoot\System32\taskkill.exe" -ArgumentList @('/PID', $process.Id, '/T', '/F') -WindowStyle Hidden -PassThru -Wait
            $killer.Dispose()
        }
        $process.WaitForExit() # Release the single-instance mutex before launching again.
        $process.Dispose()
    }
}

function Add-Watcher($key, $path, $recursive) {
    $watcher = New-Object IO.FileSystemWatcher
    $watcher.Path = $path
    $watcher.IncludeSubdirectories = $recursive
    $watcher.NotifyFilter = [IO.NotifyFilters]'LastWrite, FileName, DirectoryName, Size'
    foreach ($kind in @('Changed', 'Created', 'Deleted', 'Renamed', 'Error')) {
        Register-ObjectEvent $watcher $kind -SourceIdentifier "$prefix.$key.$kind" | Out-Null
    }
    $watchers[$key] = $watcher
    $watcher.EnableRaisingEvents = $true
}

function Remove-Watcher($key) {
    $watcher = $watchers[$key]
    $watcher.EnableRaisingEvents = $false
    foreach ($kind in @('Changed', 'Created', 'Deleted', 'Renamed', 'Error')) {
        Unregister-Event -SourceIdentifier "$prefix.$key.$kind" -ErrorAction SilentlyContinue
    }
    $watcher.Dispose()
    $watchers.Remove($key)
}

function Build-AndLaunch {
    $arguments = @('build')
    if ($Release) { $arguments += '--release' }
    Write-Host "Building $profile..."
    $script:build = Start-Process -FilePath $cargo -ArgumentList $arguments -WorkingDirectory $repo -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    # Retain the native handle before exit: PowerShell 5 can otherwise lose ExitCode.
    $null = $script:build.Handle
    while (-not $script:build.HasExited) { Start-Sleep -Milliseconds 100 }
    $script:build.WaitForExit()
    $code = $script:build.ExitCode
    $script:build.Dispose()
    $script:build = $null
    if ($code -ne 0) {
        Write-Host 'Build failed; keeping the previous instance running.' -ForegroundColor Red
        Get-Content -LiteralPath $stdout, $stderr -Tail 40 | Out-Host
        return $false
    }
    Get-Content -LiteralPath $stderr | Out-Host
    Stop-OwnedProcess $script:app
    $script:app = $null
    New-Item -ItemType Directory -Path $runDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repo "target\$profile\lockin.exe") -Destination $exe -Force
    $webViewLoader = Join-Path $repo "target\$profile\WebView2Loader.dll"
    if (Test-Path -LiteralPath $webViewLoader -PathType Leaf) {
        Copy-Item -LiteralPath $webViewLoader -Destination (Join-Path $runDir 'WebView2Loader.dll') -Force
    }
    $script:app = Start-Process -FilePath $exe -WorkingDirectory $repo -WindowStyle Hidden -PassThru
    $null = $script:app.Handle
    Start-Sleep -Milliseconds 300
    if ($script:app.HasExited) {
        throw 'LockIn exited immediately. Check whether another LockIn already owns Local\LockIn.SingleInstance.'
    }
    Write-Host "Started LockIn PID $($script:app.Id). Ctrl+C stops the app and watcher."
    return $true
}

try {
    if (-not $Once) {
        Add-Watcher 'root' $repo $false
        foreach ($dir in @('src', 'assets', 'models')) {
            $path = Join-Path $repo $dir
            if (Test-Path -LiteralPath $path -PathType Container) { Add-Watcher $dir $path $true }
        }
    }
    $built = Build-AndLaunch
    if ($Once) {
        if (-not $built) { throw 'The one-shot build failed.' }
        # Keep ownership until exit so Ctrl+C has the same cleanup behavior in both modes.
        while ($null -ne $app -and -not $app.HasExited) { Start-Sleep -Milliseconds 200 }
    } else {
        $changedAt = $null
        while ($true) {
            foreach ($event in @(Get-Event | Where-Object { $_.SourceIdentifier.StartsWith($prefix + '.') })) {
                $key = $event.SourceIdentifier.Substring($prefix.Length + 1).Split('.')[0]
                $eventArgs = $event.SourceEventArgs
                $relevant = $key -ne 'root' -or $event.SourceIdentifier.EndsWith('.Error')
                if ($key -eq 'root' -and -not $relevant) {
                    foreach ($name in @($eventArgs.Name, $eventArgs.OldName)) {
                        if ($name -eq 'Cargo.toml') { $relevant = $true }
                        if ($name -in @('src', 'assets', 'models')) {
                            $relevant = $true
                            if ($watchers.ContainsKey($name)) { Remove-Watcher $name }
                        }
                    }
                }
                if ($relevant) { $changedAt = [DateTime]::UtcNow }
                Remove-Event -EventIdentifier $event.EventIdentifier
            }
            # Also handles optional directories appearing after startup.
            foreach ($dir in @('src', 'assets', 'models')) {
                $path = Join-Path $repo $dir
                if (-not $watchers.ContainsKey($dir) -and (Test-Path -LiteralPath $path -PathType Container)) {
                    Add-Watcher $dir $path $true
                }
            }
            if ($null -ne $changedAt -and ([DateTime]::UtcNow - $changedAt).TotalSeconds -ge 1) {
                $changedAt = $null
                $null = Build-AndLaunch
            }
            Start-Sleep -Milliseconds 100
        }
    }
} finally {
    foreach ($key in @($watchers.Keys)) { Remove-Watcher $key }
    Get-Event | Where-Object { $_.SourceIdentifier.StartsWith($prefix + '.') } | Remove-Event
    Stop-OwnedProcess $build
    Stop-OwnedProcess $app
    Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
}
