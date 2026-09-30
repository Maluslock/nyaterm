[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$InstallerPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Find-7Zip {
  $command = Get-Command 7z.exe -ErrorAction SilentlyContinue
  if ($null -ne $command) {
    return $command.Source
  }

  $commonPaths = @(
    (Join-Path $env:ProgramFiles "7-Zip\7z.exe"),
    (Join-Path ${env:ProgramFiles(x86)} "7-Zip\7z.exe"),
    "C:\ProgramData\chocolatey\bin\7z.exe"
  )

  foreach ($path in $commonPaths) {
    if (Test-Path -LiteralPath $path -PathType Leaf) {
      return $path
    }
  }

  throw "Unable to find 7z.exe to inspect the NSIS installer payload."
}

if (!(Test-Path -LiteralPath $InstallerPath -PathType Leaf)) {
  throw "Windows 7 installer does not exist: $InstallerPath"
}

$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$sevenZip = Find-7Zip
$listing = (& $sevenZip l $resolvedInstaller 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0) {
  throw "7z.exe failed while inspecting ${resolvedInstaller}:`n$listing"
}

$requiredPatterns = @("webview2-fixed-runtime.*msedgewebview2\.exe")

$missing = @()
foreach ($pattern in $requiredPatterns) {
  if ($listing -notmatch $pattern) {
    $missing += $pattern
  }
}

if ($missing.Count -gt 0) {
  Write-Host "NSIS installer listing for diagnosis:"
  Write-Host $listing
  throw "Windows 7 offline installer is missing expected WebView2 fixed runtime payload entries:`n$($missing -join "`n")"
}

if ($listing -match '(?i)conpty\.dll|OpenConsole\.exe') {
  throw "Windows 7 installer must not contain bundled ConPTY files: $resolvedInstaller"
}

Write-Host "Windows 7 offline installer contains the WebView2 fixed runtime payload: $resolvedInstaller"
