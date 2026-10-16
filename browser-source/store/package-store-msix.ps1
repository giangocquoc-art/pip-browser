param(
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9.-]{3,50}$')][string]$IdentityName,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$PublisherDistinguishedName,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$PublisherDisplayName,
    [string]$BrowserExe,
    [string]$Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$out = Join-Path $PSScriptRoot 'out'
$content = Join-Path $out 'package'
$unpacked = Join-Path $out 'unpacked-check'

if ([string]::IsNullOrWhiteSpace($Version)) {
    $cargo = Get-Content (Join-Path $repo 'crates/app/Cargo.toml') -Raw
    $shortVersion = [regex]::Match($cargo, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"').Groups[1].Value
    if (-not $shortVersion) { throw 'Could not read the browser version from crates/app/Cargo.toml.' }
    $Version = "$shortVersion.0"
}
if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$') {
    throw 'Store package version must use four numeric components, for example 0.1.1.0.'
}
if ([string]::IsNullOrWhiteSpace($BrowserExe)) {
    $BrowserExe = Join-Path $repo 'target/release/evergreen-browser.exe'
}
$BrowserExe = (Resolve-Path -LiteralPath $BrowserExe).Path
$iconPath = Join-Path $repo 'crates/app/ui/icon.ico'
$templatePath = Join-Path $PSScriptRoot 'AppxManifest.xml.template'
$makeAppx = Get-ChildItem (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin') -Filter makeappx.exe -File -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Directory.Name -eq 'x64' } |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
    Select-Object -First 1
if (-not $makeAppx) { throw 'Windows SDK MakeAppx.exe (x64) is required to create the Store package.' }

foreach ($path in @($content, $unpacked)) {
    $resolved = [IO.Path]::GetFullPath($path)
    if (-not $resolved.StartsWith([IO.Path]::GetFullPath($out), [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clean a path outside the Store output directory: $resolved"
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
New-Item -ItemType Directory -Path (Join-Path $content 'Assets') -Force | Out-Null
$testWarning = Join-Path $out 'DO_NOT_SUBMIT.txt'
if (Test-Path -LiteralPath $testWarning) { Remove-Item -LiteralPath $testWarning -Force }
Copy-Item -LiteralPath $BrowserExe -Destination (Join-Path $content 'Pip.exe')

Add-Type -AssemblyName System.Drawing
$sourceIcon = [System.Drawing.Image]::FromFile($iconPath)
try {
    foreach ($asset in @(
        @{ Name = 'StoreLogo.png'; Size = 50 },
        @{ Name = 'Square44x44Logo.png'; Size = 44 },
        @{ Name = 'Square150x150Logo.png'; Size = 150 },
        @{ Name = 'Wide310x150Logo.png'; Width = 310; Height = 150 },
        @{ Name = 'Square310x310Logo.png'; Size = 310 }
    )) {
        $width = if ($asset.ContainsKey('Size')) { $asset.Size } else { $asset.Width }
        $height = if ($asset.ContainsKey('Size')) { $asset.Size } else { $asset.Height }
        $bitmap = [System.Drawing.Bitmap]::new($width, $height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $side = [Math]::Min($width, $height)
                $x = [int](($width - $side) / 2)
                $y = [int](($height - $side) / 2)
                $graphics.DrawImage($sourceIcon, [System.Drawing.Rectangle]::new($x, $y, $side, $side))
            } finally { $graphics.Dispose() }
            $bitmap.Save((Join-Path $content "Assets/$($asset.Name)"), [System.Drawing.Imaging.ImageFormat]::Png)
        } finally { $bitmap.Dispose() }
    }
} finally { $sourceIcon.Dispose() }

$manifest = Get-Content -LiteralPath $templatePath -Raw
$manifest = $manifest.Replace('__IDENTITY_NAME__', [System.Security.SecurityElement]::Escape($IdentityName))
$manifest = $manifest.Replace('__PUBLISHER_DN__', [System.Security.SecurityElement]::Escape($PublisherDistinguishedName))
$manifest = $manifest.Replace('__PUBLISHER_DISPLAY_NAME__', [System.Security.SecurityElement]::Escape($PublisherDisplayName))
$manifest = $manifest.Replace('__VERSION__', $Version)
$manifestPath = Join-Path $content 'AppxManifest.xml'
Set-Content -LiteralPath $manifestPath -Value $manifest -Encoding utf8

$packagePath = Join-Path $out "Pip_$($Version)_x64.msix"
$uploadPath = Join-Path $out "Pip_$($Version)_x64.msixupload"
& $makeAppx.FullName pack /d $content /p $packagePath /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx pack failed with exit code $LASTEXITCODE." }
& $makeAppx.FullName unpack /p $packagePath /d $unpacked /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx unpack validation failed with exit code $LASTEXITCODE." }
$sourceHash = (Get-FileHash -LiteralPath (Join-Path $content 'Pip.exe') -Algorithm SHA256).Hash
$packedHash = (Get-FileHash -LiteralPath (Join-Path $unpacked 'Pip.exe') -Algorithm SHA256).Hash
if ($sourceHash -ne $packedHash) { throw 'Packaged Pip.exe does not match the selected browser binary.' }

Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path -LiteralPath $uploadPath) { Remove-Item -LiteralPath $uploadPath -Force }
$uploadStage = Join-Path $out 'upload-stage'
if (Test-Path -LiteralPath $uploadStage) { Remove-Item -LiteralPath $uploadStage -Recurse -Force }
New-Item -ItemType Directory -Path $uploadStage | Out-Null
Copy-Item -LiteralPath $packagePath -Destination $uploadStage
[System.IO.Compression.ZipFile]::CreateFromDirectory($uploadStage, $uploadPath)
Remove-Item -LiteralPath $uploadStage -Recurse -Force
Write-Host "Store MSIX package created and payload hash verified: $packagePath" -ForegroundColor Green
Write-Host "Partner Center upload package: $uploadPath" -ForegroundColor Green
Write-Host 'The Microsoft Store signs the MSIX after certification. This package is not signed for sideloading.' -ForegroundColor Yellow
