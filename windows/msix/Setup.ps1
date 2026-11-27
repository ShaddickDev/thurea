#Requires -Version 5.1
<#
Thurea Setup, run (as Administrator) by ThureaSetup.exe from thurea-windows.msix.

An MSIX app runs in a container: its registry and AppData writes can be redirected, it can be
removed by any user from Settings > Apps, and its folder changes with every version. Thurea's
protection (browser policies, SYSTEM tasks, locked-down Program Files\Thurea)
can't live there. So this hands the install to a one-off SYSTEM task, which runs outside the
package: Thurea.ps1 -Install copies Thurea from the package to Program Files\Thurea, exactly as
"Install Thurea.cmd" from the zip does. After that the package is only a launcher, and removing
it leaves Thurea in place.
#>
$ErrorActionPreference = 'Stop'
$src = Join-Path $PSScriptRoot 'Thurea'
$ps = "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe"
$installed = Join-Path $env:ProgramFiles 'Thurea'
$ver = (Get-Content -LiteralPath (Join-Path $src 'VERSION') -Raw).Trim()
$have = $null
if (Test-Path -LiteralPath (Join-Path $installed 'VERSION')) { $have = (Get-Content -LiteralPath (Join-Path $installed 'VERSION') -Raw).Trim() }

function Done([int]$code) { Write-Host ''; Read-Host 'Press Enter to close' | Out-Null; exit $code }

# Thurea updates itself from signed releases, so the installed copy is often newer than this
# package: never go back to an older version.
if ($have -and ([version]$have -ge [version]$ver)) {
    Write-Host "Thurea $have is already installed (this setup is $ver)."
} else {
    Write-Host "Installing Thurea $ver..."
    $name = 'Thurea Setup'; $path = '\Thurea\'
    $act = New-ScheduledTaskAction -Execute $ps -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$src\Thurea.ps1`" -Install"
    $p = New-ScheduledTaskPrincipal -UserId 'S-1-5-18' -RunLevel Highest
    $set = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 30)
    Register-ScheduledTask -TaskName $name -TaskPath $path -Action $act -Principal $p -Settings $set -Force | Out-Null
    try {
        Start-ScheduledTask -TaskPath $path -TaskName $name
        # 267009: still running; 267011: hasn't started yet.
        do {
            Start-Sleep -Seconds 2
            $state = (Get-ScheduledTask -TaskPath $path -TaskName $name).State
            $result = (Get-ScheduledTaskInfo -TaskPath $path -TaskName $name).LastTaskResult
        } while ($state -eq 'Running' -or $state -eq 'Queued' -or $result -eq 267009 -or $result -eq 267011)
    } finally {
        Unregister-ScheduledTask -TaskPath $path -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    }
    if ($result -ne 0) {
        Write-Host "Thurea's install didn't finish (exit $result). The log is $env:ProgramData\Thurea\thurea.log."
        Done 1
    }
}
Write-Host ''
& $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $installed 'Thurea.ps1') -Status
Done 0
