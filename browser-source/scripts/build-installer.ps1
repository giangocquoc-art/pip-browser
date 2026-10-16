param([switch]$SkipBrowserBuild)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$buildProcess=Get-Process -Id $PID
$previousPriority=$buildProcess.PriorityClass
$previousJobs=$env:CARGO_BUILD_JOBS
$buildProcess.PriorityClass='BelowNormal'
$env:CARGO_BUILD_JOBS='2'
Push-Location $repo
try {
    if (-not $SkipBrowserBuild) {
        & cargo build -p evergreen-browser --release --locked
        if ($LASTEXITCODE -ne 0) {throw 'Pip compilation failed'}
    }
    $assets=Join-Path $repo 'crates/installer/assets'
    New-Item -ItemType Directory -Force -Path $assets | Out-Null
    $bootstrapper=Join-Path $assets 'MicrosoftEdgeWebview2Setup.exe'
    if (-not (Test-Path -LiteralPath $bootstrapper)) {
        Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $bootstrapper
    }
    $signature=Get-AuthenticodeSignature -LiteralPath $bootstrapper
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation') {
        throw 'WebView2 bootstrapper does not have a valid Microsoft signature'
    }
    & python tools/package_sources.py
    if ($LASTEXITCODE -ne 0) {throw 'Source/license packaging failed'}
    Copy-Item -LiteralPath 'target/release/evergreen-browser.exe' -Destination (Join-Path $assets 'evergreen-browser.exe') -Force
    & cargo build -p evergreen-installer --release --locked
    if ($LASTEXITCODE -ne 0) {throw 'Pip installer compilation failed'}
    New-Item -ItemType Directory -Force -Path 'dist','dist-pip' | Out-Null
    $installerVersion=[regex]::Match((Get-Content 'crates/installer/Cargo.toml' -Raw),'(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $installerVersion) {throw 'Installer version missing'}
    $setup=Join-Path $repo "dist/Pip-Setup-$installerVersion-Windows-x64.exe"
    Copy-Item -LiteralPath 'target/release/evergreen-installer.exe' -Destination $setup -Force
    Copy-Item -LiteralPath 'target/release/evergreen-browser.exe' -Destination 'dist-pip/Pip.exe' -Force
    Copy-Item -LiteralPath 'PIP_README.md','THIRD_PARTY.md','LICENSE' -Destination 'dist-pip' -Force
    $hash=Get-FileHash -LiteralPath $setup -Algorithm SHA256
    "$($hash.Hash.ToLower())  $([IO.Path]::GetFileName($setup))" | Set-Content -LiteralPath 'dist/SHA256SUMS.txt' -Encoding ascii
    [PSCustomObject]@{Setup=$setup;Bytes=(Get-Item -LiteralPath $setup).Length;SHA256=$hash.Hash;RuntimeBootstrapperSHA256=(Get-FileHash -LiteralPath $bootstrapper).Hash} | ConvertTo-Json | Set-Content -LiteralPath 'dist/build-manifest.json' -Encoding utf8
    Write-Host "Ready to distribute: $setup" -ForegroundColor Green
} finally {Pop-Location; $buildProcess.PriorityClass=$previousPriority; $env:CARGO_BUILD_JOBS=$previousJobs}
