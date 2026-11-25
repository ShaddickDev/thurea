# Test run of the Thurea Windows setup in the (offline) build VM, as Administrator.
# Results go to <XFER>\test\results.txt; QR test images to <XFER>\test\qr-*.png.
$ErrorActionPreference = 'Continue'
$X = (Get-Volume -FileSystemLabel XFER).DriveLetter + ':'
$T = "$X\test"
Start-Transcript -Path "$T\results.txt" -Force | Out-Null
function Section($m) { Write-Host "`n===== $m =====" }
$ext = (Get-Content "$T\stage\Thurea\data\extension-id" -Raw).Trim()

Section 'Compile tray app'
$csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$tray = "$T\stage\Thurea\ThureaTray.exe"
& $csc /nologo /target:winexe /optimize+ /out:$tray /r:System.Windows.Forms.dll /r:System.Drawing.dll /r:System.Web.Extensions.dll "$T\stage\Thurea\tray\ThureaTray.cs"
"csc exit $LASTEXITCODE; exe: $(Test-Path $tray)"
$i = 0
foreach ($u in 'http://192.168.1.23:47824/AbCdEfGhIjKl/', 'Thurea', ('https://example.com/' + ('x' * 300))) {
    $i++; & $tray --qr-test $u "$T\qr-$i.png"; Start-Sleep 1; Set-Content "$T\qr-$i.txt" $u -NoNewline
}
Get-ChildItem "$T\qr-*.png" | Format-Table Name, Length

Section 'Setup-Thurea (Helium, default browser, Edge removal, Thurea)'
# Undo the previous attempt (its permissions locked everyone out of Program Files\Thurea).
Get-ScheduledTask -TaskPath '\Thurea\' -ErrorAction SilentlyContinue | Stop-ScheduledTask -ErrorAction SilentlyContinue
Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object CommandLine -match '-Serve' | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
foreach ($d in "$env:ProgramFiles\Thurea", "$env:ProgramData\Thurea") {
    if (Test-Path $d) { & takeown /f $d /r /d y | Out-Null; & icacls $d /reset /T /C /Q | Out-Null; Remove-Item $d -Recurse -Force }
}
# Keep this test VM signing in by itself.
$wl = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Set-ItemProperty $wl AutoAdminLogon '1'; Set-ItemProperty $wl DefaultUserName 'Administrator'; Set-ItemProperty $wl DefaultPassword 'build-vm-only'
Remove-ItemProperty $wl AutoLogonCount -ErrorAction SilentlyContinue
$S = 'C:\Windows\Setup\Thurea'
Remove-Item $S -Recurse -Force -ErrorAction SilentlyContinue
& robocopy "$T\stage" $S /E /NFL /NDL /NJH /NJS | Out-Null
$sw = [Diagnostics.Stopwatch]::StartNew()
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$S\Setup-Thurea.ps1"
"Setup-Thurea took $([int]$sw.Elapsed.TotalSeconds) s"
Get-Content "$S\setup-thurea.log" -ErrorAction SilentlyContinue | Select-Object -Last 40

Section 'Thurea status'
& icacls "$env:ProgramFiles\Thurea" ; & icacls "$env:ProgramFiles\Thurea\data\hosts-block"
"read as admin: $((Get-Content "$env:ProgramFiles\Thurea\data\extension-id" -Raw).Trim())"
try { "update server: $((Invoke-WebRequest -UseBasicParsing http://127.0.0.1:8137/updates.xml -TimeoutSec 5).StatusCode)" } catch { "update server: $_" }
Get-NetIPAddress -AddressFamily IPv4 | Format-Table InterfaceAlias, IPAddress
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$env:ProgramFiles\Thurea\Thurea.ps1" -Status
Get-Content "$env:ProgramData\Thurea\thurea.log" -ErrorAction SilentlyContinue | Select-Object -Last 30
Get-ScheduledTask -TaskPath '\Thurea\' | Format-Table TaskName, State
Get-NetFirewallRule -Group 'Thurea', 'Thurea' | Format-Table DisplayName, Enabled, Direction, Action
Get-DnsClientServerAddress | Where-Object ServerAddresses | Format-Table InterfaceAlias, AddressFamily, ServerAddresses
Get-DnsClientDohServerAddress | Where-Object ServerAddress -in '1.1.1.3', '1.0.0.3' | Format-Table
"hosts lines: $((Get-Content "$env:WINDIR\System32\drivers\etc\hosts").Count)"
(Get-Acl "$env:WINDIR\System32\drivers\etc\hosts").Sddl

Section 'Edge'
"msedge.exe present: $(Test-Path "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe")"
"WebView2 present: $(Test-Path "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application")"
Get-AppxPackage -AllUsers *MicrosoftEdge* | Format-Table Name, Version
Get-AppxProvisionedPackage -Online | Where-Object DisplayName -match 'Edge' | Format-Table DisplayName

Section 'Default browser (new accounts)'
& dism.exe /Online /English /Get-DefaultAppAssociations | Select-String -Pattern 'http|htm|pdf' | ForEach-Object { $_.Line }
Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -ErrorAction SilentlyContinue | Format-List StartLayoutFile, LockedStartLayout

Section 'DNS with the Thurea hosts file'
foreach ($n in 'www.007angels.com', 'pornhub.com', 'example.com') {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $r = Resolve-DnsName $n -Type A -ErrorAction SilentlyContinue
    "{0,-20} -> {1,-12} {2} ms" -f $n, (@($r | Where-Object IPAddress | ForEach-Object IPAddress) -join ','), $sw.ElapsedMilliseconds
}
$svc = Get-CimInstance Win32_Service -Filter "Name='Dnscache'"
$p = Get-Process -Id $svc.ProcessId
"Dnscache host process: {0:N0} MB working set, {1:N1} s CPU" -f ($p.WorkingSet64 / 1MB), $p.CPU

Section 'Chrome install'
$sw = [Diagnostics.Stopwatch]::StartNew()
Start-Process msiexec.exe -ArgumentList "/i `"$T\GoogleChromeStandaloneEnterprise64.msi`" /qn /norestart" -Wait
"Chrome installed: $(Test-Path "$env:ProgramFiles\Google\Chrome\Application\chrome.exe") ($([int]$sw.Elapsed.TotalSeconds) s)"
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$env:ProgramFiles\Thurea\Thurea.ps1" -Apply | Select-Object -Last 3

Section 'Thurea extension force-install'
$helium = (Get-ChildItem 'HKLM:\SOFTWARE\Clients\StartMenuInternet' | Where-Object PSChildName -match 'Helium' | ForEach-Object { (Get-ItemProperty (Join-Path $_.PSPath 'shell\open\command')).'(default)' }) -replace '"', ''
"Helium: $helium"
$browsers = [ordered]@{ Helium = @($helium, "$env:LOCALAPPDATA\imput\Helium\User Data"); Chrome = @("$env:ProgramFiles\Google\Chrome\Application\chrome.exe", "$env:LOCALAPPDATA\Google\Chrome\User Data") }
foreach ($b in $browsers.GetEnumerator()) {
    $exe = $b.Value[0]
    if (-not (Test-Path $exe)) { "$($b.Key): not installed"; continue }
    $proc = Start-Process -FilePath $exe -ArgumentList '--no-first-run', '--no-default-browser-check', 'about:blank' -PassThru
    $deadline = (Get-Date).AddSeconds(120); $found = $null
    while ((Get-Date) -lt $deadline -and -not $found) {
        Start-Sleep 5
        $found = Get-ChildItem -Path $b.Value[1] -Recurse -Directory -Filter $ext -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if (-not $found) { $found = Get-ChildItem -Path "$env:LOCALAPPDATA" -Recurse -Directory -Filter $ext -ErrorAction SilentlyContinue | Where-Object FullName -match $b.Key | Select-Object -First 1 }
    "$($b.Key): Thurea extension installed: $([bool]$found) $(if ($found) { $found.FullName; (Get-ChildItem $found.FullName -Directory | ForEach-Object Name) -join ',' })"
    Start-Sleep 5
    Get-Process | Where-Object { $_.Path -eq $exe } | Stop-Process -Force -ErrorAction SilentlyContinue
}
Get-Content "$env:ProgramData\Thurea\thurea.log" -ErrorAction SilentlyContinue | Select-Object -Last 10

Section 'Done'
Stop-Transcript | Out-Null
Set-Content "$T\DONE" 'ok'
Stop-Computer -Force
