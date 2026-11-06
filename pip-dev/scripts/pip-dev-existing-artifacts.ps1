function Assert-PipDevExistingArtifacts {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string] $BunVersion,
    [Parameter(Mandatory = $true)]
    [string] $NodeModulesPath,
    [Parameter(Mandatory = $true)]
    [string] $ServerExePath,
    [Parameter(Mandatory = $true)]
    [string] $BrowserBinary
  )

  $failures = [System.Collections.Generic.List[string]]::new()
  if ($BunVersion -ne '1.4.2') {
    $failures.Add("Bun 1.4.2 is required on PATH (found '$BunVersion').")
  }
  if (-not (Test-Path -LiteralPath $NodeModulesPath -PathType Container)) {
    $failures.Add("JavaScript dependencies are missing: $NodeModulesPath")
  }
  if (-not (Test-Path -LiteralPath $ServerExePath -PathType Leaf)) {
    $failures.Add("The prebuilt MCP server is missing: $ServerExePath")
  }
  if (-not (Test-Path -LiteralPath $BrowserBinary -PathType Leaf)) {
    $failures.Add("The browser executable is missing: $BrowserBinary")
  }
  elseif ([IO.Path]::GetExtension($BrowserBinary) -ine '.exe') {
    $failures.Add("The browser executable must be a Windows .exe file: $BrowserBinary")
  }

  if ($failures.Count -gt 0) {
    throw ("Existing-artifacts preflight failed: " + ($failures -join '; '))
  }
}
