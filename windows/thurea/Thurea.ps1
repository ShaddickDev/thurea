#Requires -Version 5.1
<#
Thurea for Windows - the Windows counterpart of thurea on Omarchy.

  Thurea.ps1 -Install   copy Thurea to Program Files, register its tasks, apply (Administrator)
  Thurea.ps1 -Apply     (re)apply every protection; run by the "Thurea" task every 5 minutes
                             and at startup
  Thurea.ps1 -Serve     serve the Thurea extension's updates to browsers on 127.0.0.1:8137
  Thurea.ps1 -Status    print what is and isn't in place
  Thurea.ps1 -Update    install the newest signed Thurea release from GitHub (hourly "Thurea Update" task)

What it keeps in place (like /usr/lib/thurea/thurea-apply on Linux):
  hosts file      only if data\hosts-block is shipped (a list this size stalls Windows' DNS Client)
  browsers        Chrome, Chromium, Edge, Brave, Vivaldi, Helium (and any of them installed later): Thurea
                  force-installed and locked on, SafeSearch, no incognito or guest windows.
                  Firefox family: private browsing off, harmful sites blocked.
  managed         this PC is marked as managed, which Chromium browsers require before they accept
                  an extension that isn't from the Chrome Web Store
  blocked apps    Tor Browser, Mullvad Browser and Opera can't run
  Edge            stays removed (PCs set up from the Thurea Windows ISO only)
  uninstall block Thurea's tasks put each other back if one is deleted or disabled, and Thurea shows
                  in Settings > Apps without an Uninstall button

What it never touches: DNS. Not the adapters' DNS servers, not Windows' DNS over HTTPS, not the
browsers' secure DNS, and no firewall rule on DNS traffic. Forcing family DNS left PCs with no
internet on networks that can't reach it (YouTube and Twitch first, then everything), so
filtering is the extension's, SafeSearch's and the blocked browsers' job. Versions up to 1.0.5 did
change DNS: Restore-Dns puts back what they changed (see below).
#>
[CmdletBinding(DefaultParameterSetName = 'Apply')]
param(
    [Parameter(ParameterSetName = 'Install')][switch]$Install,
    [Parameter(ParameterSetName = 'Install')][string]$Package,
    [Parameter(ParameterSetName = 'Update')][switch]$Update,
    [Parameter(ParameterSetName = 'Update')][switch]$CheckOnly,
    [Parameter(ParameterSetName = 'Apply')][switch]$Apply,
    [Parameter(ParameterSetName = 'Serve')][switch]$Serve,
    [Parameter(ParameterSetName = 'Status')][switch]$Status
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

$Home_ = Join-Path $env:ProgramFiles 'Thurea'
$Data = Join-Path $Home_ 'data'
$Web = Join-Path $Home_ 'web'
$State = Join-Path $env:ProgramData 'Thurea'
$Port = 8137
$ExtId = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'data\extension-id') -Raw).Trim()
$HostsFile = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'
$Begin = '# >>> thurea (Thurea blocker - managed by the system, do not edit)'
$End = '# <<< thurea'

# What versions up to 1.0.5 set, so Restore-Dns can recognise and undo it.
$FamilyV4 = @('1.1.1.3', '1.0.0.3')
$FamilyV6 = @('2606:4700:4700::1113', '2606:4700:4700::1003')
$OldDnsRules = @('Thurea - public DNS-over-HTTPS servers', 'Thurea - other DNS servers (UDP)', 'Thurea - other DNS servers (TCP)')

# HKLM policy keys of the Chromium browsers (same policy names in all of them).
$Chromium = [ordered]@{
    'Chrome'   = 'SOFTWARE\Policies\Google\Chrome'
    'Chromium' = 'SOFTWARE\Policies\Chromium'
    'Edge'     = 'SOFTWARE\Policies\Microsoft\Edge'
    'Brave'    = 'SOFTWARE\Policies\BraveSoftware\Brave'
    'Vivaldi'  = 'SOFTWARE\Policies\Vivaldi'
    'Helium'   = 'SOFTWARE\Policies\Helium'
    'Thorium'  = 'SOFTWARE\Policies\Thorium'
    'Yandex'   = 'SOFTWARE\Policies\YandexBrowser'
}
# Browsers that may not run at all (as on Linux, where pacman refuses to install them).
$Blocked = @('tor.exe', 'mullvadbrowser.exe', 'opera.exe', 'opera_gx_splash.exe', 'opera_autoupdate.exe')
# Firefox family executables: Thurea writes distribution\policies.json next to each one found.
$FirefoxExes = @('firefox.exe', 'librewolf.exe', 'waterfox.exe', 'floorp.exe', 'zen.exe', 'palemoon.exe')

$EdgeStable = '{56EB18F8-B008-4CBD-B6D2-8C97FE7E9062}'
$Enroll = 'FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF'
$Repo = 'ShaddickDev/thurea'

function Log([string]$m) {
    $line = "[$((Get-Date).ToString('yyyy-MM-dd HH:mm:ss'))] $m"
    Write-Host $line
    try {
        $log = Join-Path $State 'thurea.log'
        if ((Test-Path $log) -and (Get-Item $log).Length -gt 1MB) { Move-Item $log "$log.1" -Force }
        Add-Content -LiteralPath $log -Value $line -Encoding UTF8
    } catch {}
}

function Assert-Admin {
    $p = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run this as Administrator.' }
}

# ---------- registry helpers ----------
function Set-Reg([string]$key, [string]$name, $value, [string]$type) {
    $path = "Registry::HKEY_LOCAL_MACHINE\$key"
    if (-not (Test-Path -LiteralPath $path)) { New-Item -Path $path -Force | Out-Null }
    $cur = $null
    try { $cur = (Get-ItemProperty -LiteralPath $path -Name $name -ErrorAction Stop).$name } catch {}
    if ($null -ne $cur -and "$cur" -eq "$value") { return $false }
    New-ItemProperty -LiteralPath $path -Name $name -Value $value -PropertyType $type -Force | Out-Null
    return $true
}
function Get-Reg([string]$key, [string]$name) {
    try { return (Get-ItemProperty -LiteralPath "Registry::HKEY_LOCAL_MACHINE\$key" -Name $name -ErrorAction Stop).$name } catch { return $null }
}
function Remove-Reg([string]$key, [string]$name) {
    if ($null -eq (Get-Reg $key $name)) { return $false }
    Remove-ItemProperty -LiteralPath "Registry::HKEY_LOCAL_MACHINE\$key" -Name $name -Force
    return $true
}

# ---------- managed marker ----------
# Chromium accepts off-store force-installed extensions on Windows only when the machine reports
# a device-management enrollment (IsDeviceRegisteredWithManagement). These entries are what an
# MDM enrollment leaves behind; there is no server and nothing is sent anywhere.
# A PC that is really enrolled (Intune or another MDM, "Access work or school") already counts as
# managed, so Thurea leaves the enrollment keys alone there: writing a second, empty enrollment
# next to a real one could confuse the real MDM client.
function Get-RealEnrollment {
    foreach ($k in Get-ChildItem 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Enrollments' -ErrorAction SilentlyContinue) {
        if ($k.PSChildName -eq $Enroll -or $k.PSChildName -notmatch '^\{?[0-9A-Fa-f-]{36}\}?$') { continue }
        $p = Get-ItemProperty -LiteralPath $k.PSPath -ErrorAction SilentlyContinue
        if ($p -and ($p.PSObject.Properties.Name -contains 'ProviderID') -and "$($p.ProviderID)" -and
            ($p.PSObject.Properties.Name -contains 'EnrollmentState') -and $p.EnrollmentState -eq 1) { return "$($p.ProviderID)" }
    }
    return $null
}
function Test-Managed {
    if (Get-RealEnrollment) { return $true }
    return (Get-Reg "SOFTWARE\Microsoft\Enrollments\$Enroll" 'EnrollmentState') -eq 1
}
function Set-Managed {
    $real = Get-RealEnrollment
    if ($real) {
        if (Test-Path "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Enrollments\$Enroll") {
            Remove-Item "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Enrollments\$Enroll" -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Provisioning\OMADM\Accounts\$Enroll" -Recurse -Force -ErrorAction SilentlyContinue
            Log "This PC is enrolled with $($real): Thurea's own managed marker removed (not needed)"
        }
        return
    }
    $c = $false
    $c = (Set-Reg "SOFTWARE\Microsoft\Enrollments\$Enroll" 'EnrollmentState' 1 'DWord') -or $c
    $c = (Set-Reg "SOFTWARE\Microsoft\Enrollments\$Enroll" 'EnrollmentType' 0 'DWord') -or $c
    $c = (Set-Reg "SOFTWARE\Microsoft\Enrollments\$Enroll" 'IsFederated' 0 'DWord') -or $c
    $a = "SOFTWARE\Microsoft\Provisioning\OMADM\Accounts\$Enroll"
    $c = (Set-Reg $a 'Flags' 0xd6fb7f 'DWord') -or $c
    $c = (Set-Reg $a 'AcctUId' '0x000000000000000000000000000000000000000000000000000000000000000000000000' 'String') -or $c
    $c = (Set-Reg $a 'RoamingCount' 0 'DWord') -or $c
    $c = (Set-Reg $a 'SslClientCertReference' 'MY;User;0000000000000000000000000000000000000000' 'String') -or $c
    $c = (Set-Reg $a 'ProtoVer' '1.2' 'String') -or $c
    if ($c) { Log 'Marked this PC as managed (browsers now accept Thurea as a forced extension)' }
}

# ---------- hosts ----------
function Set-Hosts {
    # A 100,000-line hosts file makes Windows' DNS Client spend minutes of CPU at every start
    # (measured: 400 s with no name resolution meanwhile), so on Windows the list is optional:
    # the Thurea extension does the blocking. If
    # data\hosts-block is present it is used; otherwise any old Thurea section is removed.
    $src = Join-Path $Data 'hosts-block'
    if (-not (Test-Path -LiteralPath $src)) {
        $cur = if (Test-Path $HostsFile) { [IO.File]::ReadAllText($HostsFile) -replace "`r", '' } else { '' }
        if ($cur.Contains('# >>> thurea')) {
            $mine = [regex]::Replace($cur, "(?s)\n?# >>> thurea.*?(# <<< thurea\n?|$)", "`n").TrimEnd("`n") + "`n"
            [IO.File]::WriteAllText($HostsFile, ($mine -replace "`n", "`r`n"), [Text.Encoding]::ASCII)
            Log 'hosts: old Thurea block list removed'
        }
        return
    }
    $block = Get-Content -LiteralPath $src -Raw
    $block = ($block -replace "`r", '').TrimEnd("`n")
    if (-not $block.StartsWith($Begin)) { $block = "$Begin`n$($block -replace '^# >>> thurea.*\n', '')" }
    if (-not $block.EndsWith($End)) { $block = "$block`n$End" }
    $cur = if (Test-Path $HostsFile) { [IO.File]::ReadAllText($HostsFile) -replace "`r", '' } else { '' }
    # Keep the user's own lines: everything outside Thurea's section.
    $mine = [regex]::Replace($cur, "(?s)\n?$([regex]::Escape($Begin)).*?$([regex]::Escape($End))\n?", "`n")
    $mine = [regex]::Replace($mine, "(?s)\n?# >>> thurea.*$", '')   # an unterminated old section
    $want = ($mine.TrimEnd("`n") + "`n`n" + $block + "`n").TrimStart("`n")
    if ($cur -ne $want) {
        Set-ItemProperty -LiteralPath $HostsFile -Name IsReadOnly -Value $false -ErrorAction SilentlyContinue
        [IO.File]::WriteAllText($HostsFile, ($want -replace "`n", "`r`n"), [Text.Encoding]::ASCII)
        Log 'hosts: Thurea block list written'
        Clear-DnsClientCache
    }
    $acl = Get-Acl $HostsFile
    $sddl = 'O:SYG:SYD:PAI(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)(A;;0x1200a9;;;AC)'
    if ($acl.Sddl -ne $sddl) { $acl.SetSecurityDescriptorSddlForm($sddl); Set-Acl -LiteralPath $HostsFile -AclObject $acl }
}

# ---------- DNS: never changed, and put back after older versions ----------
# Up to 1.0.5 Thurea switched every adapter to Cloudflare for Families, turned on Windows' DNS over
# HTTPS for it, and firewalled every other DNS server. On networks that can't reach 1.1.1.3 that
# cut the PC off the internet. Thurea no longer changes DNS at all; this undoes, once, what an older
# version left behind. Only Thurea's own settings are touched: an adapter goes back to the network's
# own DNS only if it is set to exactly the family servers Thurea set. When everything is back,
# ProgramData\Thurea\dns-restored is written and Thurea never looks at DNS again.
function Restore-Dns {
    $done = Join-Path $State 'dns-restored'
    if (Test-Path -LiteralPath $done) { return }
    $ok = $true
    # Firewall: the DNS lock first, so the network's own DNS is reachable before adapters use it.
    foreach ($name in $OldDnsRules) {
        try {
            $r = @(Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue)
            if ($r.Count) { $r | Remove-NetFirewallRule -ErrorAction Stop; Log "DNS: old firewall rule '$name' removed" }
        } catch { $ok = $false; Log "DNS: couldn't remove firewall rule '$name': $_" }
    }
    # Adapters.
    $reset = 0
    foreach ($a in @(Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceDescription -notmatch 'Loopback' })) {
        try {
            $v4 = @((Get-DnsClientServerAddress -InterfaceIndex $a.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue).ServerAddresses)
            $v6 = @((Get-DnsClientServerAddress -InterfaceIndex $a.ifIndex -AddressFamily IPv6 -ErrorAction SilentlyContinue).ServerAddresses)
            if (($v4 -join ',') -eq ($FamilyV4 -join ',') -or ($v6 -join ',') -eq ($FamilyV6 -join ',')) {
                Set-DnsClientServerAddress -InterfaceIndex $a.ifIndex -ResetServerAddresses -ErrorAction Stop
                Log "DNS: $($a.Name) back to the network's own DNS"
                $reset++
            }
        } catch { $ok = $false; Log "DNS: couldn't put $($a.Name) back to the network's own DNS: $_" }
    }
    # Windows' DNS over HTTPS policy (the DoH entries for 1.1.1.3 / 1.0.0.3 stay: they do nothing
    # unless someone chooses those servers, and then they make them encrypted).
    try {
        if ((Get-Reg 'SOFTWARE\Policies\Microsoft\Windows NT\DNSClient' 'DoHPolicy') -eq 2) {
            [void](Remove-Reg 'SOFTWARE\Policies\Microsoft\Windows NT\DNSClient' 'DoHPolicy')
            Log 'DNS: Windows DNS over HTTPS policy removed'
        }
    } catch { $ok = $false; Log "DNS: couldn't remove the DNS over HTTPS settings: $_" }
    if ($reset) { Clear-DnsClientCache -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath (Join-Path $State 'family-dns.failed') -Force -ErrorAction SilentlyContinue
    if ($ok) {
        [IO.File]::WriteAllText($done, (Get-Date).ToString('o'))
        Log 'DNS: Thurea no longer changes DNS; anything an older version set has been put back'
    }
}

# ---------- browsers ----------
function Set-ChromiumPolicy([string]$key) {
    $c = $false
    $fl = "$key\ExtensionInstallForcelist"
    $want = "$ExtId;http://127.0.0.1:$Port/updates.xml"
    $p = Get-ItemProperty -LiteralPath "Registry::HKEY_LOCAL_MACHINE\$fl" -ErrorAction SilentlyContinue
    $have = @(if ($p) { $p.PSObject.Properties | Where-Object { $_.Name -match '^\d+$' } | ForEach-Object { "$($_.Value)" } })
    if ($have -notcontains $want) {
        $n = 1; while ($have.Count -ge $n -and (Get-Reg $fl "$n")) { $n++ }
        [void](Set-Reg $fl "$n" $want 'String'); $c = $true
    }
    $settings = [ordered]@{
        $ExtId = [ordered]@{ installation_mode = 'force_installed'; update_url = "http://127.0.0.1:$Port/updates.xml"; override_update_url = $true }
        'cackkknkjnchcemedibohdnlejfgdben' = @{ installation_mode = 'removed' }
        'jinjaccalgkegednnccohejagnlnfdag' = @{ installation_mode = 'removed' }
    }
    $c = (Set-Reg $key 'ExtensionSettings' ($settings | ConvertTo-Json -Compress -Depth 4) 'String') -or $c
    $c = (Set-Reg "$key\3rdparty\extensions\$ExtId\policy" 'pornBlock' 1 'DWord') -or $c
    $c = (Set-Reg $key 'ForceGoogleSafeSearch' 1 'DWord') -or $c
    # The browser's own secure DNS is left to the browser (versions up to 1.0.5 locked it to
    # family DNS, which left the browser with no internet where that can't be reached).
    $c = (Remove-Reg $key 'DnsOverHttpsMode') -or $c
    $c = (Remove-Reg $key 'DnsOverHttpsTemplates') -or $c
    $c = (Set-Reg $key 'IncognitoModeAvailability' 1 'DWord') -or $c
    $c = (Set-Reg $key 'InPrivateModeAvailability' 1 'DWord') -or $c
    $c = (Set-Reg $key 'BrowserGuestModeEnabled' 0 'DWord') -or $c
    return $c
}
# When Thurea last changed a browser policy: profiles used after that (plus a grace period)
# must have the extension (see Test-ExtensionInstalled).
function Set-Since([string]$what) {
    New-Item -ItemType Directory -Force -Path $State | Out-Null
    [IO.File]::WriteAllText((Join-Path $State "policy-$what.since"), (Get-Date).ToString('o'))
}
function Get-Since([string]$what) {
    $f = Join-Path $State "policy-$what.since"
    if (-not (Test-Path $f)) { Set-Since $what }
    return (Get-Item $f).LastWriteTime
}

# Is the Thurea extension really there? Every browser profile (Chromium family: <profile>\Extensions\<id>,
# Firefox family: <profile>\extensions\<add-on id>.xpi) used in the last 30 days, and since Thurea set
# the policy (10 minutes' grace, so the browser has seen it), must have it. Returns the ones without.
function Test-ExtensionInstalled {
    $ext = (Get-Content (Join-Path $Data 'extension-id') -ErrorAction SilentlyContinue | Select-Object -First 1)
    $gecko = (Get-Content (Join-Path $Data 'firefox-id') -ErrorAction SilentlyContinue | Select-Object -First 1)
    $chSince = (Get-Since 'chromium').AddMinutes(10); $ffSince = (Get-Since 'firefox').AddMinutes(10)
    $recent = (Get-Date).AddDays(-30)
    $chromium = @('Google\Chrome', 'Google\Chrome Beta', 'Microsoft\Edge', 'BraveSoftware\Brave-Browser', 'Vivaldi', 'Chromium', 'imput\Helium', 'Thorium', 'Yandex\YandexBrowser')
    $firefox = @('Mozilla\Firefox', 'LibreWolf', 'Waterfox', 'zen', 'Floorp')
    $missing = New-Object System.Collections.Generic.List[string]
    foreach ($u in Get-ChildItem 'C:\Users' -Directory -ErrorAction SilentlyContinue) {
        if ($ext) {
            foreach ($b in $chromium) {
                foreach ($p in Get-ChildItem (Join-Path $u.FullName "AppData\Local\$b\User Data") -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile *' }) {
                    $pref = Get-Item (Join-Path $p.FullName 'Preferences') -ErrorAction SilentlyContinue
                    if (-not $pref -or $pref.LastWriteTime -lt $recent -or $pref.LastWriteTime -lt $chSince) { continue }
                    if (-not (Test-Path (Join-Path $p.FullName "Extensions\$($ext.Trim())"))) { $missing.Add($p.FullName) }
                }
            }
        }
        if ($gecko) {
            foreach ($b in $firefox) {
                foreach ($p in Get-ChildItem (Join-Path $u.FullName "AppData\Roaming\$b\Profiles") -Directory -ErrorAction SilentlyContinue) {
                    $pref = Get-Item (Join-Path $p.FullName 'prefs.js') -ErrorAction SilentlyContinue
                    if (-not $pref -or $pref.LastWriteTime -lt $recent -or $pref.LastWriteTime -lt $ffSince) { continue }
                    if (-not (Test-Path (Join-Path $p.FullName "extensions\$($gecko.Trim()).xpi"))) { $missing.Add($p.FullName) }
                }
            }
        }
    }
    return , $missing.ToArray()
}

function Set-Browsers {
    # Optional, and the sturdier way for Chrome: Chrome Browser Cloud Management (free, from a
    # Google Admin console). An enrolled Chrome accepts the Thurea extension without relying on
    # the managed marker. Put the enrollment token in ProgramData\Thurea\chrome-enrollment-token.
    $tokenFile = Join-Path $State 'chrome-enrollment-token'
    if (Test-Path $tokenFile) {
        $token = (Get-Content $tokenFile -Raw).Trim()
        if ($token -match '^[A-Za-z0-9-]{20,80}$' -and (Set-Reg 'SOFTWARE\Policies\Google\Chrome' 'CloudManagementEnrollmentToken' $token 'String')) {
            Log 'browsers: Chrome enrolled in Chrome Browser Cloud Management'
        }
    }
    foreach ($b in $Chromium.GetEnumerator()) {
        if (Set-ChromiumPolicy $b.Value) { Log "browsers: $($b.Key) policy set"; Set-Since 'chromium' }
    }
    # Firefox family: policies.json beside every installed copy (a registry policy would make Firefox
    # ignore policies.json, and the 37,000-site filter is too big for the registry).
    $ff = Get-Content -LiteralPath (Join-Path $Data 'firefox-policies.json') -Raw
    # The shared Firefox policy (Linux too) locks DNS over HTTPS to family DNS: on Windows Thurea
    # leaves DNS alone, so that part is taken out. Writing the file replaces an older version's lock.
    $ff = [regex]::Replace($ff, '"DNSOverHTTPS"\s*:\s*\{[^{}]*\}\s*,?\s*', '')
    $dirs = New-Object System.Collections.Generic.HashSet[string]([StringComparer]::OrdinalIgnoreCase)
    foreach ($root in @($env:ProgramFiles, ${env:ProgramFiles(x86)}) + @(Get-ChildItem 'C:\Users' -Directory -ErrorAction SilentlyContinue | ForEach-Object { Join-Path $_.FullName 'AppData\Local' })) {
        if (-not $root -or -not (Test-Path $root)) { continue }
        foreach ($d in Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue) {
            foreach ($exe in $FirefoxExes) {
                if (Test-Path (Join-Path $d.FullName $exe)) { [void]$dirs.Add($d.FullName) }
                foreach ($sub in Get-ChildItem -LiteralPath $d.FullName -Directory -ErrorAction SilentlyContinue) {
                    if (Test-Path (Join-Path $sub.FullName $exe)) { [void]$dirs.Add($sub.FullName) }
                }
            }
        }
    }
    foreach ($d in $dirs) {
        $f = Join-Path $d 'distribution\policies.json'
        $cur = if (Test-Path $f) { [IO.File]::ReadAllText($f) } else { $null }
        if ($cur -ne $ff) {
            New-Item -ItemType Directory -Force -Path (Split-Path $f) | Out-Null
            [IO.File]::WriteAllText($f, $ff, (New-Object Text.UTF8Encoding $false))
            Log "browsers: Firefox-family policy written to $f"; Set-Since 'firefox' 
        }
    }
    # Browsers that may not run.
    foreach ($exe in $Blocked) {
        [void](Set-Reg "SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe" 'Debugger' "$env:SystemRoot\System32\systray.exe" 'String')
    }
}

# ---------- Edge ----------
function Remove-Edge {
    [void](Set-Reg 'SOFTWARE\Microsoft\EdgeUpdate' 'DoNotUpdateToEdgeWithChromium' 1 'DWord')
    [void](Set-Reg 'SOFTWARE\Policies\Microsoft\EdgeUpdate' "Install$EdgeStable" 0 'DWord')
    [void](Set-Reg 'SOFTWARE\Policies\Microsoft\EdgeUpdate' 'CreateDesktopShortcutDefault' 0 'DWord')
    $dir = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application"
    $setup = Get-ChildItem "$dir\*\Installer\setup.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
    if ($setup) {
        # WinUtil's "Microsoft Edge - Remove": the official uninstaller, unlocked by the legacy Edge marker file.
        $marker = "$env:SystemRoot\SystemApps\Microsoft.MicrosoftEdge_8wekyb3d8bbwe\MicrosoftEdge.exe"
        New-Item -ItemType File -Path $marker -Force | Out-Null
        Get-Process msedge -ErrorAction SilentlyContinue | Stop-Process -Force
        Start-Process -FilePath $setup.FullName -ArgumentList '--uninstall --system-level --force-uninstall --delete-profile' -Wait
        Log 'Edge: removed'
    }
    foreach ($l in @("$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Microsoft Edge.lnk", "$env:PUBLIC\Desktop\Microsoft Edge.lnk")) {
        if (Test-Path $l) { Remove-Item $l -Force }
    }
    Get-ChildItem 'C:\Users\*\Desktop\Microsoft Edge.lnk', 'C:\Users\*\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Microsoft Edge.lnk' -ErrorAction SilentlyContinue | Remove-Item -Force
}

# ---------- update server task ----------
function Test-Server {
    try {
        $r = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$Port/updates.xml" -TimeoutSec 5
        return $r.StatusCode -eq 200 -and $r.Content -match $ExtId
    } catch { return $false }
}

function Write-Status {
    $checks = [ordered]@{}
    $h = if (Test-Path $HostsFile) { [IO.File]::ReadAllText($HostsFile) } else { '' }
    if (Test-Path (Join-Path $Data 'hosts-block')) { $checks.hosts = $h.Contains($Begin) }
    $checks.browsers = -not ($Chromium.Values | Where-Object { (Get-Reg $_ 'ForceGoogleSafeSearch') -ne 1 -or (Get-Reg $_ 'IncognitoModeAvailability') -ne 1 })
    $checks.managed = Test-Managed
    $checks.extensionServer = Test-Server
    $missing = @(Test-ExtensionInstalled)
    $checks.extensionInstalled = $missing.Count -eq 0
    if (Test-Path (Join-Path $State 'remove-edge')) { $checks.edgeRemoved = -not (Test-Path "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe") }
    $checks.tasks = Test-Tasks
    $checks.uninstallBlock = (Get-Reg 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Thurea' 'NoRemove') -eq 1
    $ok = -not ($checks.Values | Where-Object { -not $_ })
    $s = [ordered]@{ protected = $ok; checks = $checks; extensionMissing = @($missing | Select-Object -First 20); at = (Get-Date).ToString('o'); epoch = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds(); version = [string]("$(Get-Content (Join-Path $Home_ 'VERSION') -ErrorAction SilentlyContinue | Select-Object -First 1)".Trim()) }
    New-Item -ItemType Directory -Force -Path $State | Out-Null
    [IO.File]::WriteAllText((Join-Path $State 'status.json'), ($s | ConvertTo-Json -Depth 3), (New-Object Text.UTF8Encoding $false))
    return $s
}

function Invoke-Apply {
    Assert-Admin
    New-Item -ItemType Directory -Force -Path $State | Out-Null
    $steps = [ordered]@{ dns = { Restore-Dns }; managed = { Set-Managed }; hosts = { Set-Hosts }; browsers = { Set-Browsers }
        tasks = { Repair-Tasks }; appEntry = { Set-AppEntry }; startMenu = { Set-StartMenu }; tray = { Set-TrayAutostart } }
    # Edge stays removed only on PCs set up from the Thurea Windows ISO (it leaves this marker).
    if (Test-Path (Join-Path $State 'remove-edge')) { $steps.edge = { Remove-Edge } }
    foreach ($s in $steps.GetEnumerator()) {
        try { & $s.Value } catch { Log "$($s.Key): FAILED - $_" }
    }
    if (-not (Test-Server)) {
        Log 'update server not answering: starting it'
        Start-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Extensions' -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 3
    }
    $st = Write-Status
    if (-not $st.protected) { Log ("not fully protected: " + (($st.checks.GetEnumerator() | Where-Object { -not $_.Value } | ForEach-Object Key) -join ', ')) }
    if ($st.extensionMissing.Count) { Log ("Thurea extension not installed in: " + ($st.extensionMissing -join '; ')) }
}

function Invoke-Serve {
    $types = @{ '.xml' = 'text/xml'; '.crx' = 'application/x-chrome-extension'; '.xpi' = 'application/x-xpinstall'; '.json' = 'application/json' }
    $l = New-Object System.Net.HttpListener
    $l.Prefixes.Add("http://127.0.0.1:$Port/")
    $l.Start()
    while ($l.IsListening) {
        $ctx = $l.GetContext()
        try {
            $name = [IO.Path]::GetFileName($ctx.Request.Url.AbsolutePath)
            $f = Join-Path $Web $name
            if ($name -and (Test-Path -LiteralPath $f -PathType Leaf) -and $ctx.Request.HttpMethod -in 'GET', 'HEAD') {
                $b = [IO.File]::ReadAllBytes($f)
                $ctx.Response.ContentType = $types[[IO.Path]::GetExtension($f)]
                $ctx.Response.ContentLength64 = $b.Length
                $ctx.Response.Headers['Cache-Control'] = 'no-store'
                if ($ctx.Request.HttpMethod -eq 'GET') { $ctx.Response.OutputStream.Write($b, 0, $b.Length) }
            } else { $ctx.Response.StatusCode = 404 }
        } catch {} finally { try { $ctx.Response.Close() } catch {} }
    }
}

function Set-FolderAcl([string]$dir, [string]$sddl) {
    $acl = Get-Acl -LiteralPath $dir
    $acl.SetSecurityDescriptorSddlForm($sddl)
    Set-Acl -LiteralPath $dir -AclObject $acl
    # Everything inside inherits the folder's permissions (and nothing else).
    $out = & icacls "$dir\*" /reset /T /C /Q 2>&1
    if ($LASTEXITCODE) { Log "permissions on $dir`: $out" }
}

# ---------- self-update from signed GitHub releases ----------
# A release is used only if thurea-manifest.json is signed with Thurea's release key (public half in
# data\release-key.json) and the download matches the SHA-256 in that signed manifest.
function Test-ReleaseSignature([byte[]]$signedBytes, [byte[]]$signature) {
    $k = Get-Content -LiteralPath (Join-Path $Data 'release-key.json') -Raw | ConvertFrom-Json
    $p = New-Object Security.Cryptography.RSAParameters
    $p.Modulus = [Convert]::FromBase64String($k.n); $p.Exponent = [Convert]::FromBase64String($k.e)
    $rsa = New-Object Security.Cryptography.RSACng
    $rsa.ImportParameters($p)
    return $rsa.VerifyData($signedBytes, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
}

function Invoke-Update {
    Assert-Admin
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $base = Get-Reg 'SOFTWARE\Thurea' 'UpdateBase'      # for testing only; Administrators-only key
    if (-not $base) { $base = "https://github.com/$Repo/releases" }
    $have = (Get-Content -LiteralPath (Join-Path $Home_ 'VERSION') -Raw).Trim()
    $wc = New-Object Net.WebClient
    $wc.Headers['User-Agent'] = 'Thurea-updater'
    try {
        $man = $wc.DownloadData("$base/latest/download/thurea-manifest.json")
        $sig = $wc.DownloadData("$base/latest/download/thurea-manifest.json.sig")
    } catch { Log "update: couldn't reach the release ($($_.Exception.Message)); trying again later"; return }
    if (-not (Test-ReleaseSignature $man $sig)) { Log "update: the latest release isn't signed with Thurea's key: ignored"; return }
    $m = [Text.Encoding]::UTF8.GetString($man) | ConvertFrom-Json
    if ([version]$m.version -le [version]$have) { Log "update: Thurea $have is up to date"; return }
    Log "update: Thurea $($m.version) is available (installed: $have)"
    if ($CheckOnly) { return }
    $f = $m.files.windows
    $dir = Join-Path $State 'update'
    if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $zip = Join-Path $dir $f.name
    $wc.DownloadFile("$base/download/v$($m.version)/$($f.name)", $zip)
    if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne $f.sha256.ToUpper()) { Log "update: the download doesn't match the signed checksum: not installed"; return }
    Expand-Archive -LiteralPath $zip -DestinationPath "$dir\x" -Force
    $new = "$dir\x\Thurea\Thurea.ps1"
    if (-not (Test-Path $new)) { Log 'update: the release has no Thurea\Thurea.ps1: not installed'; return }
    & "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File $new -Install -Package $zip
    Log "update: Thurea $($m.version) installed (exit $LASTEXITCODE)"
}

# Tray app. Releases ship ThureaTray.exe prebuilt and code-signed (release/build.sh): an exe
# compiled on the PC is unsigned, which Smart App Control and Defender SmartScreen may block. Only
# when no signed copy is shipped is it compiled here with the C# compiler that ships with Windows
# (.NET Framework 4.8).
function Build-Tray {
    $src = @(Get-ChildItem -LiteralPath (Join-Path $Home_ 'tray') -Filter '*.cs' -ErrorAction SilentlyContinue | ForEach-Object FullName)
    $exe = Join-Path $Home_ 'ThureaTray.exe'
    if ((Test-Path -LiteralPath $exe) -and (Get-AuthenticodeSignature -LiteralPath $exe).Status -eq 'Valid') { return }
    if (-not $src) { return }
    $csc = "$env:SystemRoot\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
    $ico = Join-Path $Home_ 'thurea.ico'
    $iconArg = if (Test-Path -LiteralPath $ico) { "/win32icon:$ico" } else { '/nologo' }
    $out = & $csc /nologo $iconArg /target:winexe /optimize+ "/out:$exe" /r:System.Windows.Forms.dll /r:System.Drawing.dll /r:System.Web.Extensions.dll /r:System.Core.dll @src 2>&1
    if ($LASTEXITCODE) { Log "tray app did not compile: $out" }
}

# Starts the tray app in every signed-in user's session (after an install or update).
function Start-TrayForUsers {
    $exe = Join-Path $Home_ 'ThureaTray.exe'
    if (-not (Test-Path $exe)) { return }
    try {
        $p = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Limited
        # After an update the tray comes back quietly; after an install it opens its window.
        $act = if ($Package) { New-ScheduledTaskAction -Execute $exe -Argument '--startup' } else { New-ScheduledTaskAction -Execute $exe }
        Register-ScheduledTask -TaskName 'Thurea Tray Start' -TaskPath '\Thurea\' -Action $act -Principal $p -Force | Out-Null
        Start-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Tray Start'
        Start-Sleep -Seconds 3
        Unregister-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Tray Start' -Confirm:$false
    } catch { Log "tray start: $_" }
}

# ---------- uninstall block ----------
# Thurea's scheduled tasks: if one is deleted or disabled, the others put it back (Apply runs every
# 5 minutes, Watch every hour and at every sign-in, Update daily).
$Tasks = @('Thurea', 'Thurea Extensions', 'Thurea Update', 'Thurea Watch', 'Thurea Mesh')
function Test-Tasks {
    foreach ($t in $Tasks) {
        $x = Get-ScheduledTask -TaskPath '\Thurea\' -TaskName $t -ErrorAction SilentlyContinue
        if (-not $x -or $x.State -eq 'Disabled') { return $false }
    }
    return $true
}
function Repair-Tasks {
    if (Test-Tasks) { return }
    Log 'tasks: a Thurea task was deleted or switched off: putting it back'
    Register-Tasks
    Start-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Extensions' -ErrorAction SilentlyContinue
}
# Settings > Apps lists Thurea, with no Uninstall / Modify / Repair.
function Set-AppEntry {
    $k = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Thurea'
    $ver = (Get-Content (Join-Path $Home_ 'VERSION') -ErrorAction SilentlyContinue | Select-Object -First 1)
    $c = $false
    $c = (Set-Reg $k 'DisplayName' 'Thurea' 'String') -or $c
    $c = (Set-Reg $k 'DisplayVersion' "$ver" 'String') -or $c
    $c = (Set-Reg $k 'Publisher' 'Thurea' 'String') -or $c
    $c = (Set-Reg $k 'DisplayIcon' (Join-Path $Home_ 'ThureaTray.exe') 'String') -or $c
    $c = (Set-Reg $k 'InstallLocation' $Home_ 'String') -or $c
    $c = (Set-Reg $k 'NoRemove' 1 'DWord') -or $c
    $c = (Set-Reg $k 'NoModify' 1 'DWord') -or $c
    $c = (Set-Reg $k 'NoRepair' 1 'DWord') -or $c
    $c = (Set-Reg $k 'SystemComponent' 0 'DWord') -or $c
    $c = (Set-Reg $k 'UninstallString' "`"$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe`" -NoProfile -Command `"Add-Type -A System.Windows.Forms; [Windows.Forms.MessageBox]::Show('Thurea is part of this PC and can''t be uninstalled.', 'Thurea')`"" 'String') -or $c
    if ($c) { Log 'Settings > Apps: Thurea listed (no Uninstall)' }
}
# Start menu: "Thurea" opens the tray app's window (the logo draws itself).
function Set-StartMenu {
    $programs = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
    $lnk = Join-Path $programs 'Thurea.lnk'
    $exe = Join-Path $Home_ 'ThureaTray.exe'
    if ((Test-Path $exe) -and -not (Test-Path $lnk)) {
        $w = New-Object -ComObject WScript.Shell
        $s = $w.CreateShortcut($lnk); $s.TargetPath = $exe; $s.IconLocation = "$exe,0"; $s.Description = 'Thurea: protection, verse of the day, set up a phone'; $s.Save()
    }
}

# The browser extension's helpers (native messaging): the family helper (com.thurea.family: is the
# signed-in account a child's) and video downloads (com.thurea.ytdlp: the same downloads as
# Thurea's Downloads page). Registered for every Chromium-family browser (allowed_origins: the
# extension's ID) and every Firefox-family one (allowed_extensions: its add-on ID).
$ChromiumHostKeys = @('Google\Chrome', 'Chromium', 'Microsoft\Edge', 'BraveSoftware\Brave-Browser', 'Vivaldi', 'Thorium', 'Helium', 'Yandex\YandexBrowser')
$FirefoxHostKeys = @('Mozilla', 'LibreWolf', 'Waterfox', 'Zen', 'Floorp')
function Set-FamilyHost {
    $id = (Get-Content (Join-Path $Home_ 'data\extension-id') -ErrorAction SilentlyContinue | Select-Object -First 1)
    $gecko = (Get-Content (Join-Path $Home_ 'data\firefox-id') -ErrorAction SilentlyContinue | Select-Object -First 1)
    $hosts = @(@{ name = 'com.thurea.family'; file = 'family-host'; arg = 'family-host'; what = 'Thurea family helper' },
               @{ name = 'com.thurea.ytdlp'; file = 'video-host'; arg = 'video-host'; what = 'Thurea video downloads' })
    foreach ($h in $hosts) {
        $cmd = Join-Path $Home_ "$($h.file).cmd"
        Set-Content -Path $cmd -Value "@`"%~dp0thurea.exe`" $($h.arg) %*" -Encoding ASCII
        $sets = @()
        if ($id) { $sets += , @('', @{ allowed_origins = @("chrome-extension://$($id.Trim())/") }, $ChromiumHostKeys) }
        if ($gecko) { $sets += , @('-firefox', @{ allowed_extensions = @($gecko.Trim()) }, $FirefoxHostKeys) }
        foreach ($set in $sets) {
            $m = [ordered]@{ name = $h.name; description = $h.what; path = $cmd; type = 'stdio' }
            foreach ($kv in $set[1].GetEnumerator()) { $m[$kv.Key] = $kv.Value }
            $json = Join-Path $Home_ "$($h.file)$($set[0]).json"
            # No byte-order mark: not every browser's JSON reader skips one.
            [IO.File]::WriteAllText($json, ($m | ConvertTo-Json), (New-Object Text.UTF8Encoding $false))
            foreach ($b in $set[2]) {
                $k = "HKLM:\SOFTWARE\$b\NativeMessagingHosts\$($h.name)"
                New-Item -Path $k -Force | Out-Null
                Set-ItemProperty -Path $k -Name '(default)' -Value $json
            }
        }
    }
}

function Register-Tasks {
    $ps = "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe"
    $me = Join-Path $Home_ 'Thurea.ps1'
    $sys = New-ScheduledTaskPrincipal -UserId 'S-1-5-18' -RunLevel Highest
    $set = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 30) -MultipleInstances IgnoreNew

    # Apply: at startup and every 5 minutes (browser policies, the extension, blocked apps; never
    # DNS). Versions up to 1.0.5 also ran it on every network change, to switch DNS: -Force
    # replaces the old task, so that trigger goes.
    $boot = New-ScheduledTaskTrigger -AtStartup
    $every = New-ScheduledTaskTrigger -Once -At (Get-Date).Date -RepetitionInterval (New-TimeSpan -Minutes 5)
    $act = New-ScheduledTaskAction -Execute $ps -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$me`" -Apply"
    Register-ScheduledTask -TaskName 'Thurea' -TaskPath '\Thurea\' -Action $act -Trigger @($boot, $every) -Principal $sys -Settings $set -Force | Out-Null

    # Update server for the extension: at startup, restarted if it stops.
    $set2 = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
    $act2 = New-ScheduledTaskAction -Execute $ps -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$me`" -Serve"
    Register-ScheduledTask -TaskName 'Thurea Extensions' -TaskPath '\Thurea\' -Action $act2 -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal $sys -Settings $set2 -Force | Out-Null

    # Self-update: every hour (a release published from git reaches this PC within the hour), and
    # 3 minutes after startup.
    $daily = New-ScheduledTaskTrigger -Once -At (Get-Date).Date -RepetitionInterval (New-TimeSpan -Hours 1)
    $daily.RandomDelay = 'PT5M'
    $late = New-ScheduledTaskTrigger -AtStartup
    $late.Delay = 'PT3M'
    $act3 = New-ScheduledTaskAction -Execute $ps -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$me`" -Update"
    Register-ScheduledTask -TaskName 'Thurea Update' -TaskPath '\Thurea\' -Action $act3 -Trigger @($daily, $late) -Principal $sys -Settings $set -Force | Out-Null

    # Household mesh (thurea.exe): keeps this PC in the family, passes parents' changes on. Starts
    # at startup and every 5 minutes (it exits at once until the PC is in a household).
    $set4 = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
    $act4 = New-ScheduledTaskAction -Execute (Join-Path $Home_ 'thurea.exe') -Argument 'run --mesh-only'
    Register-ScheduledTask -TaskName 'Thurea Mesh' -TaskPath '\Thurea\' -Action $act4 -Trigger @($boot, $every) -Principal $sys -Settings $set4 -Force | Out-Null

    # Watch: every hour and at every sign-in, re-applies (and so puts back any task that's gone).
    $hourly = New-ScheduledTaskTrigger -Once -At (Get-Date).Date -RepetitionInterval (New-TimeSpan -Hours 1)
    $logon = New-ScheduledTaskTrigger -AtLogOn
    Register-ScheduledTask -TaskName 'Thurea Watch' -TaskPath '\Thurea\' -Action $act -Trigger @($hourly, $logon) -Principal $sys -Settings $set -Force | Out-Null
}

function Invoke-Install {
    Assert-Admin
    $src = $PSScriptRoot
    New-Item -ItemType Directory -Force -Path $State | Out-Null
    if ($src.TrimEnd('\') -ne $Home_.TrimEnd('\')) {
        New-Item -ItemType Directory -Force -Path $Home_ | Out-Null
        # The running tray app and update server hold their files: stop them while Thurea is replaced.
        Get-Process ThureaTray -ErrorAction SilentlyContinue | Stop-Process -Force
        Stop-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Extensions' -ErrorAction SilentlyContinue
        Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -match 'Thurea\.ps1.*-Serve' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
        & robocopy $src $Home_ /MIR /NFL /NDL /NJH /NJS /XD windows | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "copy to $Home_ failed ($LASTEXITCODE)" }
    }
    # "Thurea for Windows" for the tray's QR page: the release zip itself.
    $pkgDir = Join-Path $Home_ 'windows'
    New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null
    $zipSrc = if ($Package) { $Package } else { Join-Path $src 'windows\thurea-windows.zip' }
    if ((Test-Path -LiteralPath $zipSrc) -and ((Resolve-Path $zipSrc).Path -ne (Join-Path $pkgDir 'thurea-windows.zip'))) { Copy-Item -LiteralPath $zipSrc (Join-Path $pkgDir 'thurea-windows.zip') -Force }
    Build-Tray
    # Only SYSTEM and Administrators may change Thurea's files; everyone may read and run them.
    Set-FolderAcl $Home_ 'O:BAG:SYD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)(A;OICI;0x1200a9;;;AC)'
    New-Item -ItemType Directory -Force -Path $State | Out-Null
    Set-FolderAcl $State 'O:BAG:SYD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)'

    Set-FamilyHost
    Register-Tasks
    Start-ScheduledTask -TaskPath '\Thurea\' -TaskName 'Thurea Extensions'

    # Tray app for every user, and the LAN page it opens for the QR code (private networks only).
    $tray = Join-Path $Home_ 'ThureaTray.exe'
    if (Test-Path $tray) {
        Set-TrayAutostart
        & netsh http delete urlacl url=http://+:47824/ 2>$null | Out-Null
        & netsh http add urlacl url=http://+:47824/ sddl='D:(A;;GX;;;BU)' | Out-Null
        Get-NetFirewallRule -DisplayName 'Thurea - share with your devices' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        New-NetFirewallRule -DisplayName 'Thurea - share with your devices' -Group 'Thurea' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 47824 -Profile Private | Out-Null
        Get-NetFirewallRule -DisplayName 'Thurea - household' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        New-NetFirewallRule -DisplayName 'Thurea - household' -Group 'Thurea' -Direction Inbound -Action Allow -Protocol UDP -LocalPort 47823 -Profile Private | Out-Null
    }
    Log "Thurea $((Get-Content (Join-Path $Home_ 'VERSION') -Raw).Trim()) installed in $Home_"
    Invoke-Apply
    Start-TrayForUsers
}

# The tray app starts at every sign-in (quietly: --startup opens no window).
function Set-TrayAutostart {
    $tray = Join-Path $Home_ 'ThureaTray.exe'
    if (Test-Path $tray) {
        if (Set-Reg 'SOFTWARE\Microsoft\Windows\CurrentVersion\Run' 'Thurea' "`"$tray`" --startup" 'String') { Log 'tray app: starts at sign-in' }
    }
}

function Show-Status {
    $s = Write-Status
    "Thurea: " + $(if ($s.protected) { 'protected' } else { 'NOT fully protected' })
    foreach ($c in $s.checks.GetEnumerator()) { "  {0,-16} {1}" -f $c.Key, $(if ($c.Value) { 'ok' } else { 'MISSING' }) }
}

switch ($PSCmdlet.ParameterSetName) {
    'Install' { Invoke-Install }
    'Update' { Invoke-Update }
    'Serve' { Invoke-Serve }
    'Status' { Show-Status }
    default { Invoke-Apply }
}
