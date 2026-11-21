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
        foreach ($exe in Get-ChildItem $dir -Filter '*.exe' -File -Recurse) {
            Block-External $exe.FullName
        }
    }
    if ($hosts.minimal -eq $hosts.rhino) { throw 'Both products use the same executable' }
    foreach ($kind in 'minimal','rhino') {
        python "$PSScriptRoot/windows-ui.py" $kind $hosts[$kind] $evidence.FullName
    }
} finally {
    Get-NetFirewallRule -Group $group -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    foreach ($kind in 'minimal','rhino') {
        Get-ChildItem "$env:LOCALAPPDATA/org.example.$kind/logs" -Filter '*.log' -ErrorAction SilentlyContinue |
            Copy-Item -Destination $evidence.FullName
    }
}
