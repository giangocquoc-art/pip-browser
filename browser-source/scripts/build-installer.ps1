param(
    [switch]$SkipBrowserBuild,
    [string]$SigningCertThumbprint = $env:PIP_SIGNING_CERT_THUMBPRINT,
    [switch]$RequireCodeSigning
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if ($RequireCodeSigning -and [string]::IsNullOrWhiteSpace($SigningCertThumbprint)) {
    throw 'Code signing is required. Set PIP_SIGNING_CERT_THUMBPRINT to a trusted public code-signing certificate.'
}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$buildProcess=Get-Process -Id $PID
$previousPriority=$buildProcess.PriorityClass
$previousJobs=$env:CARGO_BUILD_JOBS
$buildProcess.PriorityClass='BelowNormal'
$env:CARGO_BUILD_JOBS='2'
Push-Location $repo
function Sign-PipExecutable {
    param([Parameter(Mandatory)][string]$Path)
    if ([string]::IsNullOrWhiteSpace($SigningCertThumbprint)) {
        if ($RequireCodeSigning) { throw 'Code signing is required. Set PIP_SIGNING_CERT_THUMBPRINT to a trusted public code-signing certificate.' }
        Write-Warning "No public code-signing certificate configured; leaving unsigned: $Path"
        return $false
    }
    $thumbprint = ($SigningCertThumbprint -replace '\s', '').ToUpperInvariant()
    $stores = @('Cert:\CurrentUser\My', 'Cert:\LocalMachine\My')
    $signer = $null
    $signerStore = $null
    foreach ($storePath in $stores) {
        $candidate = Get-ChildItem -Path $storePath -ErrorAction SilentlyContinue | Where-Object {
            $_.Thumbprint -eq $thumbprint -and $_.HasPrivateKey -and $_.NotBefore -le (Get-Date) -and $_.NotAfter -gt (Get-Date) -and
            ($_.EnhancedKeyUsageList | Where-Object { $_.ObjectId.Value -eq '1.3.6.1.5.5.7.3.3' })
        } | Select-Object -First 1
        if ($candidate) { $signer = $candidate; $signerStore = $storePath; break }
    }
    if (-not $signer) { throw "No valid, private-key-enabled Code Signing certificate with thumbprint $thumbprint was found in the CurrentUser or LocalMachine personal store." }
    $signToolCommand = Get-Command 'signtool.exe' -ErrorAction SilentlyContinue
    $signToolPath = if ($signToolCommand) { $signToolCommand.Source }
    if (-not $signToolPath) {
        $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        $signToolFile = Get-ChildItem -Path (Join-Path $sdkBin '*\x64\signtool.exe') -ErrorAction SilentlyContinue |
            Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
        if ($signToolFile) { $signToolPath = $signToolFile.FullName }
    }
    if (-not $signToolPath) { throw 'SignTool was not found. Install the Windows SDK signing tools.' }
    $arguments = @('sign', '/fd', 'SHA256', '/tr', 'https://timestamp.digicert.com', '/td', 'SHA256', '/sha1', $thumbprint, '/s', 'My', '/d', 'Pip Browser')
    if ($signerStore -eq 'Cert:\LocalMachine\My') { $arguments += '/sm' }
    $arguments += (Resolve-Path -LiteralPath $Path).Path
    & $signToolPath @arguments
    if ($LASTEXITCODE -ne 0) { throw "SignTool failed for $Path with exit code $LASTEXITCODE." }
    $verified = Get-AuthenticodeSignature -LiteralPath $Path
    if ($verified.Status -ne 'Valid' -or $verified.SignerCertificate.Thumbprint -ne $thumbprint) {
        throw "Authenticode verification failed for $Path (status: $($verified.Status))."
    }
    Write-Host "Signed and verified: $Path ($($verified.SignerCertificate.Subject))" -ForegroundColor Green
    return $true
}
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
    $browserBinary=Join-Path $repo 'target/release/evergreen-browser.exe'
    $browserSigned=Sign-PipExecutable -Path $browserBinary
    Copy-Item -LiteralPath 'target/release/evergreen-browser.exe' -Destination (Join-Path $assets 'evergreen-browser.exe') -Force
    & cargo build -p evergreen-installer --release --locked
    if ($LASTEXITCODE -ne 0) {throw 'Pip installer compilation failed'}
    $installerBinary=Join-Path $repo 'target/release/evergreen-installer.exe'
    $installerSigned=Sign-PipExecutable -Path $installerBinary
    New-Item -ItemType Directory -Force -Path 'dist','dist-pip' | Out-Null
    $installerVersion=[regex]::Match((Get-Content 'crates/installer/Cargo.toml' -Raw),'(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $installerVersion) {throw 'Installer version missing'}
    $setup=Join-Path $repo 'dist/PipSetup.exe'
    Copy-Item -LiteralPath $installerBinary -Destination $setup -Force
    Copy-Item -LiteralPath $browserBinary -Destination 'dist-pip/Pip.exe' -Force
    Copy-Item -LiteralPath 'PIP_README.md','THIRD_PARTY.md','LICENSE' -Destination 'dist-pip' -Force
    $hash=Get-FileHash -LiteralPath $setup -Algorithm SHA256
    "$($hash.Hash.ToLower())  $([IO.Path]::GetFileName($setup))" | Set-Content -LiteralPath 'dist/SHA256SUMS.txt' -Encoding ascii
    $publisher=$null
    if ($installerSigned) { $publisher=(Get-AuthenticodeSignature -LiteralPath $setup).SignerCertificate.Subject }
    [PSCustomObject]@{Setup=$setup;Bytes=(Get-Item -LiteralPath $setup).Length;SHA256=$hash.Hash;AuthenticodeSigned=($browserSigned -and $installerSigned);Publisher=$publisher;RuntimeBootstrapperSHA256=(Get-FileHash -LiteralPath $bootstrapper).Hash} | ConvertTo-Json | Set-Content -LiteralPath 'dist/build-manifest.json' -Encoding utf8
    Write-Host "Ready to distribute: $setup" -ForegroundColor Green
} finally {Pop-Location; $buildProcess.PriorityClass=$previousPriority; $env:CARGO_BUILD_JOBS=$previousJobs}
