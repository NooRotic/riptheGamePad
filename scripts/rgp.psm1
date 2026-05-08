# riptheGamePad PowerShell helpers
#
# Usage:
#   Import-Module C:\Dev\projects\riptheGamePad\scripts\rgp.psm1
# Or add to your $PROFILE:
#   Import-Module C:\Dev\projects\riptheGamePad\scripts\rgp.psm1
#
# Then use:
#   rgp                  → run the app
#   rgp-list             → list connected gamepads
#   rgp-debug            → run with RGP_LOG=debug
#   rgp-build            → cargo build (debug)
#   rgp-release          → cargo build --release
#   rgp-test             → cargo test --workspace
#   rgp-clippy           → cargo clippy --workspace -- -D warnings
#   rgp-kill             → terminate any running tray instance
#   rgp-reset            → delete user config so the next run recreates the default
#   rgp-config           → open the user's config.toml in your default editor
#   rgp-where            → print the user's config.toml path
#   rgp-bak              → list any .v1.bak files written by the migrator

$script:RgpRoot   = "C:\Dev\projects\riptheGamePad"
$script:RgpConfig = "$env:APPDATA\nooroticx\riptheGamePad\config\config.toml"

function rgp {
    Push-Location $script:RgpRoot
    try { cargo run -p rgp-app -- @args }
    finally { Pop-Location }
}

function rgp-list {
    Push-Location $script:RgpRoot
    try { cargo run -p rgp-app -- --list-devices }
    finally { Pop-Location }
}

function rgp-debug {
    Push-Location $script:RgpRoot
    try {
        $env:RGP_LOG = "debug"
        cargo run -p rgp-app -- @args
    }
    finally {
        Remove-Item Env:\RGP_LOG -ErrorAction SilentlyContinue
        Pop-Location
    }
}

function rgp-build {
    Push-Location $script:RgpRoot
    try { cargo build -p rgp-app }
    finally { Pop-Location }
}

function rgp-release {
    Push-Location $script:RgpRoot
    try { cargo build -p rgp-app --release }
    finally { Pop-Location }
}

function rgp-test {
    Push-Location $script:RgpRoot
    try { cargo test --workspace }
    finally { Pop-Location }
}

function rgp-clippy {
    Push-Location $script:RgpRoot
    try { cargo clippy --workspace -- -D warnings }
    finally { Pop-Location }
}

function rgp-kill {
    $procs = Get-Process -Name riptheGamePad -ErrorAction SilentlyContinue
    if ($null -eq $procs) {
        Write-Host "No riptheGamePad processes running."
        return
    }
    $procs | Stop-Process -Force
    Start-Sleep -Milliseconds 500
    Write-Host ("Killed {0} riptheGamePad process(es)." -f $procs.Count)
}

function rgp-reset {
    if (Test-Path $script:RgpConfig) {
        Remove-Item $script:RgpConfig -Force
        Write-Host "Deleted $script:RgpConfig (will be recreated on next run)."
    } else {
        Write-Host "No config to delete at $script:RgpConfig"
    }
}

function rgp-config {
    if (-not (Test-Path $script:RgpConfig)) {
        Write-Host "Config does not exist yet. Run 'rgp' once to create it."
        return
    }
    Start-Process $script:RgpConfig
}

function rgp-where {
    Write-Host $script:RgpConfig
    if (Test-Path $script:RgpConfig) { Write-Host "(exists)" } else { Write-Host "(missing)" }
}

function rgp-bak {
    $dir = Split-Path $script:RgpConfig -Parent
    if (-not (Test-Path $dir)) {
        Write-Host "$dir does not exist."
        return
    }
    Get-ChildItem $dir -Filter "*.v1.bak" | Format-Table FullName, Length, LastWriteTime
}

Export-ModuleMember -Function rgp, rgp-list, rgp-debug, rgp-build, rgp-release, rgp-test, rgp-clippy, rgp-kill, rgp-reset, rgp-config, rgp-where, rgp-bak
