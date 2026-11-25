# Dot-sourced by build.ps1 with $iso set: skip every Setup/OOBE screen except the disk picker and the local account screen.
    $ua = "$iso\autounattend.xml"
    $U = 'urn:schemas-microsoft-com:unattend'
    $WCM = 'http://schemas.microsoft.com/WMIConfig/2002/State'
    $doc = [xml]::new(); $doc.PreserveWhitespace = $true; $doc.Load($ua)
    $ns = New-Object System.Xml.XmlNamespaceManager($doc.NameTable); $ns.AddNamespace('u', $U)

    function El($parent, $name, $text) {
        $e = $doc.CreateElement($name, $U)
        if ($null -ne $text) { $e.InnerText = $text }
        [void]$parent.AppendChild($e); $e
    }
    function Component($pass, $name) {
        $settings = $doc.SelectSingleNode("/u:unattend/u:settings[@pass='$pass']", $ns)
        $c = $settings.SelectSingleNode("u:component[@name='$name']", $ns)
        if ($c) { [void]$settings.RemoveChild($c) }
        $c = $doc.CreateElement('component', $U)
        foreach ($a in @{ name = $name; processorArchitecture = 'amd64'; publicKeyToken = '31bf3856ad364e35'; language = 'neutral'; versionScope = 'nonSxS' }.GetEnumerator()) { $c.SetAttribute($a.Key, $a.Value) }
        [void]$settings.PrependChild($c); $c
    }
    function Set-Child($parent, $name, $text) {
        $e = $parent.SelectSingleNode("u:$name", $ns)
        if (-not $e) { $e = El $parent $name $null }
        $e.InnerText = $text
    }

    # Windows Setup: language/keyboard page skipped (UK English formats + keyboard, US English UI).
    $pe = Component 'windowsPE' 'Microsoft-Windows-International-Core-WinPE'
    $sul = El $pe 'SetupUILanguage' $null; [void](El $sul 'UILanguage' 'en-US')
    [void](El $pe 'InputLocale' '0809:00000809'); [void](El $pe 'SystemLocale' 'en-GB')
    [void](El $pe 'UILanguage' 'en-US'); [void](El $pe 'UserLocale' 'en-GB')

    # Windows Setup: product key page skipped with Microsoft's generic Pro install key (does not activate).
    $setup = $doc.SelectSingleNode("/u:unattend/u:settings[@pass='windowsPE']/u:component[@name='Microsoft-Windows-Setup']", $ns)
    $ud = $setup.SelectSingleNode('u:UserData', $ns)
    $pk = $ud.SelectSingleNode('u:ProductKey', $ns); if ($pk) { [void]$ud.RemoveChild($pk) }
    $pk = $doc.CreateElement('ProductKey', $U); [void]$ud.PrependChild($pk)
    [void](El $pk 'Key' 'VK7JG-NPHTM-C97JM-9MPGT-3V66T'); [void](El $pk 'WillShowUI' 'OnError')
    # Disk picker stays: no DiskConfiguration / InstallTo is added.

    # OOBE: region + keyboard pages skipped.
    $oi = Component 'oobeSystem' 'Microsoft-Windows-International-Core'
    [void](El $oi 'InputLocale' '0809:00000809'); [void](El $oi 'SystemLocale' 'en-GB')
    [void](El $oi 'UILanguage' 'en-US'); [void](El $oi 'UserLocale' 'en-GB')

    # OOBE: everything else hidden except the local account screen.
    $shell = $doc.SelectSingleNode("/u:unattend/u:settings[@pass='oobeSystem']/u:component[@name='Microsoft-Windows-Shell-Setup']", $ns)
    $oobe = $shell.SelectSingleNode('u:OOBE', $ns)
    Set-Child $oobe 'HideEULAPage' 'true'
    Set-Child $oobe 'HideOEMRegistrationScreen' 'true'
    Set-Child $oobe 'HideOnlineAccountScreens' 'true'
    Set-Child $oobe 'HideWirelessSetupInOOBE' 'true'
    Set-Child $oobe 'HideLocalAccountScreen' 'false'
    Set-Child $oobe 'ProtectYourPC' '3'
    if (-not $shell.SelectSingleNode('u:TimeZone', $ns)) { $tz = $doc.CreateElement('TimeZone', $U); $tz.InnerText = 'GMT Standard Time'; [void]$shell.PrependChild($tz) }

    # Shell-Setup children must follow the schema order (OOBE before FirstLogonCommands is already so).
    [System.IO.File]::WriteAllText($ua, $doc.OuterXml, [System.Text.UTF8Encoding]::new($false))
    $check = [xml](Get-Content $ua -Raw)
    Log "autounattend.xml updated and re-parsed OK"
    $check.unattend.settings | Where-Object { $_.pass -in 'windowsPE', 'oobeSystem' } | ForEach-Object { $p = $_.pass; $_.component | ForEach-Object { Log "  $p : $($_.name)" } }

