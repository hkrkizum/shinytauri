# Called on the disposable Windows runner after v1 was exercised and Rhino was installed.
param([string] $HostPath, [string] $Evidence)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$product = 'shinytauri-smoke-minimal'
$key = "HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/$product"
$old = Get-ItemProperty -LiteralPath $key
$dir = $old.InstallLocation.Trim('"')
if ($old.DisplayVersion -ne '0.1.0' -or $HostPath -ne (Join-Path $dir $old.MainBinaryName)) {
    throw 'The previous version is not registered at its expected installation path'
}
$stale = Join-Path $dir 'offline/app/www/style.css'
if (!(Test-Path -LiteralPath $stale -PathType Leaf)) { throw 'Missing v1 stylesheet before upgrade' }
$note = Join-Path $env:LOCALAPPDATA 'org.example.minimal/note.txt'
$unmanaged = Join-Path $dir 'user-note.txt'
foreach ($file in $note, $unmanaged) {
    'saved with version 0.1.0' | Set-Content -LiteralPath $file -Encoding utf8NoBOM
}
$setup = Get-Item "$env:RUNNER_TEMP/installers/minimal-upgrade-setup.exe"
Block-External $setup.FullName
python "$PSScriptRoot/windows-upgrade.py" $setup.FullName
$updated = Get-ItemProperty -LiteralPath $key
if ($updated.DisplayVersion -ne '0.2.0' -or $updated.InstallLocation -ne $old.InstallLocation) {
    throw 'The upgrade did not replace v1 at its registered location'
}
if (Test-Path -LiteralPath $stale) { throw 'The previous version left an obsolete stylesheet' }
foreach ($file in $note, $unmanaged) {
    if ((Get-Content -LiteralPath $file) -cne 'saved with version 0.1.0') { throw "User data changed: $file" }
}
python "$PSScriptRoot/windows-ui.py" upgrade $HostPath $Evidence
