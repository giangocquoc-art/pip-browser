$ErrorActionPreference = 'Stop'
$helperPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'scripts/pip-dev-existing-artifacts.ps1'
. $helperPath

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("pip-dev-preflight-" + [guid]::NewGuid().ToString('N'))
$nodeModules = Join-Path $tempRoot 'node_modules'
$serverExe = Join-Path $tempRoot 'browseros-claw-server-rs.exe'
$browserExe = Join-Path $tempRoot 'PipDev.exe'
$wrongBrowser = Join-Path $tempRoot 'PipDev.txt'

function Assert-ThrowsContaining([scriptblock] $Action, [string] $Expected) {
  try {
    & $Action
  }
  catch {
    if ($_.Exception.Message.Contains($Expected)) { return }
    throw
  }
  throw "Expected preflight to fail with '$Expected'."
}

try {
  New-Item -ItemType Directory -Path $nodeModules -Force | Out-Null
  [IO.File]::WriteAllText($serverExe, '')
  [IO.File]::WriteAllText($browserExe, '')
  [IO.File]::WriteAllText($wrongBrowser, '')

  Assert-PipDevExistingArtifacts -BunVersion '1.4.2' -NodeModulesPath $nodeModules -ServerExePath $serverExe -BrowserBinary $browserExe
  Assert-ThrowsContaining { Assert-PipDevExistingArtifacts -BunVersion '1.4.1' -NodeModulesPath $nodeModules -ServerExePath $serverExe -BrowserBinary $browserExe } 'Bun 1.4.2 is required'
  Assert-ThrowsContaining { Assert-PipDevExistingArtifacts -BunVersion '1.4.2' -NodeModulesPath (Join-Path $tempRoot 'missing-node_modules') -ServerExePath $serverExe -BrowserBinary $browserExe } 'JavaScript dependencies are missing'
  Assert-ThrowsContaining { Assert-PipDevExistingArtifacts -BunVersion '1.4.2' -NodeModulesPath $nodeModules -ServerExePath (Join-Path $tempRoot 'missing-server.exe') -BrowserBinary $browserExe } 'prebuilt MCP server is missing'
  Assert-ThrowsContaining { Assert-PipDevExistingArtifacts -BunVersion '1.4.2' -NodeModulesPath $nodeModules -ServerExePath $serverExe -BrowserBinary (Join-Path $tempRoot 'missing-browser.exe') } 'browser executable is missing'
  Assert-ThrowsContaining { Assert-PipDevExistingArtifacts -BunVersion '1.4.2' -NodeModulesPath $nodeModules -ServerExePath $serverExe -BrowserBinary $wrongBrowser } 'must be a Windows .exe file'

  Write-Host 'Pip Dev existing-artifacts preflight tests passed.'
}
finally {
  Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
