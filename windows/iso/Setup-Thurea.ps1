# Runs once during Windows Setup (specialize pass, as SYSTEM) on PCs installed from the Thurea
# Windows 11 ISO. Copied there as C:\Windows\Setup\Thurea by the ISO's $OEM$ folder.
#   1. Helium installed for all users and made the default browser for every new account
#   +  the GPU driver for this PC's graphics card (Intel Arc, NVIDIA or AMD) from the install media
#   2. Microsoft Edge removed (WebView2, which Windows and apps use internally, stays)
#   3. Thurea installed (browsers, Thurea extension, tray app)
$ErrorActionPreference = 'Stop'
$Here = $PSScriptRoot
Start-Transcript -Path (Join-Path $Here 'setup-thurea.log') -Append -Force | Out-Null
function Log($m) { Write-Host "[$((Get-Date).ToString('HH:mm:ss'))] $m" }

# The account screen asks only for a username and password: no security questions.
try {
    New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -Force | Out-Null
    Set-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -Name NoLocalPasswordResetQuestions -Value 1 -Type DWord
    Log 'Local accounts: no security questions'
} catch { Log "security questions setting: $_" }

try {
    # ---- 1. Helium ----
    $installer = Get-ChildItem $Here -Filter 'helium_*_x64-mini-installer.exe' | Select-Object -First 1
    if ($installer) {
        Log "Installing $($installer.Name)"
        $p = Start-Process -FilePath $installer.FullName -ArgumentList '--system-level --do-not-launch-chrome --verbose-logging' -Wait -PassThru
        if (Test-Path "$env:TEMP\helium_installer.log") { Copy-Item "$env:TEMP\helium_installer.log" $Here -Force }
        Log "Helium installer exit code $($p.ExitCode)"
    }
    # Helium's registration (ProgIDs) as the installer left it, so the defaults match exactly.
    $client = Get-ChildItem 'HKLM:\SOFTWARE\Clients\StartMenuInternet' -ErrorAction SilentlyContinue |
        Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).'(default)' -match 'Helium' -or $_.PSChildName -match 'Helium' } | Select-Object -First 1
    if (-not $client) { throw 'Helium did not register as a browser' }
    $cap = Join-Path $client.PSPath 'Capabilities'
    $urls = Get-ItemProperty (Join-Path $cap 'URLAssociations')
    $files = Get-ItemProperty (Join-Path $cap 'FileAssociations')
    $appName = (Get-ItemProperty $cap).ApplicationName
    Log "Helium registered as '$($client.PSChildName)' ($appName)"
    $assoc = New-Object System.Collections.Generic.List[string]
    foreach ($k in 'http', 'https', 'ftp', 'mailto') { if ($urls.$k) { $assoc.Add("  <Association Identifier=`"$k`" ProgId=`"$($urls.$k)`" ApplicationName=`"$appName`" />") } }
    foreach ($k in '.htm', '.html', '.shtml', '.xht', '.xhtml', '.svg', '.webp', '.pdf') { if ($files.$k) { $assoc.Add("  <Association Identifier=`"$k`" ProgId=`"$($files.$k)`" ApplicationName=`"$appName`" />") } }
    $xml = "<?xml version=`"1.0`" encoding=`"UTF-8`"?>`r`n<DefaultAssociations>`r`n" + ($assoc -join "`r`n") + "`r`n</DefaultAssociations>`r`n"
    $xmlPath = Join-Path $env:ProgramData 'Thurea\default-browser.xml'
    New-Item -ItemType Directory -Force -Path (Split-Path $xmlPath) | Out-Null
    [IO.File]::WriteAllText($xmlPath, $xml, (New-Object Text.UTF8Encoding $false))
    & dism.exe /Online /English /Import-DefaultAppAssociations:"$xmlPath" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "default app associations failed ($LASTEXITCODE)" }
    Log "Helium is the default browser for new accounts ($($assoc.Count) associations)"

    # Pin Helium on the taskbar for new accounts (WinUtil's layout starts empty).
    $lnk = Get-ChildItem "$env:ProgramData\Microsoft\Windows\Start Menu\Programs" -Filter 'Helium*.lnk' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($lnk) {
        $tb = @"
<?xml version="1.0" encoding="utf-8"?>
<LayoutModificationTemplate xmlns="http://schemas.microsoft.com/Start/2014/LayoutModification" xmlns:defaultlayout="http://schemas.microsoft.com/Start/2014/FullDefaultLayout" xmlns:taskbar="http://schemas.microsoft.com/Start/2014/TaskbarLayout" Version="1">
  <CustomTaskbarLayoutCollection PinListPlacement="Replace">
    <defaultlayout:TaskbarLayout>
      <taskbar:TaskbarPinList>
        <taskbar:DesktopApp DesktopApplicationLinkPath="$($lnk.FullName)" />
      </taskbar:TaskbarPinList>
    </defaultlayout:TaskbarLayout>
  </CustomTaskbarLayoutCollection>
</LayoutModificationTemplate>
"@
        $tbPath = Join-Path $env:ProgramData 'Thurea\taskbar-layout.xml'
        [IO.File]::WriteAllText($tbPath, $tb, (New-Object Text.UTF8Encoding $false))
        New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -Force | Out-Null
        Set-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -Name StartLayoutFile -Value $tbPath
        Set-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -Name LockedStartLayout -Value 0 -Type DWord
        Log 'Helium pinned on the taskbar'
    }

} catch {
    Log "Helium: FAILED: $_"
    Log $_.ScriptStackTrace
}

try {
    # ---- GPU driver: only the one this PC needs, from the install media's thurea-gpu folder ----
    $media = Get-PSDrive -PSProvider FileSystem | Where-Object { Test-Path (Join-Path $_.Root 'thurea-gpu\gpu.txt') } | Select-Object -First 1
    if (-not $media) { Log 'GPU: no thurea-gpu folder on the install media; Windows Update will provide the driver' }
    else {
        $gpuRoot = Join-Path $media.Root 'thurea-gpu'
        $vendors = [ordered]@{ '10DE' = 'nvidia'; '1002' = 'amd'; '8086' = 'intel-arc' }
        $found = Get-CimInstance Win32_PnPEntity -Filter "PNPClass='Display'" | Where-Object { $_.PNPDeviceID -match '^PCI\\VEN_([0-9A-F]{4})&DEV_([0-9A-F]{4})' }
        $done = @{}
        foreach ($d in $found) {
            [void]($d.PNPDeviceID -match '^PCI\\VEN_([0-9A-F]{4})&DEV_([0-9A-F]{4})')
            $ven = $Matches[1]; $dev = $Matches[2]
            Log "GPU: $($d.Name) (VEN_$ven DEV_$dev)"
            $pkg = $vendors[$ven]
            if (-not $pkg -or $done[$pkg]) { continue }
            $dir = Join-Path $gpuRoot $pkg
            if (-not (Test-Path $dir)) { Log "GPU: no package for vendor $ven"; continue }
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $out = & pnputil.exe /add-driver (Join-Path $dir '*.inf') /subdirs /install 2>&1
            $done[$pkg] = $true
            Log "GPU: installed the $pkg package in $([int]$sw.Elapsed.TotalSeconds) s (pnputil exit $LASTEXITCODE)"
            $out | Select-String -Pattern 'Added driver packages|Failed|installed on' | ForEach-Object { Log "  $($_.Line.Trim())" }
        }
        if (-not $found) { Log 'GPU: no PCI display adapter found' }
    }
} catch {
    Log "GPU: FAILED: $_"
}

try {
    # ---- 2 + 3. Thurea (removes Edge on this PC because of the marker) ----
    New-Item -ItemType Directory -Force -Path "$env:ProgramData\Thurea" | Out-Null
    Set-Content -Path "$env:ProgramData\Thurea\remove-edge" -Value 'Set up from the Thurea Windows 11 ISO: Microsoft Edge stays removed.'
    & "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here 'Thurea\Thurea.ps1') -Install
    if ($LASTEXITCODE) { throw "Thurea install failed ($LASTEXITCODE)" }
    Log 'Thurea installed'
} catch {
    Log "FAILED: $_"
    Log $_.ScriptStackTrace
} finally {
    Stop-Transcript | Out-Null
    # The Helium installer isn't needed any more; keep the log.
    Get-ChildItem $Here -Filter 'helium_*installer.exe' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
}
