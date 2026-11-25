# Runs inside the throwaway build VM. Takes the Win11 Creator ISO (attached as a DVD), keeps its
# setup customisations (autounattend.xml, ei.cfg, $OEM$ scripts) unchanged, and refreshes the
# Windows 11 Pro image following Microsoft's media-update steps:
#   export Pro -> mount -> add the cumulative update (checkpoint CU found in the same folder)
#   -> add drivers -> component cleanup -> commit -> export.
# Then it puts Thurea on the media (stage-thurea.ps1), makes Setup skip every screen except the
# disk picker and the account screen (patch-unattend.ps1), rebuilds the ISO with WinUtil's
# oscdimg command line and powers off.
$ErrorActionPreference = 'Stop'
$X = (Get-Volume -FileSystemLabel XFER).DriveLetter + ':'
Start-Transcript -Path "$X\build.log" -Force
function Log($m) { Write-Host "[$((Get-Date).ToString('HH:mm:ss'))] $m" }
function Dism { & dism.exe /English @args; if ($LASTEXITCODE -ne 0) { throw "dism $($args -join ' ') failed with exit code $LASTEXITCODE" } }
try {
    # This VM was also used to test Thurea: switch that copy off so it doesn't touch the build.
    Get-ScheduledTask -TaskPath '\Thurea\' -ErrorAction SilentlyContinue | Disable-ScheduledTask -ErrorAction SilentlyContinue | Out-Null
    $hf = "$env:WINDIR\System32\drivers\etc\hosts"
    try { if ((Get-Content $hf -Raw) -match "thurea") { Set-Content $hf "# hosts`r`n" -Encoding ASCII -ErrorAction Stop } } catch { Log "hosts left as is: $_" }
    $src = Get-Volume | Where-Object { $_.DriveType -eq 'CD-ROM' -and $_.DriveLetter -and (Test-Path "$($_.DriveLetter):\sources\install.wim") } | Select-Object -First 1
    if (-not $src) { throw 'Win11 Creator ISO drive not found' }
    $drive = "$($src.DriveLetter):"
    $W = "C:\work-$(Get-Date -Format 'yyyyMMdd-HHmmss')"; $iso = "$W\iso"; $mount = "$W\mount"; $scratch = "$W\scratch"
    foreach ($m in Get-WindowsImage -Mounted -ErrorAction SilentlyContinue) {
        Log "Discarding image left mounted at $($m.Path)"
        & dism.exe /English /Unmount-Image /MountDir:"$($m.Path)" /Discard | Out-Null
    }
    & dism.exe /English /Cleanup-Mountpoints | Out-Null
    # Old work folders (best effort: update scratch files belong to TrustedInstaller).
    foreach ($old in Get-ChildItem 'C:\' -Directory -Filter 'work*' -ErrorAction SilentlyContinue) {
        Log "Removing old $($old.FullName) (best effort)"
        $eap = $ErrorActionPreference; $ErrorActionPreference = 'SilentlyContinue'
        try {
            cmd.exe /c "takeown /f `"$($old.FullName)`" /r /d y >nul 2>&1 & icacls `"$($old.FullName)`" /grant *S-1-5-32-544:F /T /C /Q >nul 2>&1 & rd /s /q `"$($old.FullName)`" >nul 2>&1"
        } catch {} finally { $ErrorActionPreference = $eap }
    }
    New-Item -ItemType Directory -Force -Path $iso, $mount, $scratch | Out-Null

    Log "Copying $drive to $iso (without install.wim)"
    & robocopy $drive $iso /E /NFL /NDL /NJH /NJS /XF install.wim | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE)" }
    Get-ChildItem $iso -Recurse -File | Where-Object IsReadOnly | ForEach-Object { $_.IsReadOnly = $false }

    # The Win11 Creator answer file installs image index 6 (Pro); find it by edition instead of trusting the number.
    $images = Get-WindowsImage -ImagePath "$drive\sources\install.wim"
    $pro = $images | Where-Object { (Get-WindowsImage -ImagePath "$drive\sources\install.wim" -Index $_.ImageIndex).EditionId -eq 'Professional' } | Select-Object -First 1
    Log "Source Pro image: index $($pro.ImageIndex) $($pro.ImageName)"
    Dism /Export-Image /SourceImageFile:"$drive\sources\install.wim" /SourceIndex:$($pro.ImageIndex) /DestinationImageFile:"$W\pro.wim" /Compress:max /CheckIntegrity

    Dism /Mount-Image /ImageFile:"$W\pro.wim" /Index:1 /MountDir:$mount
    try {
        $before = (Get-ItemProperty "$mount\Windows\System32\ntoskrnl.exe").VersionInfo.ProductVersion
        Log "Image build before update: $before"
        $lcu = Get-ChildItem "$X\updates" -Filter 'windows11.0-kb5132057-x64*.msu' | Select-Object -First 1
        Log "Adding $($lcu.Name) (checkpoint update in the same folder is applied first by DISM)"
        Dism /Image:$mount /Add-Package /PackagePath:"$($lcu.FullName)" /ScratchDir:$scratch
        Log 'Adding drivers'
        Dism /Image:$mount /Add-Driver /Driver:"$X\drivers" /Recurse
        Dism /Image:$mount /Get-Drivers /Format:Table
        Log 'Component cleanup'
        Dism /Image:$mount /Cleanup-Image /StartComponentCleanup /ResetBase /ScratchDir:$scratch
        $reg = "HKLM\BUILDSOFT"
        & reg.exe load $reg "$mount\Windows\System32\config\SOFTWARE" | Out-Null
        $cv = Get-ItemProperty "Registry::$reg\Microsoft\Windows NT\CurrentVersion"
        Log "Image build after update: $($cv.CurrentBuild).$($cv.UBR) ($($cv.DisplayVersion))"
        Set-Content "$X\BUILD" "$($cv.CurrentBuild).$($cv.UBR)"
        Remove-Variable cv; [gc]::Collect()
        & reg.exe unload $reg | Out-Null
        Dism /Unmount-Image /MountDir:$mount /Commit
    } catch {
        & dism.exe /Unmount-Image /MountDir:$mount /Discard | Out-Null
        throw
    }
    Dism /Export-Image /SourceImageFile:"$W\pro.wim" /SourceIndex:1 /DestinationImageFile:"$iso\sources\install.wim" /Compress:max /CheckIntegrity
    Get-WindowsImage -ImagePath "$iso\sources\install.wim" | Format-List ImageIndex, ImageName, ImageSize

    # install.wim now holds only Pro, at index 1: point the answer file's image index at it.
    $ua = "$iso\autounattend.xml"
    $doc = [xml]::new(); $doc.PreserveWhitespace = $true; $doc.Load($ua)
    $ns = New-Object System.Xml.XmlNamespaceManager($doc.NameTable); $ns.AddNamespace('u', 'urn:schemas-microsoft-com:unattend')
    $v = $doc.SelectNodes('//u:InstallFrom/u:MetaData[u:Key="/IMAGE/INDEX"]/u:Value', $ns)
    if ($v.Count -ne 1) { throw "expected one /IMAGE/INDEX in autounattend.xml, found $($v.Count)" }
    Log "autounattend.xml image index $($v[0].InnerText) -> 1"
    $v[0].InnerText = '1'
    [System.IO.File]::WriteAllText($ua, $doc.OuterXml, [System.Text.UTF8Encoding]::new($false))

    . "$X\patch-unattend.ps1"
    . "$X\stage-thurea.ps1"
    $build = (Get-Content "$X\BUILD").Trim()
    $out = "$X\Win11_Pro_$($build)_Modified.iso"
    $bootData = "2#p0,e,b`"$iso\boot\etfsboot.com`"#pEF,e,b`"$iso\efi\microsoft\boot\efisys.bin`""
    $oscArgs = @('-m', '-o', '-u2', '-udfver102', "-bootdata:$bootData", '-l"CTOS_MODIFIED"', "`"$iso`"", "`"$out`"")
    Log "Running oscdimg"
    $p = Start-Process -FilePath "$X\oscdimg.exe" -ArgumentList ($oscArgs -join ' ') -Wait -PassThru -NoNewWindow
    if ($p.ExitCode -ne 0) { throw "oscdimg exited with code $($p.ExitCode)" }
    $hash = (Get-FileHash $out -Algorithm SHA256).Hash
    Log "ISO: $out ($((Get-Item $out).Length) bytes, SHA256 $hash)"
    Set-Content -Path "$X\DONE" -Value "$(Split-Path $out -Leaf) $hash"
} catch {
    Write-Host "FAILED: $_"; Write-Host $_.ScriptStackTrace
    Set-Content -Path "$X\FAILED" -Value "$_"
} finally {
    Stop-Transcript
    Stop-Computer -Force
}
