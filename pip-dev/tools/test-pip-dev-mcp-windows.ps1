[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateNotNullOrEmpty()]
  [string] $BrowserBinary,
  [string] $ServerExe,
  [string] $ExtensionDirectory,
  [ValidateRange(1024, 65535)]
  [int] $ServerPort = 9200,
  [ValidateRange(1024, 65535)]
  [int] $CdpPort = 49337,
  [ValidateRange(10, 300)]
  [int] $StartupTimeoutSeconds = 90,
  [switch] $ShowBrowser,
  [switch] $RequireCrop,
  [switch] $KeepArtifacts
)

$ErrorActionPreference = 'Stop'
$pipDevRoot = Split-Path -Parent $PSScriptRoot
$agentRoot = Join-Path $pipDevRoot 'packages/browseros-agent'
$fixturePath = Join-Path $PSScriptRoot 'pip-dev-mcp-fixture.html'
$tempParent = 'D:\pc\pipdev-temp'
$runRoot = $null
$fixtureJob = $null
$serverProcess = $null
$browserProcess = $null
$mcpSessionId = $null
$failed = $false
$serverReady = $false
$savedEnvironment = @{}
$McpRequestId = 0

if (-not $ServerExe) {
  $ServerExe = Join-Path $agentRoot 'target/debug/browseros-claw-server-rs.exe'
}
function Resolve-ExistingFile([string] $Path, [string] $Label) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "$Label does not exist: $Path"
  }
  return (Resolve-Path -LiteralPath $Path).Path
}

function Resolve-ExistingDirectory([string] $Path, [string] $Label) {
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
    throw "$Label does not exist: $Path"
  }
  return (Resolve-Path -LiteralPath $Path).Path
}

function Test-LocalPortAvailable([int] $Port) {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
  try {
    $listener.Start()
    return $true
  }
  catch {
    return $false
  }
  finally {
    try { $listener.Stop() } catch { }
  }
}

function Get-FreeLoopbackPort {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  try {
    $listener.Start()
    return ([System.Net.IPEndPoint] $listener.LocalEndpoint).Port
  }
  finally {
    $listener.Stop()
  }
}

function Quote-Argument([string] $Value) {
  # Start-Process joins ArgumentList entries into one command line. Quote each
  # value here so a workspace or test-data path containing spaces remains one arg.
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

function Start-LoopbackFixture([int] $Port, [string] $HtmlPath) {
  return Start-Job -ArgumentList $Port, $HtmlPath -ScriptBlock {
    param([int] $ListenPort, [string] $PagePath)
    $ErrorActionPreference = 'Stop'
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $ListenPort)
    try {
      $listener.Start()
      $html = [System.IO.File]::ReadAllBytes($PagePath)
      $health = [System.Text.Encoding]::UTF8.GetBytes('{"ok":true}')
      while ($true) {
        if (-not $listener.Pending()) {
          Start-Sleep -Milliseconds 25
          continue
        }
        $client = $listener.AcceptTcpClient()
        try {
          $stream = $client.GetStream()
          $stream.ReadTimeout = 3000
          $reader = [System.IO.StreamReader]::new($stream, [System.Text.Encoding]::ASCII, $false, 1024, $true)
          $requestLine = $reader.ReadLine()
          while ($null -ne ($header = $reader.ReadLine()) -and $header.Length -gt 0) { }
          $path = if ($requestLine) { ($requestLine.Split(' ')[1] -split '\?')[0] } else { '/' }
          if ($path -eq '/health') {
            $body = $health
            $contentType = 'application/json; charset=utf-8'
            $status = '200 OK'
          }
          elseif ($path -eq '/' -or $path -eq '/fixture') {
            $body = $html
            $contentType = 'text/html; charset=utf-8'
            $status = '200 OK'
          }
          else {
            $body = [System.Text.Encoding]::UTF8.GetBytes('not found')
            $contentType = 'text/plain; charset=utf-8'
            $status = '404 Not Found'
          }
          $head = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: $contentType`r`nContent-Length: $($body.Length)`r`nConnection: close`r`nCache-Control: no-store`r`n`r`n")
          $stream.Write($head, 0, $head.Length)
          $stream.Write($body, 0, $body.Length)
          $stream.Flush()
          $reader.Dispose()
        }
        finally {
          $client.Dispose()
        }
      }
    }
    finally {
      $listener.Stop()
    }
  }
}

function Get-HttpResponse([string] $Uri, [int] $TimeoutSeconds = 3) {
  return Invoke-WebRequest -Uri $Uri -Method Get -TimeoutSec $TimeoutSeconds -UseBasicParsing
}

function Convert-McpBodyToJson([string] $Body) {
  if ([string]::IsNullOrWhiteSpace($Body)) { return $null }
  $jsonText = $Body.Trim()
  if ($jsonText -match '(?m)^data:\s*(\{.*\})\s*$') {
    $jsonText = $Matches[1]
  }
  return $jsonText | ConvertFrom-Json
}

function Send-McpRequest([string] $Method, $Params, [bool] $IsNotification = $false) {
  $script:McpRequestId++
  $message = [ordered]@{
    jsonrpc = '2.0'
    method = $Method
    params = $Params
  }
  if (-not $IsNotification) { $message.id = $script:McpRequestId }
  $headers = @{ Accept = 'application/json, text/event-stream' }
  if ($script:mcpSessionId) {
    $headers['mcp-session-id'] = $script:mcpSessionId
    $headers['mcp-protocol-version'] = '2025-06-18'
  }
  $response = Invoke-WebRequest `
    -Uri "http://127.0.0.1:$script:ServerPort/mcp" `
    -Method Post `
    -Headers $headers `
    -ContentType 'application/json' `
    -Body ($message | ConvertTo-Json -Depth 40 -Compress) `
    -TimeoutSec 45 `
    -UseBasicParsing
  if ($Method -eq 'initialize') {
    $sessionHeader = $response.Headers['mcp-session-id']
    if ($sessionHeader) { $script:mcpSessionId = [string]$sessionHeader }
  }
  if ($IsNotification) { return $null }
  return Convert-McpBodyToJson ([string] $response.Content)
}

function Invoke-McpTool([string] $Name, $Arguments) {
  $rpc = Send-McpRequest 'tools/call' @{ name = $Name; arguments = $Arguments }
  if ($rpc.error) {
    throw "MCP tool '$Name' returned JSON-RPC error: $($rpc.error.message)"
  }
  if ($rpc.result.isError) {
    $detail = (($rpc.result.content | ForEach-Object { $_.text }) -join "`n")
    throw "MCP tool '$Name' failed: $detail"
  }
  return $rpc.result
}

function Get-ToolText($ToolResult) {
  return (($ToolResult.content | Where-Object { $_.type -eq 'text' } | ForEach-Object { $_.text }) -join "`n")
}

function Stop-StartedProcessTree($Process) {
  if (-not $Process) { return }
  try { $Process.Refresh() } catch { return }
  if ($Process.HasExited) { return }
  # The PID is the exact process returned by Start-Process above; /T limits
  # cleanup to its descendants (Chromium renderer/GPU helpers), never by name.
  $taskkill = Join-Path $env:SystemRoot 'System32/taskkill.exe'
  if (Test-Path -LiteralPath $taskkill -PathType Leaf) {
    & $taskkill /PID $Process.Id /T /F 2>$null | Out-Null
  }
  else {
    Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
  }
}

function Assert-ToolAvailable($ToolList, [string] $Name) {
  if (-not ($ToolList | Where-Object { $_.name -eq $Name })) {
    throw "Required MCP tool '$Name' is missing from tools/list."
  }
}

try {
  $resolvedBrowser = Resolve-ExistingFile $BrowserBinary 'Patched Pip Dev/BrowserOS browser executable'
  if ([IO.Path]::GetExtension($resolvedBrowser) -ine '.exe') {
    throw "BrowserBinary must be a Windows .exe: $resolvedBrowser"
  }
  $resolvedServer = Resolve-ExistingFile $ServerExe 'Prebuilt BrowserOS neo MCP server executable'
  if (-not $ExtensionDirectory) {
    $extensionCandidates = @(
      (Join-Path $agentRoot 'apps/claw-app/dist/chrome-mv3-dev'),
      (Join-Path $agentRoot 'apps/claw-app/dist/chrome-mv3')
    )
    $ExtensionDirectory = $extensionCandidates | Where-Object {
      Test-Path -LiteralPath (Join-Path $_ 'manifest.json') -PathType Leaf
    } | Select-Object -First 1
    if (-not $ExtensionDirectory) {
      throw "Prebuilt Pip Dev extension manifest was not found. Build @browseros/claw-app, then pass -ExtensionDirectory (expected under $($extensionCandidates -join ' or '))."
    }
  }
  $resolvedExtension = Resolve-ExistingDirectory $ExtensionDirectory 'Prebuilt Pip Dev unpacked extension directory'
  $manifestPath = Join-Path $resolvedExtension 'manifest.json'
  if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "ExtensionDirectory must contain manifest.json: $resolvedExtension"
  }
  if (-not (Test-Path -LiteralPath $fixturePath -PathType Leaf)) {
    throw "Local fixture is missing: $fixturePath"
  }
  if (-not (Test-Path -LiteralPath 'D:\' -PathType Container)) {
    throw 'Drive D: is required by the Pip Dev Windows smoke test.'
  }
  if ($ServerPort -eq $CdpPort) {
    throw 'ServerPort and CdpPort must be different ports.'
  }
  foreach ($port in @($ServerPort, $CdpPort) | Select-Object -Unique) {
    if (-not (Test-LocalPortAvailable $port)) {
      throw "Loopback port $port is unavailable. No processes were started; choose another -ServerPort/-CdpPort."
    }
  }

  New-Item -ItemType Directory -Path $tempParent -Force | Out-Null
  $resolvedTempParent = (Resolve-Path -LiteralPath $tempParent).Path.TrimEnd('\')
  $runId = [Guid]::NewGuid().ToString('N')
  $runRoot = Join-Path $resolvedTempParent ("pipdev-mcp-smoke-{0}-{1}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'), $runId)
  $resolvedRunRoot = [IO.Path]::GetFullPath($runRoot).TrimEnd('\')
  if (-not $resolvedRunRoot.StartsWith($resolvedTempParent + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to create smoke data outside $resolvedTempParent"
  }
  New-Item -ItemType Directory -Path $runRoot -Force | Out-Null
  $profileDir = Join-Path $runRoot 'profile'
  $browserClawDir = Join-Path $runRoot 'browserclaw'
  $resourcesDir = Join-Path $runRoot 'resources'
  $executionDir = Join-Path $runRoot 'execution'
  $tempDir = Join-Path $runRoot 'temp'
  $configPath = Join-Path $runRoot 'claw-server.json'
  $serverStdout = Join-Path $runRoot 'server.stdout.log'
  $serverStderr = Join-Path $runRoot 'server.stderr.log'
  $browserStdout = Join-Path $runRoot 'browser.stdout.log'
  $browserStderr = Join-Path $runRoot 'browser.stderr.log'
  New-Item -ItemType Directory -Path $profileDir, $browserClawDir, $resourcesDir, $executionDir, $tempDir -Force | Out-Null

  $fixturePort = Get-FreeLoopbackPort
  $fixtureUrl = "http://127.0.0.1:$fixturePort/fixture"
  $sidecar = @{
    ports = @{ server = $ServerPort; cdp = $CdpPort; proxy = $ServerPort }
    directories = @{ resources = $resourcesDir; execution = $executionDir }
    flags = @{ devMode = $true; allowRemoteInMcp = $false }
  } | ConvertTo-Json -Depth 5
  [IO.File]::WriteAllText($configPath, $sidecar + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))

  foreach ($key in @('BROWSERCLAW_DIR', 'TEMP', 'TMP')) {
    $savedEnvironment[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
  }
  $env:BROWSERCLAW_DIR = $browserClawDir
  $env:TEMP = $tempDir
  $env:TMP = $tempDir

  $fixtureJob = Start-LoopbackFixture $fixturePort $fixturePath
  $fixtureDeadline = [DateTime]::UtcNow.AddSeconds(15)
  $fixtureReady = $false
  while ([DateTime]::UtcNow -lt $fixtureDeadline) {
    if ($fixtureJob.State -eq 'Failed') { throw "Loopback fixture failed: $($fixtureJob.ChildJobs[0].JobStateInfo.Reason)" }
    try {
      $fixtureResponse = Get-HttpResponse "http://127.0.0.1:$fixturePort/health" 2
      if ($fixtureResponse.StatusCode -eq 200) { $fixtureReady = $true; break }
    }
    catch { Start-Sleep -Milliseconds 150 }
  }
  if (-not $fixtureReady) { throw 'The loopback fixture did not become ready.' }

  $serverProcess = Start-Process `
    -FilePath $resolvedServer `
    -ArgumentList @('--config', (Quote-Argument $configPath)) `
    -WorkingDirectory $agentRoot `
    -WindowStyle Hidden `
    -PassThru `
    -RedirectStandardOutput $serverStdout `
    -RedirectStandardError $serverStderr

  $serverReadyUrl = "http://127.0.0.1:$ServerPort/system/health"
  $serverDeadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
  $serverReady = $false
  while ([DateTime]::UtcNow -lt $serverDeadline) {
    $serverProcess.Refresh()
    if ($serverProcess.HasExited) { throw "MCP server exited early ($($serverProcess.ExitCode)). See $serverStderr" }
    try {
      $health = Get-HttpResponse $serverReadyUrl 2
      if ($health.StatusCode -eq 200) { $serverReady = $true; break }
    }
    catch { Start-Sleep -Milliseconds 300 }
  }
  if (-not $serverReady) { throw "MCP server health did not become 200 at $serverReadyUrl." }

  $browserArgs = @(
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-sync',
    '--disable-default-apps',
    '--test-type',
    '--disable-browseros-server',
    '--disable-browseros-extensions',
    '--browseros-product=browserclaw',
    "--remote-debugging-port=$CdpPort",
    "--browseros-mcp-port=$ServerPort",
    "--browseros-server-port=$ServerPort",
    "--browseros-proxy-port=$ServerPort",
    '--window-size=1280,800',
    (Quote-Argument "--crash-dumps-dir=$tempDir"),
    (Quote-Argument "--disk-cache-dir=$(Join-Path $runRoot 'cache')"),
    (Quote-Argument "--user-data-dir=$profileDir"),
    (Quote-Argument "--load-extension=$resolvedExtension"),
    'about:blank'
  )
  $browserProcess = Start-Process `
    -FilePath $resolvedBrowser `
    -ArgumentList $browserArgs `
    -WorkingDirectory $runRoot `
    -WindowStyle $(if ($ShowBrowser) { 'Normal' } else { 'Hidden' }) `
    -PassThru `
    -RedirectStandardOutput $browserStdout `
    -RedirectStandardError $browserStderr

  Write-Host "Browser and server test state: $runRoot"
  Write-Host "Fixture URL (loopback only): $fixtureUrl"
  $readyDeadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
  $browserReady = $false
  while ([DateTime]::UtcNow -lt $readyDeadline) {
    $serverProcess.Refresh()
    $browserProcess.Refresh()
    if ($serverProcess.HasExited) { throw "MCP server exited while waiting for Chromium ($($serverProcess.ExitCode)). See $serverStderr" }
    if ($browserProcess.HasExited) { throw "Pip Dev Chromium exited early ($($browserProcess.ExitCode)). See $browserStderr" }
    try {
      $ready = Get-HttpResponse "http://127.0.0.1:$ServerPort/system/ready" 2
      if ($ready.StatusCode -eq 200) { $browserReady = $true; break }
    }
    catch { Start-Sleep -Milliseconds 500 }
  }
  if (-not $browserReady) { throw 'MCP server health was live, but /system/ready never reported a connected BrowserOS browser.' }
  Write-Host 'PASS /system/health and /system/ready (patched Chromium connected)'

  $initialize = Send-McpRequest 'initialize' @{
    protocolVersion = '2025-06-18'
    capabilities = @{}
    clientInfo = @{ name = 'PipDev-WindowsSmoke'; version = '1.0' }
  }
  if (-not $initialize.result.serverInfo) { throw "MCP initialize failed: $($initialize | ConvertTo-Json -Depth 8 -Compress)" }
  # The session id is an HTTP response header, not part of the JSON-RPC result.
  if (-not $mcpSessionId) { throw 'MCP initialize response did not include mcp-session-id.' }
  Send-McpRequest 'notifications/initialized' @{} $true | Out-Null

  $toolListRpc = Send-McpRequest 'tools/list' @{}
  $toolList = @($toolListRpc.result.tools)
  foreach ($name in @('tabs', 'snapshot', 'act', 'diff', 'visual_targets', 'screenshot')) {
    Assert-ToolAvailable $toolList $name
  }
  Write-Host "PASS MCP initialize and tools/list ($($toolList.Count) tools; required DOM/canvas tools present)"

  $newTab = Invoke-McpTool 'tabs' @{ action = 'new'; url = $fixtureUrl }
  $newTabText = Get-ToolText $newTab
  $pageMatch = [regex]::Match($newTabText, '(?im)opened page\s+(\d+)')
  if (-not $pageMatch.Success -and $newTab.structuredContent.page) {
    $pageId = [int] $newTab.structuredContent.page
  }
  elseif ($pageMatch.Success) {
    $pageId = [int] $pageMatch.Groups[1].Value
  }
  else {
    throw "tabs(action=new) did not return a page id. Output: $newTabText"
  }

  $listedTabs = Invoke-McpTool 'tabs' @{ action = 'list' }
  if ((Get-ToolText $listedTabs) -notmatch "(?m)^\[$pageId\]\s+$([regex]::Escape($fixtureUrl))") {
    throw "tabs(action=list) did not include the fixture page id $pageId. Output: $(Get-ToolText $listedTabs)"
  }
  Write-Host "PASS tabs new/list selection (page $pageId)"

  $snapshot = $null
  $snapshotText = ''
  $snapshotDeadline = [DateTime]::UtcNow.AddSeconds(20)
  while ([DateTime]::UtcNow -lt $snapshotDeadline) {
    try {
      $snapshot = Invoke-McpTool 'snapshot' @{ page = $pageId; mode = 'interactive' }
      $snapshotText = Get-ToolText $snapshot
      if ($snapshotText -match '(?i)Advance state') { break }
    }
    catch { }
    Start-Sleep -Milliseconds 250
  }
  if ($snapshotText -notmatch '(?i)Advance state') { throw "DOM snapshot did not expose fixture button. Output: $snapshotText" }
  $refMatch = [regex]::Match($snapshotText, '(?is)button\s+"Advance state"[^\r\n]*?\[ref=(e\d+)\]')
  if (-not $refMatch.Success) { $refMatch = [regex]::Match($snapshotText, '(?is)"Advance state"[^\r\n]*?\[ref=(e\d+)\]') }
  if (-not $refMatch.Success) { throw "Snapshot had the button but no actionable ref. Output: $snapshotText" }
  $elementRef = $refMatch.Groups[1].Value
  Write-Host "PASS DOM snapshot supplied a fresh button ref ($elementRef)"

  Invoke-McpTool 'act' @{ page = $pageId; kind = 'click'; ref = $elementRef; diff = 'none' } | Out-Null
  $diff = Invoke-McpTool 'diff' @{ page = $pageId }
  $diffText = Get-ToolText $diff
  $after = Invoke-McpTool 'snapshot' @{ page = $pageId; mode = 'interactive' }
  $afterText = Get-ToolText $after
  if ($afterText -notmatch 'State: verified') {
    throw "DOM action did not reach expected fixture state. Snapshot: $afterText`nDiff: $diffText"
  }
  if ($diffText -notmatch 'State: verified') {
    throw "diff did not report the expected post-action state. Output: $diffText"
  }
  Write-Host 'PASS ref-based DOM action and follow-up diff/state verification'

  $visual = Invoke-McpTool 'visual_targets' @{ page = $pageId; max_targets = 12 }
  $visualText = Get-ToolText $visual
  $regions = $null
  if ($visual.structuredContent.regions) {
    $regions = @($visual.structuredContent.regions)
  }
  else {
    foreach ($block in @($visual.content | Where-Object { $_.type -eq 'text' })) {
      try {
        $parsed = $block.text | ConvertFrom-Json
        if ($block.text.TrimStart().StartsWith('[')) { $regions = @($parsed); break }
      }
      catch { }
    }
  }
  if (-not $regions -or $regions.Count -eq 0) { throw "visual_targets returned no parseable visual regions. Output: $visualText" }
  $canvasRegion = $regions | Where-Object { $_.type -eq 'canvas' } | Select-Object -First 1
  if (-not $canvasRegion) { throw "visual_targets did not discover the fixture canvas. Output: $visualText" }
  if ([int]$canvasRegion.width -lt 12 -or [int]$canvasRegion.height -lt 12) {
    throw "visual_targets returned invalid canvas bounds: $($canvasRegion | ConvertTo-Json -Compress)"
  }
  Write-Host "PASS visual_targets found canvas at $($canvasRegion.x),$($canvasRegion.y) $($canvasRegion.width)x$($canvasRegion.height) CSS px"

  $observationId = $null
  if ($visual.structuredContent.observationId) { $observationId = [string]$visual.structuredContent.observationId }
  if (-not $observationId -and $visualText -match '(v1:\d+:[0-9a-fA-F]{8})') { $observationId = $Matches[1] }
  if ($observationId) {
    $crop = Invoke-McpTool 'screenshot' @{
      page = $pageId
      format = 'png'
      region = @{
        x = [int]$canvasRegion.x
        y = [int]$canvasRegion.y
        width = [int]$canvasRegion.width
        height = [int]$canvasRegion.height
        observationId = $observationId
      }
    }
    if (-not ($crop.content | Where-Object { $_.type -eq 'image' })) {
      throw "screenshot(region) did not return an inline image block: $(Get-ToolText $crop)"
    }
    Write-Host 'PASS target-bounded screenshot crop returned inline; the script did not save image bytes'
  }
  elseif ($RequireCrop) {
    throw 'visual_targets did not expose observationId, so a target-bounded crop could not be called. The MCP wire response must expose this token first.'
  }
  else {
    Write-Host 'SKIP screenshot(region): MCP tools/call did not expose visual_targets.observationId; no crop was requested.'
  }

  Write-Host 'SMOKE TEST PASSED'
}
catch {
  $failed = $true
  Write-Host "ERROR: $_"
  if ($serverStderr -and (Test-Path -LiteralPath $serverStderr)) {
    Write-Host '--- MCP server stderr tail ---'
    Get-Content -LiteralPath $serverStderr -Tail 30 | ForEach-Object { Write-Host $_ }
  }
  if ($browserStderr -and (Test-Path -LiteralPath $browserStderr)) {
    Write-Host '--- browser stderr tail ---'
    Get-Content -LiteralPath $browserStderr -Tail 30 | ForEach-Object { Write-Host $_ }
  }
  throw
}
finally {
  if ($browserProcess) { Stop-StartedProcessTree $browserProcess }
  if ($fixtureJob) {
    Stop-Job -Job $fixtureJob -ErrorAction SilentlyContinue
    Remove-Job -Job $fixtureJob -Force -ErrorAction SilentlyContinue
  }
  if ($serverProcess) {
    try { $serverProcess.Refresh() } catch { }
    if ($serverReady -and -not $serverProcess.HasExited) {
      try {
        Invoke-WebRequest -Uri "http://127.0.0.1:$ServerPort/system/shutdown" -Method Post -ContentType 'application/json' -Body '{}' -TimeoutSec 2 -UseBasicParsing | Out-Null
      }
      catch { }
    }
    try {
      if (-not $serverProcess.WaitForExit(5000)) { Stop-StartedProcessTree $serverProcess }
    }
    catch { Stop-StartedProcessTree $serverProcess }
  }
  foreach ($key in $savedEnvironment.Keys) {
    [Environment]::SetEnvironmentVariable($key, $savedEnvironment[$key], 'Process')
  }
  if ($runRoot -and (Test-Path -LiteralPath $runRoot)) {
    if ($KeepArtifacts -or $failed) {
      Write-Host "Temporary smoke artifacts retained under $runRoot"
    }
    else {
      $resolvedTempParent = (Resolve-Path -LiteralPath $tempParent).Path.TrimEnd('\')
      $resolvedRunRoot = [IO.Path]::GetFullPath($runRoot).TrimEnd('\')
      if ($resolvedRunRoot.StartsWith($resolvedTempParent + '\', [StringComparison]::OrdinalIgnoreCase) -and
          (Split-Path -Leaf $resolvedRunRoot) -like 'pipdev-mcp-smoke-*') {
        Remove-Item -LiteralPath $resolvedRunRoot -Recurse -Force
      }
      else {
        Write-Warning "Refusing to remove a path outside the generated smoke directory: $resolvedRunRoot"
      }
    }
  }
}
