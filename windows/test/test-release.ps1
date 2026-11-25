# Thurea release test in the offline build VM, as Administrator: install from the release zip, the
# tray app (compiled by Windows' own csc.exe), the uninstall block and the status report.
# Results: <XFER>\test\results-41.txt and <XFER>\test\*.png. Shuts the VM down at the end.
$ErrorActionPreference = 'Continue'
$X = (Get-Volume -FileSystemLabel XFER).DriveLetter + ':'
$T = "$X\test"
Start-Transcript -Path "$T\results-41.txt" -Force | Out-Null
function Section($m) { Write-Host "`n===== $m =====" }

Section 'Install from thurea-windows.zip'
$W = "$env:TEMP\thurea41"
Remove-Item $W -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive "$T\thurea-windows.zip" $W -Force
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$W\Thurea\Thurea.ps1" -Install -Package "$T\thurea-windows.zip"
"install exit $LASTEXITCODE"
Get-Content "$env:ProgramData\Thurea\thurea.log" -ErrorAction SilentlyContinue | Select-Object -Last 25

Section 'Tray app'
$tray = "$env:ProgramFiles\Thurea\ThureaTray.exe"
"ThureaTray.exe built: $(Test-Path $tray)"
Get-Content "$env:ProgramData\Thurea\thurea.log" | Select-String 'tray app did not compile' | Select-Object -Last 1
& $tray --icon-test "$T\icon-41.png"; Start-Sleep 2
& $tray --qr-test '{"android.app.extra.PROVISIONING_DEVICE_ADMIN_COMPONENT_NAME":"com.thurea.guard/.AdminReceiver"}' "$T\qr-41.png"; Start-Sleep 2
"icon png: $(Test-Path "$T\icon-41.png"); qr png: $(Test-Path "$T\qr-41.png")"
"verses.json: $(Test-Path "$env:ProgramFiles\Thurea\data\verses.json"); CREDITS.md: $(Test-Path "$env:ProgramFiles\Thurea\CREDITS.md"); Thurea.apk: $(Test-Path "$env:ProgramFiles\Thurea\android\Thurea.apk")"
"Run key: $((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run').Thurea)"
"Start menu: $(Test-Path "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Thurea.lnk")"
Get-Process ThureaTray -ErrorAction SilentlyContinue | Format-Table Id, SessionId, MainWindowTitle

Section 'Uninstall block'
$u = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Thurea' -ErrorAction SilentlyContinue
"Apps entry: $($u.DisplayName) $($u.DisplayVersion) NoRemove=$($u.NoRemove) NoModify=$($u.NoModify)"
Get-ScheduledTask -TaskPath '\Thurea\' | Format-Table TaskName, State
Unregister-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea' -Confirm:$false
Disable-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Update' | Out-Null
"after tampering:"; Get-ScheduledTask -TaskPath '\Thurea\' | Format-Table TaskName, State
Start-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Watch'
$deadline = (Get-Date).AddMinutes(4)
do { Start-Sleep 10; $t = Get-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Watch' } while ($t.State -eq 'Running' -and (Get-Date) -lt $deadline)
"after Thurea Watch ran:"; Get-ScheduledTask -TaskPath '\Thurea\' | Format-Table TaskName, State
Get-Content "$env:ProgramData\Thurea\thurea.log" | Select-String 'tasks:' | Select-Object -Last 2

Section 'Status'
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$env:ProgramFiles\Thurea\Thurea.ps1" -Status
Get-Content "$env:ProgramData\Thurea\status.json"

Section 'Tray window'
# Open the window in the signed-in session (the tray is already running there: it signals it).
$p = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Limited
Register-ScheduledTask -TaskName 'Thurea Tray Test' -TaskPath '\' -Action (New-ScheduledTaskAction -Execute $tray) -Principal $p -Force | Out-Null
Start-ScheduledTask -TaskName 'Thurea Tray Test'; Start-Sleep 8
Unregister-ScheduledTask -TaskName 'Thurea Tray Test' -Confirm:$false
Get-Process ThureaTray -ErrorAction SilentlyContinue | Format-Table Id, SessionId, MainWindowTitle
"done $(Get-Date -Format T)"
Stop-Transcript | Out-Null
