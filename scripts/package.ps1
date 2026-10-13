param([string]$OutputDirectory = '')
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repoRoot 'dist' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$packageDirectory = Join-Path $OutputDirectory 'DungeonWorkshop-preview'

Push-Location $repoRoot
try {
    & cargo build --locked --release -p dd-overlay --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Overlay build failed.' }
    & cargo build --locked --release -p dd-adapter --target i686-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Adapter build failed.' }

    # Do not clear a supplied directory or accidentally package stale extra files.
    if (Test-Path -LiteralPath $packageDirectory) { throw "Package directory already exists: $packageDirectory. Choose a fresh output directory." }
    New-Item -ItemType Directory -Path $packageDirectory -Force | Out-Null
    $targetRoot = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) } else { Join-Path $repoRoot 'target' }
    Copy-Item -LiteralPath (Join-Path $targetRoot 'x86_64-pc-windows-msvc\release\dd-overlay.exe') -Destination $packageDirectory
    Copy-Item -LiteralPath (Join-Path $targetRoot 'i686-pc-windows-msvc\release\dd-adapter-host.exe') -Destination $packageDirectory
    Copy-Item -LiteralPath (Join-Path $targetRoot 'i686-pc-windows-msvc\release\dd_adapter.dll') -Destination $packageDirectory
    Copy-Item -LiteralPath (Join-Path $repoRoot 'README.md'),(Join-Path $repoRoot 'LICENSE') -Destination $packageDirectory
    $docsDirectory = Join-Path $packageDirectory 'docs'
    New-Item -ItemType Directory -Path $docsDirectory | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\integration.md') -Destination $docsDirectory
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\runtime-verification.md') -Destination $docsDirectory
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\current-build-validation.md') -Destination $docsDirectory
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\item-menu-implementation.md') -Destination $docsDirectory
    $protocolDocs = Join-Path $packageDirectory 'crates\protocol'
    New-Item -ItemType Directory -Path $protocolDocs -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot 'crates\protocol\README.md') -Destination $protocolDocs
    $catalogDocs = Join-Path $packageDirectory 'crates\catalog'
    New-Item -ItemType Directory -Path $catalogDocs -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot 'crates\catalog\README.md') -Destination $catalogDocs
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\automatic-catalog.md') -Destination $docsDirectory
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\reroll-benchmark.md') -Destination $docsDirectory

    $testLog = Join-Path $OutputDirectory 'adapter-smoke.stdout.log'
    $errorLog = Join-Path $OutputDirectory 'adapter-smoke.stderr.log'
    $testProcess = Start-Process -FilePath (Join-Path $packageDirectory 'dd-overlay.exe') -ArgumentList '--self-test' -WindowStyle Hidden -RedirectStandardOutput $testLog -RedirectStandardError $errorLog -PassThru
    # Keep the native handle open so Windows PowerShell retains the exit code.
    $null = $testProcess.Handle
    if (-not $testProcess.WaitForExit(20000)) {
        $testProcess.Kill()
        throw 'Adapter smoke test timed out.'
    }
    if ($testProcess.ExitCode -ne 0) { throw "Adapter smoke test failed: $($testProcess.ExitCode). See $errorLog" }
    Get-Content -LiteralPath $testLog
    & (Join-Path $PSScriptRoot 'transport-test.ps1') -PackageDirectory $packageDirectory

    $archive = Join-Path $OutputDirectory 'DungeonWorkshop-preview-windows.zip'
    if (Test-Path -LiteralPath $archive) { throw "Archive already exists: $archive" }
    Compress-Archive -LiteralPath $packageDirectory -DestinationPath $archive
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($archive))" | Set-Content -LiteralPath ($archive + '.sha256') -Encoding ascii
    Write-Output $archive
} finally { Pop-Location }
