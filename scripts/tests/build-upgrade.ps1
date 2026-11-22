# Build a second version of the external minimal project with the same identity.
param([string] $Cli, [string] $Previous, [string] $Destination)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$project = Join-Path $env:RUNNER_TEMP '新版/shinytauri-smoke-minimal'
New-Item -ItemType Directory -Path $project -Force | Out-Null
Copy-Item "$PSScriptRoot/fixtures/upgrade/app.R", "$Previous/DESCRIPTION" $project
& $Cli init $project --id org.example.minimal --r-version 4.5.3
# Both versions declare the same runtime and package requirements.
New-Item -ItemType Directory -Force "$project/.shinytauri" | Out-Null
Copy-Item "$Previous/.shinytauri/windows.lock" "$project/.shinytauri/windows.lock"
$config = "$project/shinytauri.toml"
(Get-Content -LiteralPath $config) -replace '^version = "0\.1\.0"$', 'version = "0.2.0"' |
    Set-Content -LiteralPath $config -Encoding utf8NoBOM
& $Cli build $project
Copy-Item "$project/dist/shinytauri-smoke-minimal_0.2.0_x64-setup.exe" $Destination
