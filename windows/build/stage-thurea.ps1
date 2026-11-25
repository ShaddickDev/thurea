# Dot-sourced by build.ps1 (in the build VM) with $X (transfer disk), $W (work dir) and $iso set.
# Puts Thurea on the setup media and makes Windows Setup run it:
#   sources\$OEM$\$$\Setup\Thurea  ->  C:\Windows\Setup\Thurea on the installed PC
#   specialize pass: Setup-Thurea.ps1 (Helium, default browser, Edge removed, Thurea)
$sh = "$X\thurea"
if (-not (Test-Path "$sh\Setup-Thurea.ps1")) { throw "Thurea stage missing on the transfer disk ($sh)" }
$stage = "$W\thurea"
& robocopy $sh $stage /E /NFL /NDL /NJH /NJS | Out-Null
if ($LASTEXITCODE -ge 8) { throw "copy of $sh failed ($LASTEXITCODE)" }

# Tray app, compiled with the C# compiler that ships with Windows (.NET Framework 4.8).
$csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$traySources = @(Get-ChildItem -LiteralPath "$stage\Thurea\tray" -Filter '*.cs' | ForEach-Object FullName)
& $csc /nologo /target:winexe /optimize+ /out:"$stage\Thurea\ThureaTray.exe" /r:System.Windows.Forms.dll /r:System.Drawing.dll /r:System.Web.Extensions.dll /r:System.Core.dll @traySources
if ($LASTEXITCODE) { throw "Thurea tray did not compile ($LASTEXITCODE)" }
$qrCheck = "$W\qr-check.png"
if (Test-Path -LiteralPath $qrCheck) { Remove-Item -LiteralPath $qrCheck }
$qrTest = Start-Process -FilePath "$stage\Thurea\ThureaTray.exe" -ArgumentList '--qr-test', 'http://192.168.1.2:47824/test/', "`"$qrCheck`"" -Wait -PassThru
if ($qrTest.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $qrCheck)) { throw 'ThureaTray.exe failed the QR test' }
Log 'ThureaTray.exe built'

# "Thurea for Windows" download (offered by the tray's QR page): Thurea without the phone app.
$zipRoot = "$W\thurea-zip\Thurea"
& robocopy "$stage\Thurea" $zipRoot /E /NFL /NDL /NJH /NJS /XD android windows | Out-Null
New-Item -ItemType Directory -Force -Path "$stage\Thurea\windows" | Out-Null
Compress-Archive -Path $zipRoot -DestinationPath "$stage\Thurea\windows\ThureaForWindows.zip" -Force
Log "ThureaForWindows.zip: $([int]((Get-Item "$stage\Thurea\windows\ThureaForWindows.zip").Length / 1KB)) KB"

$oem = Join-Path $iso 'sources\$OEM$\$$\Setup\Thurea'
& robocopy $stage $oem /E /NFL /NDL /NJH /NJS | Out-Null
if ($LASTEXITCODE -ge 8) { throw "copy to $oem failed ($LASTEXITCODE)" }
Log "Thurea staged at sources\`$OEM`$\`$`$\Setup\Thurea ($([int]((Get-ChildItem $oem -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)) MB)"

# GPU driver packages at the root of the media (not in the image): Setup-Thurea.ps1 installs
# only the one matching the PC's graphics card.
if (Test-Path "$X\thurea-gpu") {
    & robocopy "$X\thurea-gpu" "$iso\thurea-gpu" /E /NFL /NDL /NJH /NJS | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "copy of thurea-gpu failed ($LASTEXITCODE)" }
    Set-Content "$iso\thurea-gpu\gpu.txt" 'GPU drivers for Thurea Windows setup: intel-arc, nvidia, amd. Setup-Thurea.ps1 installs the one this PC needs.'
    Log "GPU packages on the media: $((Get-ChildItem "$iso\thurea-gpu" -Directory).Name -join ', ')"
}

# Specialize pass: run Setup-Thurea.ps1 after WinUtil's own steps.
$ua = "$iso\autounattend.xml"
$U = 'urn:schemas-microsoft-com:unattend'
$doc = [xml]::new(); $doc.PreserveWhitespace = $true; $doc.Load($ua)
$ns = New-Object System.Xml.XmlNamespaceManager($doc.NameTable); $ns.AddNamespace('u', $U)
$rs = $doc.SelectSingleNode("/u:unattend/u:settings[@pass='specialize']/u:component[@name='Microsoft-Windows-Deployment']/u:RunSynchronous", $ns)
if (-not $rs) { throw 'autounattend.xml has no specialize RunSynchronous' }
$cmdLine = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "C:\Windows\Setup\Thurea\Setup-Thurea.ps1"'
if (-not ($rs.SelectNodes('u:RunSynchronousCommand/u:Path', $ns) | Where-Object { $_.InnerText -eq $cmdLine })) {
    $max = ($rs.SelectNodes('u:RunSynchronousCommand/u:Order', $ns) | ForEach-Object { [int]$_.InnerText } | Measure-Object -Maximum).Maximum
    $c = $doc.CreateElement('RunSynchronousCommand', $U)
    $a = $doc.CreateAttribute('wcm', 'action', 'http://schemas.microsoft.com/WMIConfig/2002/State'); $a.Value = 'add'; [void]$c.Attributes.Append($a)
    $o = $doc.CreateElement('Order', $U); $o.InnerText = [string]($max + 1); [void]$c.AppendChild($o)
    $p = $doc.CreateElement('Path', $U); $p.InnerText = $cmdLine; [void]$c.AppendChild($p)
    [void]$rs.AppendChild($c)
    [System.IO.File]::WriteAllText($ua, $doc.OuterXml, [System.Text.UTF8Encoding]::new($false))
    Log "autounattend.xml: specialize step $($max + 1) runs Setup-Thurea.ps1"
}
[void]([xml](Get-Content $ua -Raw))
