<#
.SYNOPSIS
  Opt-in YouTube playback smoke test for a specific Pip.exe on Windows.

.DESCRIPTION
  Copies only the explicitly supplied Pip.exe into a new portable directory under
  the repository's ignored .data directory, enables Google DNS in that throwaway
  profile, and checks a public YouTube video through the local WebView2 DevTools
  Protocol endpoint.
  The JSON report and final page screenshot are retained in the run directory.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\check_youtube_playback.ps1 `
    -PipExe D:\path\to\Pip.exe

  The test opens a visible browser window and closes only the Pip.exe process it
  started. It does not use or modify the current Pip profile.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string] $PipExe,

    [bool] $GoogleDnsEnabled = $true,

    [string] $VideoUrl = 'https://www.youtube.com/watch?v=aqz-KE-bpKQ',

    [ValidateRange(30, 180)]
    [int] $TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$artifactBase = Join-Path $repositoryRoot '.data\youtube-playback'
$runId = '{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([Guid]::NewGuid().ToString('N').Substring(0, 8))
$runRoot = Join-Path $artifactBase "youtube-playback-$runId"
$isolatedExe = Join-Path $runRoot 'Pip.exe'
$dataRoot = Join-Path $runRoot 'user_data'
$tempRoot = Join-Path $runRoot 'temp'
$reportPath = Join-Path $runRoot 'youtube-playback-report.json'
$screenshotPath = Join-Path $runRoot 'youtube-final.png'
$netLogPath = Join-Path $runRoot 'netlog.json'

$process = $null
$socket = $null
$cdpId = 0
$mediaResponses = New-Object 'System.Collections.Generic.List[object]'
$networkFailures = New-Object 'System.Collections.Generic.List[object]'
$assertions = New-Object 'System.Collections.Generic.List[object]'
$report = [ordered]@{
    status = 'running'
    startedAt = [DateTime]::UtcNow.ToString('o')
    pipExe = $null
    pipExeSha256 = $null
    isolatedExe = $isolatedExe
    isolatedProfile = $dataRoot
    googleDnsEnabled = $GoogleDnsEnabled
    devtoolsAddress = '127.0.0.1'
    devtoolsPort = $null
    devtoolsListenerAddresses = @()
    videoUrl = $VideoUrl
    googleDnsProxyConfigured = $null
    directDnsTransactions = $null
    videoTitle = $null
    offlineUiDetected = $null
    playerBefore = $null
    playerAfter = $null
    mediaResponses = @()
    networkFailures = @()
    assertions = @()
    screenshot = $null
    processId = $null
    error = $null
    finishedAt = $null
}

function Add-Assertion {
    param([string] $Name, [bool] $Passed, [string] $Details)
    [void] $script:assertions.Add([ordered]@{
        name = $Name
        passed = $Passed
        details = $Details
    })
}

function Receive-CdpMessage {
    param(
        [System.Net.WebSockets.ClientWebSocket] $WebSocket,
        [int] $TimeoutMilliseconds = 1000
    )

    $stream = New-Object System.IO.MemoryStream
    $cancellation = New-Object System.Threading.CancellationTokenSource
    $cancellation.CancelAfter($TimeoutMilliseconds)
    try {
        do {
            $buffer = New-Object byte[] 65536
            $segment = New-Object 'System.ArraySegment[byte]' -ArgumentList (,$buffer)
            try {
                $result = $WebSocket.ReceiveAsync($segment, $cancellation.Token).GetAwaiter().GetResult()
            }
            catch [System.OperationCanceledException] {
                throw [System.TimeoutException]::new('Timed out waiting for a DevTools WebSocket message.')
            }
            if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
                throw 'The local DevTools WebSocket closed unexpectedly.'
            }
            $stream.Write($buffer, 0, $result.Count)
        } while (-not $result.EndOfMessage)

        $json = [System.Text.Encoding]::UTF8.GetString($stream.ToArray())
        return ConvertFrom-Json -InputObject $json
    }
    finally {
        $cancellation.Dispose()
        $stream.Dispose()
    }
}

function Save-CdpEvent {
    param($Message)

    if ($Message.method -eq 'Network.responseReceived') {
        $response = $Message.params.response
        try {
            $uri = [Uri]::new([string] $response.url)
            if ($uri.AbsolutePath -eq '/videoplayback' -and $uri.DnsSafeHost.EndsWith('.googlevideo.com', [StringComparison]::OrdinalIgnoreCase)) {
                [void] $script:mediaResponses.Add([ordered]@{
                    host = $uri.DnsSafeHost
                    path = $uri.AbsolutePath
                    status = [int] $response.status
                    mimeType = [string] $response.mimeType
                    fromDiskCache = [bool] $response.fromDiskCache
                })
            }
        }
        catch {
            # Ignore unrelated or malformed resource URLs; never retain query strings.
        }
    }
    elseif ($Message.method -eq 'Network.loadingFailed') {
        [void] $script:networkFailures.Add([ordered]@{
            requestId = [string] $Message.params.requestId
            resourceType = [string] $Message.params.type
            errorText = [string] $Message.params.errorText
            canceled = [bool] $Message.params.canceled
        })
    }
}

function Invoke-CdpCommand {
    param(
        [System.Net.WebSockets.ClientWebSocket] $WebSocket,
        [string] $Method,
        [hashtable] $Parameters = @{}
    )

    $script:cdpId++
    $id = $script:cdpId
    $payload = [ordered]@{ id = $id; method = $Method; params = $Parameters } |
        ConvertTo-Json -Depth 20 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    $segment = New-Object 'System.ArraySegment[byte]' -ArgumentList (,$bytes)
    [void] ($WebSocket.SendAsync(
        $segment,
        [System.Net.WebSockets.WebSocketMessageType]::Text,
        $true,
        [Threading.CancellationToken]::None
    ).GetAwaiter().GetResult())

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        $remainingMilliseconds = [int] [Math]::Max(1000, [Math]::Ceiling(($deadline - [DateTime]::UtcNow).TotalMilliseconds))
        try {
            $message = Receive-CdpMessage -WebSocket $WebSocket -TimeoutMilliseconds $remainingMilliseconds
        }
        catch [System.TimeoutException] {
            throw [System.TimeoutException]::new("Timed out waiting for local DevTools command: $Method")
        }
        Save-CdpEvent -Message $message
        if ($message.PSObject.Properties.Name -contains 'id' -and [int] $message.id -eq $id) {
            if ($message.error) {
                throw ("CDP {0} failed: {1}" -f $Method, $message.error.message)
            }
            return $message.result
        }
    }
    throw "Timed out waiting for the local DevTools command: $Method"
}

function Get-PlayerState {
    param([System.Net.WebSockets.ClientWebSocket] $WebSocket)

    $expression = @'
(()=>{
  const body = document.body?.innerText || '';
  const video = document.querySelector('video');
  const player = document.querySelector('#movie_player');
  const rect = video?.getBoundingClientRect();
  return {
    url: location.href,
    title: document.title,
    readyState: document.readyState,
    bodyText: body.replace(/\s+/g, ' ').slice(0, 1600),
    offlineUiDetected: /connect to the internet|you're offline|you are offline|no internet connection/i.test(body),
    video: video ? {
      paused: video.paused,
      readyState: video.readyState,
      currentTime: video.currentTime,
      duration: video.duration,
      videoWidth: video.videoWidth,
      videoHeight: video.videoHeight,
      networkState: video.networkState,
      error: video.error ? { code: video.error.code, message: video.error.message } : null,
      rect: rect ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : null
    } : null,
    playerState: player && typeof player.getPlayerState === 'function' ? player.getPlayerState() : null,
    playabilityStatus: player && typeof player.getPlayerResponse === 'function'
      ? (player.getPlayerResponse()?.playabilityStatus?.status || null)
      : null
  };
})()
'@
    $reply = Invoke-CdpCommand -WebSocket $WebSocket -Method 'Runtime.evaluate' -Parameters @{
        expression = $expression
        awaitPromise = $true
        returnByValue = $true
    }
    if ($reply.exceptionDetails) {
        throw ("Page inspection failed: {0}" -f $reply.exceptionDetails.exception.description)
    }
    return $reply.result.value
}

function Save-PageScreenshot {
    param([System.Net.WebSockets.ClientWebSocket] $WebSocket, [string] $Path)

    $capture = Invoke-CdpCommand -WebSocket $WebSocket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [IO.File]::WriteAllBytes($Path, [Convert]::FromBase64String([string] $capture.data))
    return $Path
}

try {
    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw 'PowerShell 5.1 or newer is required.'
    }
    $sourcePath = (Resolve-Path -LiteralPath $PipExe).Path
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "PipExe must point to a file: $PipExe"
    }
    $sourceItem = Get-Item -LiteralPath $sourcePath
    if ($sourceItem.Extension -ne '.exe') {
        throw "PipExe must be an .exe file: $sourcePath"
    }
    $videoUri = [Uri]::new($VideoUrl)
    if (-not $videoUri.IsAbsoluteUri -or $videoUri.Scheme -ne 'https' -or
        $videoUri.DnsSafeHost -notin @('youtube.com', 'www.youtube.com', 'm.youtube.com') -or
        $videoUri.AbsolutePath -ne '/watch' -or -not $videoUri.Query.Contains('v=')) {
        throw 'VideoUrl must be an https://www.youtube.com/watch?v=... URL.'
    }
    if ($VideoUrl.Contains('"') -or $VideoUrl -match '\s') {
        throw 'VideoUrl cannot contain quotes or whitespace.'
    }

    New-Item -ItemType Directory -Path $artifactBase -Force -ErrorAction Stop | Out-Null
    New-Item -ItemType Directory -Path $runRoot -ErrorAction Stop | Out-Null
    New-Item -ItemType Directory -Path $dataRoot, $tempRoot -ErrorAction Stop | Out-Null
    Copy-Item -LiteralPath $sourcePath -Destination $isolatedExe -ErrorAction Stop
    Set-Content -LiteralPath (Join-Path $runRoot 'portable.ini') -Value 'portable_mode=true' -Encoding ascii
    $dnsSettings = @{ google_dns = $GoogleDnsEnabled } | ConvertTo-Json -Compress
    Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json') -Value $dnsSettings -Encoding ascii

    $report.pipExe = $sourcePath
    $report.pipExeSha256 = (Get-FileHash -LiteralPath $isolatedExe -Algorithm SHA256).Hash.ToLowerInvariant()

    $portListener = [System.Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $portListener.Start()
    $port = ([Net.IPEndPoint] $portListener.LocalEndpoint).Port
    $portListener.Stop()
    $report.devtoolsPort = $port

    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $isolatedExe
    $startInfo.WorkingDirectory = $runRoot
    $startInfo.Arguments = '"{0}"' -f $videoUri.AbsoluteUri
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $false
    $startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Normal
    $startInfo.EnvironmentVariables['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] =
        "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port --remote-allow-origins=* --log-net-log=`"$netLogPath`""
    $startInfo.EnvironmentVariables['TEMP'] = $tempRoot
    $startInfo.EnvironmentVariables['TMP'] = $tempRoot
    $process = [System.Diagnostics.Process]::Start($startInfo)
    if (-not $process) {
        throw 'Could not start the isolated Pip.exe process.'
    }
    $report.processId = $process.Id

    $target = $null
    $connectDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not $target -and [DateTime]::UtcNow -lt $connectDeadline) {
        if ($process.HasExited) {
            throw "The isolated Pip.exe exited early with code $($process.ExitCode)."
        }
        try {
            $targets = Invoke-RestMethod -Uri "http://127.0.0.1:$port/json/list" -TimeoutSec 2 -UseBasicParsing
            $target = @($targets) | Where-Object {
                $_.type -eq 'page' -and $_.url -match '^https://(?:www\.|m\.)?youtube\.com/'
            } | Select-Object -First 1
        }
        catch {
            Start-Sleep -Milliseconds 350
        }
    }
    if (-not $target) {
        throw 'Timed out waiting for Pip local DevTools endpoint and YouTube page.'
    }
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue)
    if ($listeners.Count -eq 0) {
        throw 'Could not verify the local DevTools listener address.'
    }
    $listenerAddresses = @($listeners | ForEach-Object { [string] $_.LocalAddress } | Sort-Object -Unique)
    $report.devtoolsListenerAddresses = $listenerAddresses
    foreach ($address in $listenerAddresses) {
        $parsedAddress = [Net.IPAddress]::Parse($address)
        if (-not [Net.IPAddress]::IsLoopback($parsedAddress)) {
            throw "Refusing to use a non-loopback DevTools listener: $address"
        }
    }
    Add-Assertion -Name 'devtools_bound_to_loopback' -Passed $true `
        -Details ("Listener address(es): {0}" -f ($listenerAddresses -join ', '))
    $debugUri = [Uri]::new([string] $target.webSocketDebuggerUrl)
    if ($debugUri.Scheme -ne 'ws' -or $debugUri.Host -ne '127.0.0.1' -or $debugUri.Port -ne $port) {
        throw 'The DevTools WebSocket was not bound to the expected local test endpoint.'
    }

    $socket = New-Object System.Net.WebSockets.ClientWebSocket
    $socket.Options.Proxy = $null
    [void] ($socket.ConnectAsync($debugUri, [Threading.CancellationToken]::None).GetAwaiter().GetResult())
    [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Runtime.enable')
    [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Network.enable')
    [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Page.enable')
    [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Page.navigate' -Parameters @{ url = $VideoUrl })

    $loadDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $before = $null
    while ([DateTime]::UtcNow -lt $loadDeadline) {
        if ($process.HasExited) {
            throw "The isolated Pip.exe exited during playback with code $($process.ExitCode)."
        }
        $before = Get-PlayerState -WebSocket $socket
        if ($before.video -and [int] $before.video.readyState -ge 2 -and
            [int] $before.video.videoWidth -gt 0 -and [int] $before.video.videoHeight -gt 0) {
            break
        }
        Start-Sleep -Milliseconds 750
    }
    $report.playerBefore = $before
    $report.videoTitle = if ($before) { [string] $before.title } else { $null }
    if (-not $before -or -not $before.video -or [int] $before.video.readyState -lt 2 -or
        [int] $before.video.videoWidth -le 0 -or [int] $before.video.videoHeight -le 0) {
        throw 'YouTube did not produce a ready video element before the timeout.'
    }

    if ([bool] $before.video.paused) {
        $rect = $before.video.rect
        if (-not $rect -or [double] $rect.width -le 0 -or [double] $rect.height -le 0) {
            throw 'The paused video has no visible click target.'
        }
        $x = [Math]::Round([double] $rect.x + [double] $rect.width / 2)
        $y = [Math]::Round([double] $rect.y + [double] $rect.height / 2)
        [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Input.dispatchMouseEvent' -Parameters @{
            type = 'mouseMoved'; x = $x; y = $y
        })
        [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Input.dispatchMouseEvent' -Parameters @{
            type = 'mousePressed'; x = $x; y = $y; button = 'left'; clickCount = 1
        })
        [void] (Invoke-CdpCommand -WebSocket $socket -Method 'Input.dispatchMouseEvent' -Parameters @{
            type = 'mouseReleased'; x = $x; y = $y; button = 'left'; clickCount = 1
        })
    }

    Start-Sleep -Seconds 10
    $after = Get-PlayerState -WebSocket $socket
    $report.playerAfter = $after
    $report.videoTitle = [string] $after.title
    $report.offlineUiDetected = [bool] $after.offlineUiDetected
    $report.mediaResponses = @($mediaResponses.ToArray())
    $report.networkFailures = @($networkFailures.ToArray() | Select-Object -Last 50)
    $advance = [double] $after.video.currentTime - [double] $before.video.currentTime
    $successfulMedia = @($mediaResponses | Where-Object { $_.status -ge 200 -and $_.status -lt 300 })

    Add-Assertion -Name 'page_has_no_offline_ui' -Passed (-not [bool] $after.offlineUiDetected) `
        -Details 'The final YouTube page text was checked for common offline messages.'
    Add-Assertion -Name 'player_is_ready' -Passed ($after.video -and [int] $after.video.readyState -ge 2 -and
        [int] $after.video.videoWidth -gt 0 -and [int] $after.video.videoHeight -gt 0 -and -not $after.video.error) `
        -Details ("readyState={0}; size={1}x{2}; mediaError={3}" -f $after.video.readyState, $after.video.videoWidth, $after.video.videoHeight, ($null -ne $after.video.error))
    Add-Assertion -Name 'playback_time_advanced' -Passed ($advance -ge 1.5 -and -not [bool] $after.video.paused) `
        -Details ("currentTime advanced by {0:N2}s; paused={1}" -f $advance, $after.video.paused)
    Add-Assertion -Name 'successful_video_media_response' -Passed ($successfulMedia.Count -gt 0) `
        -Details ("Observed {0} successful googlevideo.com/videoplayback response(s)." -f $successfulMedia.Count)

    $report.screenshot = Save-PageScreenshot -WebSocket $socket -Path $screenshotPath
    $report.status = if (@($assertions | Where-Object { -not $_.passed }).Count -eq 0) { 'passed' } else { 'failed' }
    if ($report.status -ne 'passed') {
        throw 'One or more YouTube playback assertions failed; see the retained JSON report.'
    }
}
catch {
    $report.status = 'failed'
    $report.error = $_.Exception.Message
    if ($socket -and $socket.State -eq [System.Net.WebSockets.WebSocketState]::Open -and
        -not (Test-Path -LiteralPath $screenshotPath)) {
        try {
            $report.screenshot = Save-PageScreenshot -WebSocket $socket -Path $screenshotPath
        }
        catch {
            $report.screenshotError = $_.Exception.Message
        }
    }
}
finally {
    if ($socket) {
        try {
            if ($socket.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
                [void] $socket.CloseAsync(
                    [System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure,
                    'Pip YouTube smoke test finished',
                    [Threading.CancellationToken]::None
                ).GetAwaiter().GetResult()
            }
        }
        catch { }
        $socket.Dispose()
    }
    if ($process -and -not $process.HasExited) {
        [void] $process.CloseMainWindow()
        if (-not $process.WaitForExit(8000)) {
            # Fallback is limited to the exact Pip.exe process started above.
            $process.Kill()
            [void] $process.WaitForExit(5000)
        }
    }
    if (Test-Path -LiteralPath $runRoot -PathType Container) {
        if (Test-Path -LiteralPath $netLogPath -PathType Leaf) {
            try {
                $netLogItem = Get-Item -LiteralPath $netLogPath
                if ($netLogItem.Length -gt 134217728) {
                    throw 'NetLog exceeded the 128 MiB parsing limit.'
                }
                $netLog = Get-Content -LiteralPath $netLogPath -Raw | ConvertFrom-Json
                $eventNames = @{}
                foreach ($eventType in $netLog.constants.logEventTypes.PSObject.Properties) {
                    $eventNames[[string] $eventType.Value] = [string] $eventType.Name
                }
                $proxyConfigured = $false
                $proxyPorts = New-Object 'System.Collections.Generic.HashSet[int]'
                $dnsCount = 0
                foreach ($event in $netLog.events) {
                    $eventName = $eventNames[[string] $event.type]
                    if ($eventName -eq 'DNS_TRANSACTION') {
                        $dnsCount++
                    }
                    elseif ($eventName -eq 'PROXY_CONFIG_CHANGED' -and $event.params.new_config) {
                        $configJson = $event.params.new_config | ConvertTo-Json -Depth 10 -Compress
                        # Chromium may serialize the proxy as [127.0.0.1:PORT] in
                        # `single_proxy`; this port differs from the DevTools port.
                        foreach ($match in [regex]::Matches($configJson, '127\.0\.0\.1:(\d{1,5})')) {
                            $proxyPort = 0
                            if ([int]::TryParse($match.Groups[1].Value, [ref] $proxyPort) -and
                                $proxyPort -gt 0 -and $proxyPort -le 65535) {
                                [void] $proxyPorts.Add($proxyPort)
                            }
                        }
                    }
                }
                $proxyConfigured = $proxyPorts.Count -gt 0
                $report.googleDnsProxyConfigured = $proxyConfigured
                $report.googleDnsProxyPorts = @($proxyPorts | Sort-Object)
                $report.proxyConfigEventCount = @($netLog.events | Where-Object {
                    $eventNames[[string] $_.type] -eq 'PROXY_CONFIG_CHANGED'
                }).Count
                $report.directDnsTransactions = $dnsCount
                $dnsRouteMatchesSetting = $proxyConfigured -eq $GoogleDnsEnabled
                if ($GoogleDnsEnabled) {
                    $proxyDetails = 'Google DNS is enabled and WebView2 NetLog shows a loopback proxy.'
                }
                else {
                    $proxyDetails = 'Google DNS is disabled and WebView2 NetLog shows no loopback proxy.'
                }
                Add-Assertion -Name 'dns_route_matches_setting' -Passed $dnsRouteMatchesSetting `
                    -Details $proxyDetails
                if ($GoogleDnsEnabled) {
                    Add-Assertion -Name 'no_direct_dns_transactions' -Passed ($dnsCount -eq 0) `
                        -Details ("Observed {0} direct DNS_TRANSACTION event(s)." -f $dnsCount)
                }
                else {
                    Add-Assertion -Name 'system_dns_route_used' -Passed ($dnsCount -gt 0) `
                        -Details ("Observed {0} Windows DNS_TRANSACTION event(s) with Google DNS disabled." -f $dnsCount)
                }
            }
            catch {
                # ConvertFrom-Json can include the entire NetLog text in its exception.
                # Retain only the exception class; this artifact must never duplicate raw logs.
                $report.googleDnsEvidenceError = "WebView2 NetLog parse failed ({0}); raw log discarded." -f $_.Exception.GetType().Name
                Add-Assertion -Name 'google_dns_netlog_evidence' -Passed $false `
                    -Details $report.googleDnsEvidenceError
            }
            finally {
                # NetLog can contain URLs and request metadata; retain only the sanitized summary.
                Remove-Item -LiteralPath $netLogPath -Force -ErrorAction SilentlyContinue
            }
        }
        else {
            Add-Assertion -Name 'google_dns_netlog_evidence' -Passed $false `
                -Details 'The isolated run did not produce a WebView2 NetLog.'
        }
        if (@($assertions | Where-Object { -not $_.passed }).Count -gt 0) {
            $report.status = 'failed'
            if (-not $report.error) {
                $report.error = 'One or more playback or Google DNS assertions failed.'
            }
        }
        $report.mediaResponses = @($mediaResponses.ToArray())
        $report.networkFailures = @($networkFailures.ToArray() | Select-Object -Last 50)
        $report.assertions = @($assertions.ToArray())
        $report.finishedAt = [DateTime]::UtcNow.ToString('o')
        $report | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $reportPath -Encoding utf8
    }
}

Write-Output ("Pip YouTube playback smoke test: {0}" -f $report.status)
Write-Output ("Artifacts: {0}" -f $runRoot)
if ($report.status -ne 'passed') {
    if ($report.error) { Write-Error $report.error }
    exit 1
}
