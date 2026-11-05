param(
  [Parameter(Mandatory = $true)]
  [string] $BrowserBinary,
  [switch] $InstallDependencies,
  [switch] $UseExistingArtifacts,
  [ValidateRange(1, 32)]
  [int] $BuildJobs = 1,
  [ValidateRange(1024, 65535)]
  [int] $ServerPort = 9200,
  [ValidateRange(1024, 65535)]
  [int] $CdpPort = 49337,
  [ValidateRange(10, 300)]
  [int] $StartupTimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scripts/pip-dev-existing-artifacts.ps1')
if ($UseExistingArtifacts -and $InstallDependencies) {
  throw 'Use -UseExistingArtifacts or -InstallDependencies, not both.'
}
$repoRoot = $PSScriptRoot
$projectRoot = Split-Path -Parent $repoRoot
$agentRoot = Join-Path $repoRoot 'packages/browseros-agent'
$clawAppRoot = Join-Path $agentRoot 'apps/claw-app'
$serverExe = Join-Path $agentRoot 'target/debug/browseros-claw-server-rs.exe'
$cargoHome = Join-Path $projectRoot '.cargo-home'
$cargoBuildDir = Join-Path $agentRoot '.cargo-target'
$stateRoot = Join-Path $agentRoot '.data/windows-dev'
$buildTempDir = Join-Path $stateRoot 'temp'
$profileDir = Join-Path $stateRoot 'profile'
$browserClawDir = Join-Path $stateRoot 'browserclaw'
$configPath = Join-Path $stateRoot 'claw-server.json'
$resourcesDir = Join-Path $agentRoot 'resources'
$serverProcess = $null
$buildEnvironmentConfigured = $false
$originalBuildEnvironment = @{
  CARGO_HOME = $env:CARGO_HOME
  TEMP = $env:TEMP
  TMP = $env:TMP
}

function Test-LocalTcpPortInUse([int] $Port) {
  $client = [System.Net.Sockets.TcpClient]::new()
  try {
    $pending = $client.BeginConnect('127.0.0.1', $Port, $null, $null)
    if (-not $pending.AsyncWaitHandle.WaitOne(250)) { return $false }
    $client.EndConnect($pending)
    return $client.Connected
  }
  catch {
    return $false
  }
  finally {
    $client.Dispose()
  }
}

function Get-ServerLogTail([string] $Path) {
  if (Test-Path -LiteralPath $Path) {
    return (Get-Content -LiteralPath $Path -Tail 40 -ErrorAction SilentlyContinue) -join [Environment]::NewLine
  }
  return '(server log has not been created)'
}

try {
  $resolvedBinary = (Resolve-Path -LiteralPath $BrowserBinary).Path
  if (-not (Test-Path -LiteralPath $resolvedBinary -PathType Leaf)) {
    throw "BrowserBinary must point to an existing Windows BrowserOS/Pip Dev executable: $BrowserBinary"
  }
  if ([IO.Path]::GetExtension($resolvedBinary) -ine '.exe') {
    throw "BrowserBinary must be a Windows .exe file: $resolvedBinary"
  }
  if (-not (Test-Path -LiteralPath (Join-Path $agentRoot 'Cargo.toml') -PathType Leaf)) {
    throw "BrowserOS neo source was not found under $agentRoot"
  }
  if (-not (Test-Path -LiteralPath (Join-Path $agentRoot 'bun.lock') -PathType Leaf)) {
    throw "BrowserOS neo Bun lockfile was not found under $agentRoot"
  }

  $bunCommand = $null
  $bunInvocationPrefix = @()
  if ($UseExistingArtifacts) {
    $bun = Get-Command bun -ErrorAction SilentlyContinue
    if (-not $bun) {
      throw 'UseExistingArtifacts requires Bun 1.4.2 already installed on PATH; it will not invoke npm or download Bun.'
    }
    $bunCommand = $bun.Source
    $bunVersion = (& $bunCommand --version).Trim()
    $bunExitCode = $LASTEXITCODE
    if ($bunExitCode -ne 0 -or $bunVersion -ne '1.4.2') {
      throw "UseExistingArtifacts requires Bun 1.4.2 on PATH; found '$bunVersion'."
    }
    Assert-PipDevExistingArtifacts `
      -BunVersion $bunVersion `
      -NodeModulesPath (Join-Path $agentRoot 'node_modules') `
      -ServerExePath $serverExe `
      -BrowserBinary $resolvedBinary
    Write-Warning 'Existing-artifacts mode checks file presence only; it cannot verify the server matches current Rust source. Rebuild without -UseExistingArtifacts after changing Rust code or dependencies.'
  }
  else {
    $bun = Get-Command bun -ErrorAction SilentlyContinue
    $npm = Get-Command npm -ErrorAction SilentlyContinue
    if ($bun) {
      $bunCommand = $bun.Source
      $bunVersion = (& $bunCommand --version).Trim()
      $bunExitCode = $LASTEXITCODE
    }
    if (-not $bun -or $bunExitCode -ne 0 -or $bunVersion -ne '1.4.2') {
      if (-not $npm) {
        throw 'Bun 1.4.2 is required. Install Bun 1.4.2, or install Node.js/npm so this launcher can invoke the pinned Bun version.'
      }
      $bunCommand = $npm.Source
      $bunInvocationPrefix = @(
        'exec', '--yes', '--cache', (Join-Path $stateRoot 'npm-cache'),
        '--package', 'bun@1.4.2', '--', 'bun'
      )
      Write-Host 'Bun 1.4.2 is not on PATH; invoking the pinned version through npm.'
      $bunVersion = (& $bunCommand @bunInvocationPrefix --version).Trim()
      $bunExitCode = $LASTEXITCODE
    }
    if ($bunExitCode -ne 0 -or $bunVersion -ne '1.4.2') {
      throw "This BrowserOS checkout pins Bun 1.4.2; found '$bunVersion'."
    }
  }

  if (-not $UseExistingArtifacts) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) {
      throw 'Rust/Cargo is required to build the BrowserOS neo MCP server.'
    }
    $rustc = Get-Command rustc -ErrorAction SilentlyContinue
    if (-not $rustc) {
      throw 'rustc is required to build the BrowserOS neo MCP server. Install Rust with the MSVC Windows target.'
    }
    $rustInfo = & $rustc.Source -vV
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the installed Rust target.' }
    $hostLine = $rustInfo | Select-String '^host:'
    if (-not $hostLine) { throw 'rustc -vV did not report its host target.' }
    $rustTarget = $hostLine.ToString().Split(':', 2)[1].Trim()
    if ($rustTarget -notmatch 'windows-msvc$') {
      throw "BrowserOS neo Windows builds require the Rust MSVC target; current target is '$rustTarget'."
    }
  }

  foreach ($port in @($ServerPort, $CdpPort) | Select-Object -Unique) {
    if (Test-LocalTcpPortInUse $port) {
      throw "Port $port is already in use. Close the process using it or select another port with -ServerPort/-CdpPort."
    }
  }

  $runtimeDirectories = @($stateRoot, $profileDir, $browserClawDir, $buildTempDir)
  if (-not $UseExistingArtifacts) {
    $runtimeDirectories += @($cargoHome, $cargoBuildDir)
  }
  New-Item -ItemType Directory -Path $runtimeDirectories -Force | Out-Null
  $buildEnvironmentConfigured = $true
  if (-not $UseExistingArtifacts) {
    $env:CARGO_HOME = $cargoHome
  }
  $env:TEMP = $buildTempDir
  $env:TMP = $buildTempDir

  Push-Location $agentRoot
  try {
    if ($UseExistingArtifacts) {
      Write-Host 'Using validated existing artifacts; skipping bun install and Cargo build.'
    }
    else {
      if ($InstallDependencies -or -not (Test-Path -LiteralPath (Join-Path $agentRoot 'node_modules'))) {
        Write-Host 'Installing the locked BrowserOS neo JavaScript dependencies...'
        & $bunCommand @bunInvocationPrefix install --filter '@browseros/claw-app' --frozen-lockfile
        if ($LASTEXITCODE -ne 0) { throw "bun install failed with exit code $LASTEXITCODE" }
      }

      New-Item -ItemType Directory -Path $cargoBuildDir -Force | Out-Null
      $cargoBuildDirConfig = ([IO.Path]::GetFullPath($cargoBuildDir)).Replace('\', '/')
      $originalCargoBuildJobs = $env:CARGO_BUILD_JOBS
      try {
        $env:CARGO_BUILD_JOBS = [string] $BuildJobs
        Write-Host "Building the BrowserOS neo Rust MCP/API server for Windows with up to $BuildJobs Cargo jobs (cache: $cargoBuildDir)..."
        & $cargo.Source --config "build.build-dir=`"$cargoBuildDirConfig`"" build --locked -p claw-server
        if ($LASTEXITCODE -ne 0) { throw "cargo build -p claw-server failed with exit code $LASTEXITCODE" }
      }
      finally {
        if ($null -eq $originalCargoBuildJobs) {
          Remove-Item Env:CARGO_BUILD_JOBS -ErrorAction SilentlyContinue
        }
        else {
          $env:CARGO_BUILD_JOBS = $originalCargoBuildJobs
        }
      }
    }
  }
  finally {
    Pop-Location
  }

  if (-not (Test-Path -LiteralPath $serverExe -PathType Leaf)) {
    throw "The expected existing or newly built Windows server executable is missing: $serverExe"
  }

  $sidecar = @{
    ports = @{ server = $ServerPort; cdp = $CdpPort; proxy = $ServerPort }
    directories = @{ resources = $resourcesDir; execution = $stateRoot }
    flags = @{ devMode = $true; allow_remote_in_mcp = $false }
  } | ConvertTo-Json -Depth 5
  [IO.File]::WriteAllText(
    $configPath,
    $sidecar + [Environment]::NewLine,
    [Text.UTF8Encoding]::new($false)
  )

  $env:BROWSEROS_BINARY = $resolvedBinary
  $env:BROWSEROS_PRODUCT = 'browserclaw'
  $env:BROWSEROS_USER_DATA_DIR = $profileDir
  $env:BROWSEROS_CLAW_CDP_PORT = [string]$CdpPort
  $env:BROWSEROS_SERVER_PORT = [string]$ServerPort
  $env:BROWSERCLAW_DIR = $browserClawDir
  $env:VITE_BROWSEROS_CLAW_API_URL = "http://127.0.0.1:$ServerPort"

  $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
  $stdoutPath = Join-Path $stateRoot "claw-server-$stamp.out.log"
  $stderrPath = Join-Path $stateRoot "claw-server-$stamp.err.log"
  $serverProcess = Start-Process `
    -FilePath $serverExe `
    -ArgumentList ('--config "{0}"' -f $configPath) `
    -WorkingDirectory $agentRoot `
    -WindowStyle Hidden `
    -PassThru `
    -RedirectStandardOutput $stdoutPath `
    -RedirectStandardError $stderrPath

  $healthUrl = "http://127.0.0.1:$ServerPort/system/health"
  $deadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
  $serverReady = $false
  while ([DateTime]::UtcNow -lt $deadline) {
    $serverProcess.Refresh()
    if ($serverProcess.HasExited) {
      $logs = Get-ServerLogTail $stderrPath
      throw "BrowserOS neo MCP server exited with code $($serverProcess.ExitCode).`n$logs"
    }
    try {
      $response = Invoke-WebRequest -Uri $healthUrl -Method Get -TimeoutSec 2 -UseBasicParsing
      if ($response.StatusCode -eq 200) { $serverReady = $true; break }
    }
    catch {
      Start-Sleep -Milliseconds 500
    }
  }
  if (-not $serverReady) {
    $logs = Get-ServerLogTail $stderrPath
    throw "BrowserOS neo MCP server did not become ready at $healthUrl within $StartupTimeoutSeconds seconds.`n$logs"
  }

  Write-Host "BrowserOS neo MCP server is ready at http://127.0.0.1:$ServerPort/mcp"
  Write-Host "Launching Pip Dev with $resolvedBinary. Close the browser to stop the development session."
  Push-Location $clawAppRoot
  try {
    # Run WXT directly so Windows does not invoke the upstream Bash/Go supervisor
    # or require the macOS-only .env.development file. Runtime values are injected above.
    & $bunCommand @bunInvocationPrefix run wxt
    if ($LASTEXITCODE -ne 0) { throw "Pip Dev extension runner failed with exit code $LASTEXITCODE" }
  }
  finally {
    Pop-Location
  }
}
finally {
  if ($serverProcess) {
    $serverProcess.Refresh()
    if (-not $serverProcess.HasExited) {
      try {
        Invoke-WebRequest `
          -Uri "http://127.0.0.1:$ServerPort/system/shutdown" `
          -Method Post `
          -TimeoutSec 2 `
          -UseBasicParsing | Out-Null
      }
      catch { }
      try {
        if (-not $serverProcess.WaitForExit(5000)) {
          Stop-Process -Id $serverProcess.Id -Force -ErrorAction SilentlyContinue
        }
      }
      catch {
        Stop-Process -Id $serverProcess.Id -Force -ErrorAction SilentlyContinue
      }
    }
  }
  if ($buildEnvironmentConfigured) {
    foreach ($name in @('CARGO_HOME', 'TEMP', 'TMP')) {
      $value = $originalBuildEnvironment[$name]
      if ($null -eq $value) {
        Remove-Item "Env:$name" -ErrorAction SilentlyContinue
      }
      else {
        Set-Item "Env:$name" $value
      }
    }
  }
}
