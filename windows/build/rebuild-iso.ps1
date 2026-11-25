# Quick rebuild in the build VM: reuses the updated Windows image of the latest C:\work-* folder,
# re-stages Thurea and any supplied GPU packages from the transfer disk (checked against
# SHA256SUMS made on the host), and rebuilds the ISO.
param([string]$OutputDirectory = $PSScriptRoot)

$ErrorActionPreference = 'Stop'
$X = $PSScriptRoot
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Start-Transcript -Path (Join-Path $OutputDirectory 'build.log') -Force
function Log($m) { Write-Host "[$((Get-Date).ToString('HH:mm:ss'))] $m" }
try {
    $W = (Get-ChildItem 'C:\' -Directory -Filter 'work-*' | Sort-Object Name | Select-Object -Last 1).FullName
    $iso = "$W\iso"
    if (-not (Test-Path "$iso\sources\install.wim")) { throw "no built image in $iso" }
    Log "Reusing $W ($([int]((Get-Item "$iso\sources\install.wim").Length / 1MB)) MB install.wim)"

    # Every file on the transfer disk must match the host's checksum list.
    $bad = 0; $n = 0
    foreach ($line in Get-Content "$X\SHA256SUMS") {
        $hash, $rel = $line -split '  ', 2
        $f = Join-Path $X ($rel -replace '/', '\')
        $n++
        if (-not (Test-Path -LiteralPath $f) -or (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash -ne $hash.ToUpper()) { $bad++; Log "BAD: $rel" }
    }
    if ($bad) { throw "$bad of $n files on the transfer disk don't match" }
    Log "Transfer disk: all $n files match their checksums"

    $restage = @((Join-Path $iso 'sources\$OEM$\$$\Setup\Thurea'), "$W\thurea", "$W\thurea-zip")
    if (Test-Path -LiteralPath "$X\thurea-gpu") { $restage += "$iso\thurea-gpu" }
    foreach ($d in $restage) {
        if (Test-Path -LiteralPath $d) { Remove-Item -LiteralPath $d -Recurse -Force }
    }
    . "$X\stage-thurea.ps1"

    # The staged copies on the media must match too.
    $bad = 0
    foreach ($line in Get-Content "$X\SHA256SUMS") {
        $hash, $rel = $line -split '  ', 2
        $f = if ($rel -like 'thurea-gpu/*') { Join-Path $iso ($rel -replace '/', '\') } elseif ($rel -like 'thurea/*') { Join-Path (Join-Path $iso 'sources\$OEM$\$$\Setup\Thurea') ($rel.Substring(7) -replace '/', '\') } else { continue }
        if (-not (Test-Path -LiteralPath $f) -or (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash -ne $hash.ToUpper()) { $bad++; Log "BAD on media: $rel" }
    }
    if ($bad) { throw "$bad staged files don't match" }
    Log 'Media copies match'

    $build = (Get-Content "$X\BUILD").Trim()
    $thureaVersion = (Get-Content -LiteralPath "$W\thurea\Thurea\VERSION").Trim()
    if ($thureaVersion -notmatch '^\d+\.\d+\.\d+$') { throw "Invalid Thurea version: $thureaVersion" }
    $out = Join-Path $OutputDirectory "Win11_Pro_$($build)_Thurea_$thureaVersion.iso"
    $bootData = "2#p0,e,b`"$iso\boot\etfsboot.com`"#pEF,e,b`"$iso\efi\microsoft\boot\efisys.bin`""
    $oscArgs = @('-m', '-o', '-u2', '-udfver102', "-bootdata:$bootData", '-l"CTOS_MODIFIED"', "`"$iso`"", "`"$out`"")
    Log 'Running oscdimg'
    $p = Start-Process -FilePath "$X\oscdimg.exe" -ArgumentList ($oscArgs -join ' ') -Wait -PassThru -NoNewWindow
    if ($p.ExitCode -ne 0) { throw "oscdimg exited with code $($p.ExitCode)" }
    $hash = (Get-FileHash $out -Algorithm SHA256).Hash
    Log "ISO: $out ($((Get-Item $out).Length) bytes, SHA256 $hash)"
    Set-Content -Path (Join-Path $OutputDirectory 'DONE') -Value "$(Split-Path $out -Leaf) $hash"
} catch {
    Write-Host "FAILED: $_"; Write-Host $_.ScriptStackTrace
    Set-Content -Path (Join-Path $OutputDirectory 'FAILED') -Value "$_"
} finally {
    Stop-Transcript
    Stop-Computer -Force
}
