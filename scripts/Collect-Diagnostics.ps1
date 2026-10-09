param(
    [string]$Destination = (Join-Path $env:TEMP ('startpoint-diagnostics-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [switch]$IncludeDumps,
    [string]$Executable
)
$ErrorActionPreference = 'Stop'
if (Test-Path $Destination) { throw 'Choose a new destination folder; existing bundles are never overwritten.' }
New-Item -ItemType Directory -Path $Destination | Out-Null
$source = Join-Path $env:LOCALAPPDATA 'Startpoint\data\diagnostics'
if (Test-Path $source) {
    Get-ChildItem $source -Filter '*.log' -File | Copy-Item -Destination $Destination
}
$events = @(Get-WinEvent -FilterHashtable @{ LogName='Application'; Id=1000,1001; StartTime=(Get-Date).AddDays(-7) } -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match '(?i)startpoint\.exe' })
ConvertTo-Json -InputObject @($events | Select-Object TimeCreated,Id,ProviderName,Message) -Depth 4 |
    Set-Content (Join-Path $Destination 'windows-events.json') -Encoding UTF8
$reports = @(
    (Join-Path $env:ProgramData 'Microsoft\Windows\WER\ReportArchive'),
    (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\WER\ReportArchive')
)
foreach ($root in $reports) {
    foreach ($report in @(Get-ChildItem $root -Directory -Filter '*startpoint.exe*' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 3)) {
        $file = Join-Path $report.FullName 'Report.wer'
        if (Test-Path $file) { Copy-Item $file (Join-Path $Destination ($report.Name + '.wer')) }
    }
}
if ($Executable) {
    $item = Get-Item -LiteralPath $Executable
    $hash = Get-FileHash -LiteralPath $Executable -Algorithm SHA256
    [ordered]@{ File=$item.Name; Bytes=$item.Length; Modified=$item.LastWriteTimeUtc; SHA256=$hash.Hash } |
        ConvertTo-Json | Set-Content (Join-Path $Destination 'build.json') -Encoding UTF8
}
if ($IncludeDumps) {
    Write-Warning 'Crash dumps may contain API keys, notes and other process memory. Review privately; do not post them publicly.'
    $dumpFolder = Join-Path $Destination 'private-dumps'
    New-Item -ItemType Directory -Path $dumpFolder | Out-Null
    foreach ($root in @((Join-Path $source 'crashes'), (Join-Path $env:LOCALAPPDATA 'CrashDumps'))) {
        Get-ChildItem $root -Filter 'startpoint*.dmp' -File -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending | Select-Object -First 3 |
            Copy-Item -Destination $dumpFolder
    }
    foreach ($event in $events) {
        foreach ($match in [regex]::Matches($event.Message, '(?m)\\\\\?\\(C:\\[^\r\n]+\.dmp)')) {
            $file = $match.Groups[1].Value.Trim()
            if (Test-Path -LiteralPath $file) { Copy-Item -LiteralPath $file -Destination $dumpFolder }
        }
    }
}
@'
Local Startpoint diagnostics; nothing has been uploaded.
Logs contain action labels, job states, GPUI warning/error locations and panic stack traces, not credentials or library contents.
Windows reports and stack traces can expose usernames, file paths and machine details. Review before sharing.
If private-dumps exists, its files can contain secrets from memory: share privately only.
Keep the matching startpoint.exe and startpoint.pdb privately for crash analysis.
'@ | Set-Content (Join-Path $Destination 'READ-ME.txt') -Encoding UTF8
Compress-Archive -Path (Join-Path $Destination '*') -DestinationPath ($Destination + '.zip')
Write-Output ('Diagnostics: ' + $Destination + '.zip')
