@echo off
rem Trusts Thurea's code-signing certificate on this PC, so thurea-windows.msix installs.
rem Run once per PC (asks for Administrator). Only needed while Thurea is signed with its own
rem self-signed certificate; release/build.sh fills in the certificate.
net session >nul 2>&1 || (powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs" & exit /b)
powershell -NoProfile -ExecutionPolicy Bypass -Command "$c = New-Object Security.Cryptography.X509Certificates.X509Certificate2(,[Convert]::FromBase64String('@CERT@')); $s = New-Object Security.Cryptography.X509Certificates.X509Store('TrustedPeople', 'LocalMachine'); $s.Open('ReadWrite'); $s.Add($c); $s.Close(); Write-Host ('This PC now trusts Thurea''s installer: ' + $c.Subject + ' (' + $c.Thumbprint + ')')"
echo.
pause
