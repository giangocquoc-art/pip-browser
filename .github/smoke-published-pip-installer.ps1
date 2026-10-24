$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$version = '0.1.16'
$expectedSha256 = '3ffd295cd68e8d65b096cd538946be46238ece143c1fc5ecfaa26badb41be7c2'
$installerUrl = "https://github.com/giangocquoc-art/pip-browser/releases/download/v$version/Pip-Setup-$version-Windows-x64.exe"
$debugPort = 9222
$firewallRuleNames = @()
$createdUser = $false
$userSid = $null
$userName = $null
$profilePath = $null
$stageRoot = $null
$webSocket = $null
$plainPassword = $null
$securePassword = $null
$credential = $null
$report = [ordered]@{
    release = "v$version"
    installerSha256 = $expectedSha256
    installed = $false
    webview2RuntimePrepared = $false
    googleDnsProxyConfigured = $false
    systemDnsFallbackBlocked = $false
    hostResolverDirectDnsBlocked = $false
    youtubeLoaded = $false
    offlineUiDetected = $null
    hostname = $null
    title = $null
}

function Get-ProcessOwnerName {
    param([Parameter(Mandatory)] $Process)
    try {
        $owner = Invoke-CimMethod -InputObject $Process -MethodName GetOwner -ErrorAction Stop
        if ($owner.ReturnValue -eq 0) {
            return [string]$owner.User
        }
    } catch {
        return $null
    }
    return $null
}

function Get-SmokeProcesses {
    param([Parameter(Mandatory)][string] $Name)
    $matches = @()
    foreach ($process in Get-CimInstance -ClassName Win32_Process -Filter "Name='$Name'") {
        if ((Get-ProcessOwnerName -Process $process) -eq $userName) {
            $matches += $process
        }
    }
    return $matches
}

function Send-CdpRequest {
    param(
        [Parameter(Mandatory)][System.Net.WebSockets.ClientWebSocket] $Socket,
        [Parameter(Mandatory)][int] $RequestId,
        [Parameter(Mandatory)][string] $Method,
        [hashtable] $Params = @{}
    )

    $message = ConvertTo-Json -InputObject @{ id = $RequestId; method = $Method; params = $Params } -Compress -Depth 20
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($message)
    $tokenSource = [System.Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(10))
    try {
        $segment = [System.ArraySegment[byte]]::new($bytes)
        $Socket.SendAsync($segment, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $tokenSource.Token).GetAwaiter().GetResult() | Out-Null

        $buffer = [byte[]]::new(32768)
        $stream = [System.IO.MemoryStream]::new()
        while ($true) {
            $chunk = [System.ArraySegment[byte]]::new($buffer)
            $result = $Socket.ReceiveAsync($chunk, $tokenSource.Token).GetAwaiter().GetResult()
            if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
                throw 'The WebView2 DevTools connection closed before replying.'
            }
            $stream.Write($buffer, 0, $result.Count)
            if ($result.EndOfMessage) {
                $response = ConvertFrom-Json -InputObject ([System.Text.Encoding]::UTF8.GetString($stream.ToArray())) -AsHashtable
                $stream.SetLength(0)
                if ($response.id -eq $RequestId) {
                    return $response
                }
            }
        }
    } finally {
        $tokenSource.Dispose()
    }
}

try {
    if (-not $env:RUNNER_TEMP) {
        throw 'This smoke test must run on a GitHub-hosted Windows runner.'
    }
    $stageRoot = Join-Path $env:RUNNER_TEMP ("pip-public-release-smoke-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null
    $installerPath = Join-Path $stageRoot "Pip-Setup-$version-Windows-x64.exe"

    Write-Host "Downloading the published Pip v$version installer."
    Invoke-WebRequest -Uri $installerUrl -OutFile $installerPath -TimeoutSec 90
    $actualSha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "Published installer SHA-256 mismatch: expected $expectedSha256, got $actualSha256."
    }

    $userName = 'PipSmoke' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $plainPassword = [guid]::NewGuid().ToString('N') + 'Aa1!'
    $securePassword = ConvertTo-SecureString -String $plainPassword -AsPlainText -Force
    $localUser = New-LocalUser -Name $userName -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword
    $createdUser = $true
    $userSid = $localUser.SID.Value
    $credential = [System.Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$userName", $securePassword)

    $administratorsSid = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
    $administrators = Get-LocalGroup -SID $administratorsSid
    $adminMembers = Get-LocalGroupMember -Group $administrators
    if ($adminMembers | Where-Object { $_.SID.Value -eq $userSid }) {
        throw 'The temporary smoke-test account unexpectedly belongs to Administrators.'
    }

    $systemWebViewRoots = @()
    foreach ($programFilesRoot in @([Environment]::GetEnvironmentVariable('ProgramFiles(x86)'), $env:ProgramFiles)) {
        if ($programFilesRoot) {
            $systemWebViewRoots += Join-Path $programFilesRoot 'Microsoft\EdgeWebView\Application'
        }
    }

    function Find-WebView2Runtime {
        param([string[]] $Roots)
        foreach ($root in $Roots) {
            if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
            foreach ($directory in Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue) {
                if ($directory.Name -match '^\d+(\.\d+){1,3}$' -and (Test-Path -LiteralPath (Join-Path $directory.FullName 'msedgewebview2.exe') -PathType Leaf)) {
                    return $directory.FullName
                }
            }
        }
        return $null
    }

    $webViewRuntimePath = Find-WebView2Runtime -Roots $systemWebViewRoots
    if (-not $webViewRuntimePath) {
        Write-Host 'No system WebView2 Runtime is present; installing the Microsoft-signed Evergreen Runtime for the temporary standard user.'
        $webViewBootstrapper = Join-Path $stageRoot 'MicrosoftEdgeWebview2Setup.exe'
        Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $webViewBootstrapper -TimeoutSec 120
        $bootstrapperSignature = Get-AuthenticodeSignature -LiteralPath $webViewBootstrapper
        if ($bootstrapperSignature.Status -ne 'Valid' -or $bootstrapperSignature.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation') {
            throw 'The downloaded WebView2 bootstrapper does not have a valid Microsoft signature.'
        }

        $runtimeInstaller = Start-Process -FilePath $webViewBootstrapper -ArgumentList @('/silent', '/install') -Credential $credential -LoadUserProfile -WindowStyle Hidden -PassThru -Wait
        if ($runtimeInstaller.ExitCode -notin @(0, 3010)) {
            throw "The Microsoft WebView2 bootstrapper exited with code $($runtimeInstaller.ExitCode)."
        }

        $profileKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$userSid"
        $profileDeadline = [DateTime]::UtcNow.AddSeconds(45)
        while (-not (Test-Path -LiteralPath $profileKey) -and [DateTime]::UtcNow -lt $profileDeadline) {
            Start-Sleep -Milliseconds 250
        }
        if (-not (Test-Path -LiteralPath $profileKey)) {
            throw 'The WebView2 bootstrapper did not create the temporary standard-user profile.'
        }
        $profilePath = [Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath $profileKey -Name ProfileImagePath).ProfileImagePath)
        $perUserWebViewRoot = Join-Path $profilePath 'AppData\Local\Microsoft\EdgeWebView\Application'
        $webViewRoots = @($perUserWebViewRoot) + $systemWebViewRoots

        $runtimeDeadline = [DateTime]::UtcNow.AddSeconds(120)
        while ([DateTime]::UtcNow -lt $runtimeDeadline) {
            $webViewRuntimePath = Find-WebView2Runtime -Roots $webViewRoots
            if ($webViewRuntimePath) { break }
            Start-Sleep -Milliseconds 500
        }
        if (-not $webViewRuntimePath) {
            throw 'The signed WebView2 bootstrapper finished but did not install a runtime visible to the temporary standard user.'
        }
    }
    Write-Host "WebView2 Runtime preflight passed: $webViewRuntimePath"
    $report.webview2RuntimePrepared = $true

    Write-Host 'Installing the release as a temporary standard user with no shortcuts or registry registration.'
    $profileInitializer = Start-Process -FilePath (Get-Command pwsh.exe -ErrorAction Stop).Source -ArgumentList @('-NoLogo', '-NoProfile', '-NonInteractive', '-Command', 'exit 0') -Credential $credential -LoadUserProfile -WindowStyle Hidden -PassThru -Wait
    if ($profileInitializer.ExitCode -ne 0) {
        throw "The temporary user profile initializer exited with code $($profileInitializer.ExitCode)."
    }
    $profileKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$userSid"
    if (-not (Test-Path -LiteralPath $profileKey)) {
        throw 'The temporary standard-user profile was not created.'
    }
    $profilePath = [Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath $profileKey -Name ProfileImagePath).ProfileImagePath)
    $userTempDirectory = Join-Path $profilePath 'AppData\Local\Temp'
    New-Item -ItemType Directory -Path $userTempDirectory -Force | Out-Null
    $testInstallerPath = Join-Path $userTempDirectory "Pip-Setup-$version-Windows-x64.exe"
    Copy-Item -LiteralPath $installerPath -Destination $testInstallerPath -Force
    $diagnosticPath = Join-Path $userTempDirectory 'Pip-install-diagnostic.json'
    $quotedTestInstallerPath = $testInstallerPath.Replace("'", "''")
    $quotedDiagnosticPath = $diagnosticPath.Replace("'", "''")
    $installDiagnosticWrapper = @'
$ErrorActionPreference = 'Continue'
$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [System.Security.Principal.WindowsPrincipal]::new($identity)
$adminRole = [System.Security.Principal.WindowsBuiltInRole]::Administrator
$runtimeRoots = @(
    [Environment]::GetEnvironmentVariable('ProgramFiles(x86)'),
    $env:ProgramFiles,
    (Join-Path $env:LOCALAPPDATA 'Microsoft\EdgeWebView\Application')
) | Where-Object { $_ }
$runtimeFound = $false
foreach ($root in $runtimeRoots) {
    if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
    foreach ($versionDirectory in Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue) {
        if ($versionDirectory.Name -match '^\d+(\.\d+){1,3}$' -and (Test-Path -LiteralPath (Join-Path $versionDirectory.FullName 'msedgewebview2.exe') -PathType Leaf)) {
            $runtimeFound = $true
            break
        }
    }
    if ($runtimeFound) { break }
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class PipSmokeTokenDiagnostics {
    [StructLayout(LayoutKind.Sequential)]
    private struct TOKEN_ELEVATION { public int TokenIsElevated; }
    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);
    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool GetTokenInformation(IntPtr token, int infoClass, out TOKEN_ELEVATION elevation, int size, out int returned);
    public static int? GetElevation() {
        IntPtr token;
        if (!OpenProcessToken(GetCurrentProcess(), 0x0008, out token)) return null;
        try {
            TOKEN_ELEVATION value;
            int returned;
            if (!GetTokenInformation(token, 20, out value, Marshal.SizeOf(typeof(TOKEN_ELEVATION)), out returned)) return null;
            return value.TokenIsElevated;
        } finally { CloseHandle(token); }
    }
}
"@

$diagnostic = [ordered]@{
    username = $env:USERNAME
    sid = $identity.User.Value
    expectedSid = '__EXPECTED_SID__'
    identityMatchesTemporaryUser = ($identity.User.Value -eq '__EXPECTED_SID__')
    isAdministrator = $principal.IsInRole($adminRole)
    tokenElevated = [PipSmokeTokenDiagnostics]::GetElevation()
    userProfile = $env:USERPROFILE
    localAppData = $env:LOCALAPPDATA
    runtimeDetected = $runtimeFound
    installerPath = '__INSTALLER_PATH__'
    installerOutput = @()
    installerExitCode = $null
    installDirectoryExists = $false
    filesAfterExit = @()
}
try {
    $installerOutput = @(& '__INSTALLER_PATH__' --silent --no-shortcuts --no-register 2>&1 | ForEach-Object { $_.ToString() })
    $diagnostic.installerExitCode = $LASTEXITCODE
    $diagnostic.installerOutput = $installerOutput
} catch {
    $diagnostic.installerExitCode = 1
    $diagnostic.installerOutput = @($_.ToString())
}
$installDirectory = Join-Path $env:LOCALAPPDATA 'Programs\Pip'
$diagnostic.installDirectoryExists = Test-Path -LiteralPath $installDirectory -PathType Container
if ($diagnostic.installDirectoryExists) {
    $diagnostic.filesAfterExit = @(Get-ChildItem -LiteralPath $installDirectory -Force | ForEach-Object { $_.Name })
}
[System.IO.File]::WriteAllText('__DIAGNOSTIC_PATH__', ($diagnostic | ConvertTo-Json -Compress -Depth 5), [System.Text.UTF8Encoding]::new($false))
exit ([int]$diagnostic.installerExitCode)
'@
    $installDiagnosticWrapper = $installDiagnosticWrapper.Replace('__EXPECTED_SID__', $userSid).
        Replace('__INSTALLER_PATH__', $quotedTestInstallerPath).
        Replace('__DIAGNOSTIC_PATH__', $quotedDiagnosticPath)
    $diagnosticWrapperPath = Join-Path $userTempDirectory 'Pip-install-diagnostic.ps1'
    [System.IO.File]::WriteAllText($diagnosticWrapperPath, $installDiagnosticWrapper, [System.Text.UTF8Encoding]::new($false))
    $profilePwshPath = (Get-Command pwsh.exe -ErrorAction Stop).Source
    Write-Host 'Installing exact release under the standard user from that user profile, with stdout/stderr and token diagnostics captured.'

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $profilePwshPath
    $startInfo.WorkingDirectory = $userTempDirectory
    $startInfo.UserName = $userName
    $startInfo.Domain = $env:COMPUTERNAME
    $startInfo.Password = $securePassword
    $startInfo.LoadUserProfile = $true
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.ArgumentList.Add('-NoLogo')
    $startInfo.ArgumentList.Add('-NoProfile')
    $startInfo.ArgumentList.Add('-NonInteractive')
    $startInfo.ArgumentList.Add('-File')
    $startInfo.ArgumentList.Add($diagnosticWrapperPath)

    $installerProcess = [System.Diagnostics.Process]::new()
    $installerProcess.StartInfo = $startInfo
    if (-not $installerProcess.Start()) {
        throw 'Could not start the temporary-user installer diagnostic process.'
    }
    $wrapperStdOutTask = $installerProcess.StandardOutput.ReadToEndAsync()
    $wrapperStdErrTask = $installerProcess.StandardError.ReadToEndAsync()
    $installerProcess.WaitForExit()
    $wrapperStdOut = $wrapperStdOutTask.GetAwaiter().GetResult()
    $wrapperStdErr = $wrapperStdErrTask.GetAwaiter().GetResult()
    if ($wrapperStdOut) { Write-Host ('Installer wrapper stdout: ' + $wrapperStdOut.Trim()) }
    if ($wrapperStdErr) { Write-Host ('Installer wrapper stderr: ' + $wrapperStdErr.Trim()) }
    if (-not (Test-Path -LiteralPath $diagnosticPath -PathType Leaf)) {
        throw "The temporary-user installer wrapper exited with code $($installerProcess.ExitCode) without writing diagnostic data."
    }
    $installerDiagnostics = Get-Content -LiteralPath $diagnosticPath -Raw | ConvertFrom-Json
    Write-Host ('Exact installer diagnostic: ' + ($installerDiagnostics | ConvertTo-Json -Compress -Depth 5))
    if ($installerProcess.ExitCode -ne 0 -or $installerDiagnostics.installerExitCode -ne 0) {
        throw "The published installer exited with code $($installerDiagnostics.installerExitCode) under temporary SID $($installerDiagnostics.sid)."
    }

    $profileKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$userSid"
    if (-not (Test-Path -LiteralPath $profileKey)) {
        throw 'The temporary standard-user profile was not created.'
    }
    $profilePath = [Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath $profileKey -Name ProfileImagePath).ProfileImagePath)
    $installDir = Join-Path $profilePath 'AppData\Local\Programs\Pip'
    $pipPath = Join-Path $installDir 'Pip.exe'
    foreach ($required in @($pipPath, (Join-Path $installDir '.pip-installed'), (Join-Path $installDir 'uninstall.exe'))) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "The published installer did not produce its expected per-user file: $required"
        }
    }
    $report.installed = $true

    $quotedPipPath = $pipPath.Replace("'", "''")
    $wrapper = @'
$ErrorActionPreference = 'Stop'
try {
    $settingsDirectory = Join-Path $env:APPDATA 'evergreen-browser'
    New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $settingsDirectory 'settings.json'), '{"google_dns":true}', [System.Text.UTF8Encoding]::new($false))
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-address=127.0.0.1 --remote-debugging-port=__DEBUG_PORT__'
    $browser = Start-Process -FilePath '__PIP_PATH__' -ArgumentList 'https://www.youtube.com/' -PassThru
    [System.IO.File]::WriteAllText((Join-Path $settingsDirectory 'smoke-pid.txt'), $browser.Id.ToString(), [System.Text.UTF8Encoding]::new($false))
    exit 0
} catch {
    [Console]::Error.WriteLine($_)
    exit 1
}
'@
    $wrapper = $wrapper.Replace('__DEBUG_PORT__', [string]$debugPort).Replace('__PIP_PATH__', $quotedPipPath)
    $encodedWrapper = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($wrapper))
    $pwshPath = (Get-Command pwsh.exe -ErrorAction Stop).Source

    Write-Host 'Blocking outbound system DNS so a successful page load proves Google DoH handled hostname resolution.'
    foreach ($protocol in @('UDP', 'TCP')) {
        $ruleName = "PipReleaseSmoke-$userName-$protocol"
        New-NetFirewallRule -DisplayName $ruleName -Direction Outbound -Action Block -Profile Any -Protocol $protocol -RemotePort 53 | Out-Null
        $firewallRuleNames += $ruleName
    }
    $report.systemDnsFallbackBlocked = $true

    Write-Host 'Starting the installed Pip executable under the same standard user with Google DNS enabled.'
    $launcher = Start-Process -FilePath $pwshPath -ArgumentList @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', $encodedWrapper) -Credential $credential -LoadUserProfile -WindowStyle Hidden -PassThru -Wait
    if ($launcher.ExitCode -ne 0) {
        throw "The standard-user browser launcher exited with code $($launcher.ExitCode)."
    }

    $settingsPath = Join-Path $profilePath 'AppData\Roaming\evergreen-browser\settings.json'
    if (-not (Test-Path -LiteralPath $settingsPath -PathType Leaf)) {
        throw 'Pip did not receive the temporary Google DNS test settings.'
    }
    $savedSettings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
    if ($savedSettings.google_dns -ne $true) {
        throw 'The installed Pip profile did not enable Google DNS.'
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(70)
    $webviewCommandLine = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        $webviews = Get-SmokeProcesses -Name 'msedgewebview2.exe'
        foreach ($webview in $webviews) {
            if ($webview.CommandLine -match '--proxy-server=http://127\.0\.0\.1:\d+') {
                $webviewCommandLine = [string]$webview.CommandLine
                break
            }
        }
        if ($webviewCommandLine) { break }
        Start-Sleep -Milliseconds 500
    }
    if (-not $webviewCommandLine) {
        throw 'The installed WebView2 process did not expose Pip loopback Google DNS proxy argument.'
    }
    $report.googleDnsProxyConfigured = $true
    if ($webviewCommandLine -notmatch '--host-resolver-rules=.*MAP\s+\*\s+~NOTFOUND') {
        throw 'The installed WebView2 process did not block direct hostname DNS while Google DNS was enabled.'
    }
    $report.hostResolverDirectDnsBlocked = $true

    $target = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(70)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $targets = Invoke-RestMethod -Uri "http://127.0.0.1:$debugPort/json/list" -TimeoutSec 3
            $target = $targets | Where-Object { $_.type -eq 'page' -and $_.url -match '^https://[^/]*youtube\.com/' } | Select-Object -First 1
            if ($target -and $target.webSocketDebuggerUrl) { break }
        } catch {
            # The port appears only after WebView2 starts its browser process.
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $target) {
        throw 'Could not inspect the installed Pip page through its loopback-only WebView2 DevTools endpoint.'
    }

    $webSocket = [System.Net.WebSockets.ClientWebSocket]::new()
    $connectTimeout = [System.Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(10))
    try {
        $webSocket.ConnectAsync([Uri]$target.webSocketDebuggerUrl, $connectTimeout.Token).GetAwaiter().GetResult()
    } finally {
        $connectTimeout.Dispose()
    }

    $expression = @'
(()=> {
  const expected = 'youtube.com';
  const hostname = location.hostname.toLowerCase();
  const pageText = (document.body?.innerText || '').replace(/\s+/g, ' ').trim();
  return {
    loaded: (hostname === expected || hostname.endsWith('.' + expected)) &&
      document.readyState === 'complete' && pageText.length > 0,
    hostname,
    title: document.title,
    offlineUiDetected: /connect to the internet|you'?re offline|you are offline|no internet connection/i.test(pageText)
  };
})()
'@
    $requestId = 1
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    $page = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        $response = Send-CdpRequest -Socket $webSocket -RequestId $requestId -Method 'Runtime.evaluate' -Params @{ expression = $expression; returnByValue = $true; awaitPromise = $true }
        $requestId++
        if ($response.ContainsKey('error')) {
            Start-Sleep -Milliseconds 500
            continue
        }
        $page = $response.result.result.value
        if ($page.offlineUiDetected -eq $true) {
            $report.hostname = $page.hostname
            $report.title = $page.title
            $report.offlineUiDetected = $true
            throw 'The installed Pip release displayed YouTube offline UI with Google DNS enabled.'
        }
        if ($page.loaded -eq $true -and $page.offlineUiDetected -eq $false) {
            break
        }
        Start-Sleep -Milliseconds 750
    }

    if (-not $page -or $page.loaded -ne $true -or $page.offlineUiDetected -ne $false) {
        throw 'The installed Pip release did not reach usable YouTube content before the 60-second deadline.'
    }
    $report.youtubeLoaded = $true
    $report.offlineUiDetected = $false
    $report.hostname = [string]$page.hostname
    $report.title = [string]$page.title
    Write-Host ('PASS: ' + ($report | ConvertTo-Json -Compress))
} finally {
    if ($webSocket) {
        try {
            if ($webSocket.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
                $closeTimeout = [System.Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(2))
                try {
                    $webSocket.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, 'smoke complete', $closeTimeout.Token).GetAwaiter().GetResult()
                } finally {
                    $closeTimeout.Dispose()
                }
            }
        } catch {}
        $webSocket.Dispose()
    }

    foreach ($ruleName in $firewallRuleNames) {
        try { Remove-NetFirewallRule -DisplayName $ruleName -ErrorAction Stop } catch {
            Write-Warning "Temporary DNS firewall rule cleanup could not complete: $($_.Exception.Message)"
        }
    }

    if ($userName) {
        foreach ($processName in @('Pip.exe', 'msedgewebview2.exe', 'pwsh.exe')) {
            foreach ($process in @(Get-SmokeProcesses -Name $processName)) {
                try { Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop } catch {}
            }
        }
        Start-Sleep -Milliseconds 750
    }

    if ($userSid) {
        try {
            $profile = Get-CimInstance -ClassName Win32_UserProfile |
                Where-Object { $_.SID -eq $userSid -and -not $_.Special } |
                Select-Object -First 1
            if ($profile) {
                if ($profile.Loaded) {
                    throw 'The temporary Pip smoke profile is still loaded; refusing to remove it.'
                }
                $expectedProfile = $profilePath
                if (-not $expectedProfile) {
                    $profileKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$userSid"
                    if (Test-Path -LiteralPath $profileKey) {
                        $expectedProfile = [Environment]::ExpandEnvironmentVariables(
                            (Get-ItemProperty -LiteralPath $profileKey -Name ProfileImagePath).ProfileImagePath
                        )
                    }
                }
                if (-not $expectedProfile) {
                    throw 'The expected temporary smoke profile path is unavailable; refusing to remove a profile by SID alone.'
                }
                $resolvedProfile = [System.IO.Path]::GetFullPath([string]$profile.LocalPath)
                $expectedProfile = [System.IO.Path]::GetFullPath([string]$expectedProfile)
                if (-not [string]::Equals($resolvedProfile, $expectedProfile, [System.StringComparison]::OrdinalIgnoreCase)) {
                    throw 'Refusing to remove a profile whose canonical path does not exactly match the temporary Pip smoke user.'
                }
                Remove-CimInstance -InputObject $profile -ErrorAction Stop
            }
        } catch {
            Write-Warning "Temporary Pip smoke profile cleanup could not complete: $($_.Exception.Message)"
        }
        if ($createdUser) {
            try { Remove-LocalUser -SID ([System.Security.Principal.SecurityIdentifier]::new($userSid)) -ErrorAction Stop } catch {
                Write-Warning "Temporary Pip smoke account cleanup could not complete: $($_.Exception.Message)"
            }
        }
    }

    if ($stageRoot -and (Test-Path -LiteralPath $stageRoot)) {
        $resolvedStage = [System.IO.Path]::GetFullPath($stageRoot)
        $resolvedRunnerTemp = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
        if ($resolvedStage.StartsWith($resolvedRunnerTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedStage -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    $plainPassword = $null
    $securePassword = $null
    $credential = $null
}
