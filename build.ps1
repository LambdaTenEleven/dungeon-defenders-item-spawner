param([string]$OutputDirectory = '')
$ErrorActionPreference = 'Stop'

# Rustup normally adds this directory to PATH; also support existing terminals.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin 'cargo.exe'))) {
        throw 'Install Rust with rustup, then run this script again. See README.md.'
    }
    $env:PATH = "$cargoBin;$env:PATH"
}

if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $PSScriptRoot ('dist\' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
}

Push-Location $PSScriptRoot
try {
    & cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed.' }
    & cargo clippy --locked --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed.' }
    & cargo clippy --locked -p dd-adapter --all-targets --target i686-pc-windows-msvc -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw '32-bit adapter Clippy failed.' }
    & cargo test --locked --workspace --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
    & (Join-Path $PSScriptRoot 'scripts\package.ps1') -OutputDirectory $OutputDirectory
} finally {
    Pop-Location
}
