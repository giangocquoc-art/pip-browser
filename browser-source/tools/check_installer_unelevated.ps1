param(
    [string]$SetupPath = 'dist/PipSetup.exe',
    [string]$ScreenshotPath
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
$portableDirectory = Join-Path $sourceRoot 'dist-pip'
$portableBrowser = Join-Path $portableDirectory 'Pip.exe'
$adblockCheckScript = Join-Path $toolsDirectory 'check_adblock_binary.py'

function Capture-PipWindow {
    param(
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][string]$OutputPath,
        [PSCredential]$Credential
    )

    Add-Type -AssemblyName System.Drawing
    if (-not ('PipCiWindowCapture' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PipCiWindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll", SetLastError=true)] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll", SetLastError=true)] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();

    public static IntPtr FindProcessWindow(uint expectedProcessId, string titlePrefix) {
        IntPtr match = IntPtr.Zero;
        EnumWindows((hWnd, _) => {
            if (!IsWindowVisible(hWnd)) return true;
            uint processId;
            GetWindowThreadProcessId(hWnd, out processId);
            if (processId != expectedProcessId) return true;
            StringBuilder title = new StringBuilder(512);
            GetWindowText(hWnd, title, title.Capacity);
            if (title.ToString().StartsWith(titlePrefix, StringComparison.Ordinal)) {
                match = hWnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return match;
    }
}
'@
    }

    $start = @{ FilePath = $Executable; WorkingDirectory = $sourceRoot; PassThru = $true; WindowStyle = 'Normal' }
    if ($Credential) { $start.Credential = $Credential; $start.LoadUserProfile = $true }
    $browser = Start-Process @start
    try {
        $handle = [IntPtr]::Zero
        for ($attempt = 0; $attempt -lt 35; $attempt++) {
            Start-Sleep -Seconds 1
            $browser.Refresh()
            if ($browser.HasExited) { throw "Pip exited before its window appeared (exit code $($browser.ExitCode))." }
            # Pip.exe has a console subsystem, so Process.MainWindowHandle may
            # return the runner's console host instead of Pip's native Winit window.
            $handle = [PipCiWindowCapture]::FindProcessWindow([uint32]$browser.Id, 'Pip —')
            if ($handle -ne [IntPtr]::Zero -and [PipCiWindowCapture]::IsWindowVisible($handle)) { break }
        }
        if ($handle -eq [IntPtr]::Zero -or -not [PipCiWindowCapture]::IsWindowVisible($handle)) {
            throw 'Pip did not create a visible native window titled Pip — within 35 seconds.'
        }

        # Place the test window fully inside the runner's primary display so the
        # capture contains the complete app even when the runner resolution differs.
        $screenWidth = [PipCiWindowCapture]::GetSystemMetrics(0)
        $screenHeight = [PipCiWindowCapture]::GetSystemMetrics(1)
        $captureWidth = [Math]::Min(1280, $screenWidth)
        $captureHeight = [Math]::Min(800, $screenHeight)
        if ($captureWidth -lt 700 -or $captureHeight -lt 480) {
            throw "Windows runner display is too small for UX review: ${screenWidth}x${screenHeight}."
        }
        # Bring Pip to the foreground before sampling the desktop. A no-activate
        # topmost placement can leave the hosted runner terminal visible above
        # the app in the captured pixels even when the correct HWND was found.
        # HWND_TOPMOST, SWP_SHOWWINDOW
        if (-not [PipCiWindowCapture]::SetWindowPos($handle, [IntPtr](-1), 0, 0, $captureWidth, $captureHeight, 0x0040)) {
            throw 'Could not place the Pip window above the runner terminal for visual capture.'
        }
        [void][PipCiWindowCapture]::SetForegroundWindow($handle)
        Start-Sleep -Milliseconds 500
        if ([PipCiWindowCapture]::GetForegroundWindow() -ne $handle) {
            throw 'Pip could not be brought to the foreground; refusing to upload a screenshot of another window.'
        }

        # Give WebView2 time to paint the bundled Pip start page before sampling
        # the actual app window. The page itself performs no external navigation.
        Start-Sleep -Seconds 8
        $rect = New-Object PipCiWindowCapture+RECT
        if (-not [PipCiWindowCapture]::GetWindowRect($handle, [ref]$rect)) {
            throw 'Could not read the Pip window bounds for the UX screenshot.'
        }
        $width = $rect.Right - $rect.Left
        $height = $rect.Bottom - $rect.Top
        if ($width -lt 700 -or $height -lt 480) {
            throw "Pip window is too small for visual UX review: ${width}x${height}."
        }

        $bitmap = [System.Drawing.Bitmap]::new($width, $height)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
            $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
        }
        finally {
            $graphics.Dispose()
            $bitmap.Dispose()
        }
        if (-not (Test-Path -LiteralPath $OutputPath -PathType Leaf) -or (Get-Item -LiteralPath $OutputPath).Length -lt 10000) {
            throw 'Pip UI screenshot is missing or unexpectedly small.'
        }
        Write-Host "Captured visible Pip UI: $OutputPath (${width}x${height})" -ForegroundColor Green
    }
    finally {
        if (-not $browser.HasExited) {
            [void]$browser.CloseMainWindow()
            if (-not $browser.WaitForExit(8000)) { Stop-Process -Id $browser.Id -Force -ErrorAction SilentlyContinue }
        }
        $browser.Dispose()
    }
}

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
    if ($ScreenshotPath) {
        $screenshot = [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $ScreenshotPath))
        Capture-PipWindow -Executable $portableBrowser -OutputPath $screenshot
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
    if ($ScreenshotPath) { Grant-TemporaryAccess -Path $portableDirectory -Right RX }
    foreach ($path in @($checkScript, $adblockCheckScript, $setup, $payload)) {
        Grant-TemporaryAccess -Path $path -Right RX
    }
    if ($ScreenshotPath) { Grant-TemporaryAccess -Path $portableBrowser -Right RX }
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
    if ($ScreenshotPath) {
        $screenshot = [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $ScreenshotPath))
        Capture-PipWindow -Executable $portableBrowser -OutputPath $screenshot -Credential $credential
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
