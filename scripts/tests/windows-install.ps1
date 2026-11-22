# Runs only in an ephemeral GitHub-hosted Windows job. Scope firewall rules to
# the installers, bundled executables and WebView2, leaving loopback available.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'This acceptance script requires a disposable GitHub-hosted Windows runner.'
}
$group = "shinytauri-acceptance-$([guid]::NewGuid())"
$evidence = New-Item -Type Directory "$env:RUNNER_TEMP/windows-acceptance"
function Block-External($exe) {
    New-NetFirewallRule -DisplayName $group -Group $group -Direction Outbound -Action Block -Profile Any `
        -Program $exe -RemoteAddress '0.0.0.0-126.255.255.255','128.0.0.0-255.255.255.255','::2-ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff' | Out-Null
}
try {
    $edge = @(Get-ChildItem "${env:ProgramFiles(x86)}/Microsoft/EdgeWebView/Application/*/msedgewebview2.exe")
    if (!$edge.Count) { throw 'WebView2 Runtime is required' }
    foreach ($exe in $edge) { Block-External $exe.FullName }
    $hosts = @{}
    foreach ($kind in 'minimal','rhino') {
        $product = "shinytauri-smoke-$kind"
        $installer = Get-Item "$env:RUNNER_TEMP/installers/$kind-setup.exe"
        Block-External $installer.FullName
        if ($kind -eq 'rhino') {
            python "$PSScriptRoot/windows-branding.py" setup $installer.FullName (Join-Path $evidence.FullName 'installer-license.png')
        }
        $install = Start-Process $installer.FullName -ArgumentList '/S' -Wait -PassThru
        if ($install.ExitCode) { throw "$product setup failed: $($install.ExitCode)" }
        $key = Get-ItemProperty "HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/$product"
        if ($key.DisplayName -ne $product) { throw "Missing $product registration" }
        if ($kind -eq 'rhino' -and $key.DisplayVersion -ne '1.2.3') { throw 'Wrong Rhino release version' }
        $dir = $key.InstallLocation.Trim('"')
        if ($kind -eq 'rhino' -and !(Test-Path "$dir/offline/app/app/static" -PathType Container)) {
            throw 'The installed Rhino app is missing its static resource directory'
        }
        $hosts[$kind] = (Get-Item (Join-Path $dir $key.MainBinaryName)).FullName
        if ($kind -eq 'rhino') {
            if ($key.Publisher -ne '販売 株式会社' -or (Get-Item $hosts[$kind]).VersionInfo.CompanyName -ne $key.Publisher) {
                throw 'The installed Rhino publisher does not match its configured publisher'
            }
            python "$PSScriptRoot/windows-branding.py" host $hosts[$kind]
        }
        foreach ($exe in Get-ChildItem $dir -Filter '*.exe' -File -Recurse) {
            Block-External $exe.FullName
        }
    }
    if ($hosts.minimal -eq $hosts.rhino) { throw 'Both products use the same executable' }
    python "$PSScriptRoot/windows-ui.py" minimal $hosts.minimal $evidence.FullName
    & "$PSScriptRoot/check-upgrade.ps1" $hosts.minimal $evidence.FullName
    # The separately registered Rhino product must still run after the update.
    $rhino = Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/shinytauri-smoke-rhino'
    if ($rhino.DisplayVersion -ne '1.2.3' -or
        (Join-Path $rhino.InstallLocation.Trim('"') $rhino.MainBinaryName) -ne $hosts.rhino) {
        throw 'The minimal upgrade changed the registered Rhino installation'
    }
    python "$PSScriptRoot/windows-ui.py" rhino $hosts.rhino $evidence.FullName
} finally {
    Get-NetFirewallRule -Group $group -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    foreach ($kind in 'minimal','rhino') {
        Get-ChildItem "$env:LOCALAPPDATA/org.example.$kind/logs" -Filter '*.log' -ErrorAction SilentlyContinue |
            Copy-Item -Destination $evidence.FullName
    }
}
