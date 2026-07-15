# Sentinel Edge — Windows uninstaller (card #83).
# Removes the service and the installed files. Run from an ELEVATED PowerShell.
#     powershell -ExecutionPolicy Bypass -File .\uninstall.ps1

#Requires -RunAsAdministrator
$ErrorActionPreference = "SilentlyContinue"

$Base      = Join-Path $env:ProgramData "Sentinel"
$ServiceId = "SentinelEdge"

if (Test-Path "$Base\WinSW.exe") {
    Push-Location $Base
    & ".\WinSW.exe" stop      "$ServiceId.xml"
    & ".\WinSW.exe" uninstall "$ServiceId.xml"
    Pop-Location
}
Start-Sleep -Seconds 1
Remove-Item -Recurse -Force $Base
Write-Host "Sentinel Edge removed."
