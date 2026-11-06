param(
    [string]$SetupPath = 'dist/PipSetup.exe'
)

$ErrorActionPreference = 'Stop'

$sourceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$python = (Get-Command python.exe -ErrorAction Stop).Source
$setup = [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $SetupPath))
$checkScript = Join-Path $PSScriptRoot 'check_installer.py'
$targetDirectory = Join-Path $sourceRoot 'target'
$releaseDirectory = Join-Path $targetDirectory 'release'
$payload = Join-Path $releaseDirectory 'evergreen-browser.exe'
$toolsDirectory = $PSScriptRoot
$distributionDirectory = Join-Path $sourceRoot 'dist'
$adblockCheckScript = Join-Path $toolsDirectory 'check_adblock_binary.py'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    & $python $checkScript $setup
    if ($LASTEXITCODE -ne 0) {
        throw "Installer verification failed with exit code $LASTEXITCODE."
    }
    $env:PIP_ADBLOCK_BINARY_VERIFICATION_PATH = Join-Path $targetDirectory 'adblock-binary-verification.json'
    & $python $adblockCheckScript $payload
    if ($LASTEXITCODE -ne 0) {
        throw "Packaged ad-block verification failed with exit code $LASTEXITCODE."
    }
    exit 0
}

$userName = 'pipverify' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
$evidenceDirectory = Join-Path $targetDirectory "installer-check-$userName"
$evidencePath = Join-Path $evidenceDirectory 'installer-verification.json'
$plainPassword = [Guid]::NewGuid().ToString('N') + 'Aa1!'
$password = ConvertTo-SecureString $plainPassword -AsPlainText -Force
$account = "$env:COMPUTERNAME\$userName"
$stdout = Join-Path $evidenceDirectory 'installer-check.stdout.log'
$stderr = Join-Path $evidenceDirectory 'installer-check.stderr.log'
$adblockStdout = Join-Path $evidenceDirectory 'adblock-check.stdout.log'
$adblockStderr = Join-Path $evidenceDirectory 'adblock-check.stderr.log'
$adblockEvidencePath = Join-Path $evidenceDirectory 'adblock-binary-verification.json'
$userCreated = $false
$grantedPaths = [System.Collections.Generic.List[string]]::new()
$previousEvidencePath = $env:PIP_INSTALLER_VERIFICATION_PATH
$previousAdblockEvidencePath = $env:PIP_ADBLOCK_BINARY_VERIFICATION_PATH
$previousTemp = $env:TEMP
$previousTmp = $env:TMP

function Grant-TemporaryAccess {
    param([string]$Path, [ValidateSet('RX', 'M')][string]$Right)

    & icacls.exe $Path /grant "${account}:${Right}" /C | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Could not grant the temporary standard user $Right access to $Path."
    }
    [void] $grantedPaths.Add($Path)
}

try {
    New-LocalUser -Name $userName -Password $password -PasswordNeverExpires `
        -AccountNeverExpires -UserMayNotChangePassword | Out-Null
    $userCreated = $true
    New-Item -ItemType Directory -Path $evidenceDirectory | Out-Null

    # Hosted runner workspaces may be private to the runner account. Grant only
    # traversal/read access to the exact inputs and write access to a disposable
    # evidence directory; keep the compiled output and rest of the checkout read-only.
    foreach ($path in @($sourceRoot, $toolsDirectory, $distributionDirectory, $targetDirectory, $releaseDirectory)) {
        Grant-TemporaryAccess -Path $path -Right RX
    }
    foreach ($path in @($checkScript, $adblockCheckScript, $setup, $payload)) {
        Grant-TemporaryAccess -Path $path -Right RX
    }
    Grant-TemporaryAccess -Path $evidenceDirectory -Right M

    $credential = [PSCredential]::new($account, $password)
    $env:PIP_INSTALLER_VERIFICATION_PATH = $evidencePath
    $env:PIP_ADBLOCK_BINARY_VERIFICATION_PATH = $adblockEvidencePath
    # A credentialed child inherits the runner's environment. Point its temp
    # paths at this user-writable evidence directory instead of runneradmin's.
    $env:TEMP = $evidenceDirectory
    $env:TMP = $evidenceDirectory
    $arguments = '"' + $checkScript + '" "' + $setup + '"'
    $process = Start-Process -FilePath $python -ArgumentList $arguments `
        -Credential $credential -LoadUserProfile -WindowStyle Hidden `
        -WorkingDirectory $sourceRoot -RedirectStandardOutput $stdout `
        -RedirectStandardError $stderr -Wait -PassThru

    if (Test-Path -LiteralPath $stdout) {
        Get-Content -LiteralPath $stdout
    }
    if ($process.ExitCode -ne 0) {
        if (Test-Path -LiteralPath $stderr) {
            Get-Content -LiteralPath $stderr | Write-Error
        }
        throw "Installer verification under a standard user failed with exit code $($process.ExitCode)."
    }

    $adblockArguments = '"' + $adblockCheckScript + '" "' + $payload + '"'
    $adblockProcess = Start-Process -FilePath $python -ArgumentList $adblockArguments `
        -Credential $credential -LoadUserProfile -WindowStyle Hidden `
        -WorkingDirectory $sourceRoot -RedirectStandardOutput $adblockStdout `
        -RedirectStandardError $adblockStderr -Wait -PassThru

    if (Test-Path -LiteralPath $adblockStdout) {
        Get-Content -LiteralPath $adblockStdout
    }
    if ($adblockProcess.ExitCode -ne 0) {
        if (Test-Path -LiteralPath $adblockStderr) {
            Get-Content -LiteralPath $adblockStderr | Write-Error
        }
        throw "Packaged ad-block verification under a standard user failed with exit code $($adblockProcess.ExitCode)."
    }
    if (-not (Test-Path -LiteralPath $adblockEvidencePath -PathType Leaf)) {
        throw 'Packaged ad-block verification did not produce its evidence file.'
    }
    $adblockEvidence = Get-Content -LiteralPath $adblockEvidencePath -Raw | ConvertFrom-Json
    if ($adblockEvidence.sha256 -ne (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash.ToLowerInvariant()) {
        throw 'Packaged ad-block evidence does not match the verified browser binary.'
    }
}
finally {
    $cleanupFailures = [System.Collections.Generic.List[string]]::new()

    if ($null -eq $previousEvidencePath) {
        Remove-Item Env:\PIP_INSTALLER_VERIFICATION_PATH -ErrorAction SilentlyContinue
    }
    else {
        $env:PIP_INSTALLER_VERIFICATION_PATH = $previousEvidencePath
    }
    if ($null -eq $previousAdblockEvidencePath) {
        Remove-Item Env:\PIP_ADBLOCK_BINARY_VERIFICATION_PATH -ErrorAction SilentlyContinue
    }
    else {
        $env:PIP_ADBLOCK_BINARY_VERIFICATION_PATH = $previousAdblockEvidencePath
    }
    if ($null -eq $previousTemp) {
        Remove-Item Env:\TEMP -ErrorAction SilentlyContinue
    }
    else {
        $env:TEMP = $previousTemp
    }
    if ($null -eq $previousTmp) {
        Remove-Item Env:\TMP -ErrorAction SilentlyContinue
    }
    else {
        $env:TMP = $previousTmp
    }
    foreach ($path in $grantedPaths) {
        try {
            & icacls.exe $path /remove:g $account /C | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "icacls exited with code $LASTEXITCODE"
            }
        }
        catch {
            $cleanupFailures.Add("Could not remove the temporary ACL from '$path': $_")
        }
    }
    if ($userCreated) {
        try {
            Remove-LocalUser -Name $userName -ErrorAction Stop
        }
        catch {
            $cleanupFailures.Add("Could not remove the temporary user '$userName': $_")
        }
    }
    foreach ($path in @($stdout, $stderr, $adblockStdout, $adblockStderr)) {
        try {
            Remove-Item -LiteralPath $path -Force -ErrorAction Stop
        }
        catch {
            if (Test-Path -LiteralPath $path) {
                $cleanupFailures.Add("Could not remove temporary output '$path': $_")
            }
        }
    }
    try {
        Remove-Item -LiteralPath $evidenceDirectory -Recurse -Force -ErrorAction Stop
    }
    catch {
        if (Test-Path -LiteralPath $evidenceDirectory) {
            $cleanupFailures.Add("Could not remove temporary evidence '$evidenceDirectory': $_")
        }
    }
    $plainPassword = $null
    $password = $null
    $credential = $null
    if ($cleanupFailures.Count -gt 0) {
        Write-Warning ($cleanupFailures -join [Environment]::NewLine)
    }
}
