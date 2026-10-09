# Opt-in, per-executable Windows Error Reporting dumps. Run in an elevated PowerShell.
param([switch]$Disable)
$ErrorActionPreference = 'Stop'
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script in an administrator PowerShell. Startpoint does not change machine crash settings automatically.'
}
$key = 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\startpoint.exe'
if ($Disable) {
    # Remove only the three values this script configures; leave other WER settings alone.
    foreach ($name in @('DumpFolder','DumpCount','DumpType')) {
        Remove-ItemProperty -Path $key -Name $name -ErrorAction SilentlyContinue
    }
    Write-Output 'Startpoint-specific crash dump settings removed.'
    return
}
if ((Test-Path $key) -and (Get-Item $key).GetValueNames().Count -gt 0) {
    throw 'Startpoint crash settings already exist. Review them instead of overwriting them.'
}
$folder = Join-Path $env:LOCALAPPDATA 'Startpoint\data\diagnostics\crashes'
New-Item -ItemType Directory -Path $folder -Force | Out-Null
New-Item -Path $key -Force | Out-Null
New-ItemProperty -Path $key -Name DumpFolder -Value $folder -PropertyType ExpandString | Out-Null
New-ItemProperty -Path $key -Name DumpCount -Value 3 -PropertyType DWord | Out-Null
New-ItemProperty -Path $key -Name DumpType -Value 1 -PropertyType DWord | Out-Null
Write-Warning 'Minidumps are private and may contain secrets from process memory. They are never uploaded automatically.'
Write-Output ('Windows will retain up to three Startpoint minidumps in ' + $folder)
