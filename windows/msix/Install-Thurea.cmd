@echo off
rem Install Thurea: double-click, say yes to the Administrator prompt, done.
rem 1. trusts Thurea's code-signing certificate on this PC (release/build.sh fills it in),
rem 2. installs thurea-windows.msix ("Thurea Setup"): the copy next to this file, or else the
rem    latest release from GitHub,
rem 3. runs Thurea Setup, which installs Thurea itself.
rem Windows checks the certificate before anything inside an MSIX can run, which is why the trust
rem step lives here and not in the package.
net session >nul 2>&1 || (powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs" & exit /b)
set "THUREA_CMD=%~f0"
powershell -NoProfile -ExecutionPolicy Bypass -Command "iex ((Get-Content -LiteralPath $env:THUREA_CMD -Raw) -split ('#' + 'PS' + '#'))[1]"
exit /b
#PS#
$ErrorActionPreference = 'Stop'
try {
    $cert = New-Object Security.Cryptography.X509Certificates.X509Certificate2(, [Convert]::FromBase64String('@CERT@'))
    # Trusted People is what MSIX checks; a self-signed certificate is also its own root.
    foreach ($store in 'TrustedPeople', 'Root') {
        $s = New-Object Security.Cryptography.X509Certificates.X509Store($store, 'LocalMachine')
        $s.Open('ReadWrite'); $s.Add($cert); $s.Close()
    }
    Write-Host "Trusted Thurea's certificate ($($cert.Thumbprint))."

    $msix = Join-Path (Split-Path -Parent $env:THUREA_CMD) 'thurea-windows.msix'
    if (-not (Test-Path -LiteralPath $msix)) {
        $msix = Join-Path $env:TEMP 'thurea-windows.msix'
        Write-Host 'Downloading Thurea...'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        (New-Object Net.WebClient).DownloadFile('https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-windows.msix', $msix)
    }
    $sig = Get-AuthenticodeSignature -LiteralPath $msix
    if ($sig.SignerCertificate -and $sig.SignerCertificate.Thumbprint -ne $cert.Thumbprint) {
        throw "$msix isn't signed with Thurea's certificate"
    }
    Write-Host 'Installing Thurea Setup...'
    Add-AppxPackage -Path $msix -ForceUpdateFromAnyVersion
    $pkg = Get-AppxPackage -Name 'Thurea.Setup' | Where-Object { $_.Publisher -eq $cert.Subject } | Sort-Object Version -Descending | Select-Object -First 1
    if (-not $pkg) { throw "Thurea Setup didn't install" }
    # Run Thurea Setup here, already as Administrator, so there is no second prompt.
    & (Join-Path $pkg.InstallLocation 'Setup.ps1')
} catch {
    Write-Host ''
    Write-Host "Thurea wasn't installed: $($_.Exception.Message)"
    Read-Host 'Press Enter to close' | Out-Null
    exit 1
}
