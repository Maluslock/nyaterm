[CmdletBinding()]
param(
  [string]$InstallerPath,
  [string]$PortableZipPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($InstallerPath) -eq [string]::IsNullOrWhiteSpace($PortableZipPath)) {
  throw "Specify exactly one of InstallerPath or PortableZipPath."
}

$required = @(
  'conpty[\\/]x64[\\/]conpty\.dll',
  'conpty[\\/]x64[\\/]x64[\\/]OpenConsole\.exe',
  'conpty[\\/]x64[\\/]arm64[\\/]OpenConsole\.exe',
  'conpty[\\/]arm64[\\/]conpty\.dll',
  'conpty[\\/]arm64[\\/]arm64[\\/]OpenConsole\.exe',
  'conpty[\\/]LICENSE\.txt'
)

if ($InstallerPath) {
  $path = (Resolve-Path -LiteralPath $InstallerPath).Path
  if ([IO.Path]::GetExtension($path) -ieq '.msi') {
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $extractDir = Join-Path $tempRoot "nyaterm-msi-inspect-$([Guid]::NewGuid().ToString('N'))"
    try {
      $process = Start-Process msiexec.exe -ArgumentList @(
        '/a', "`"$path`"", '/qn', "TARGETDIR=`"$extractDir`""
      ) -Wait -PassThru -WindowStyle Hidden
      if ($process.ExitCode -ne 0) {
        throw "MSI administrative extraction failed with code $($process.ExitCode): $path"
      }
      $listing = (Get-ChildItem -LiteralPath $extractDir -Recurse -File |
        ForEach-Object { $_.FullName.Substring($extractDir.Length).TrimStart('\', '/') }) -join "`n"
    }
    finally {
      if ([IO.Path]::GetFullPath($extractDir).StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and
          (Split-Path -Leaf $extractDir) -like 'nyaterm-msi-inspect-*') {
        Remove-Item -LiteralPath $extractDir -Recurse -Force -ErrorAction SilentlyContinue
      }
    }
  }
  else {
    $sevenZip = Get-Command 7z.exe -ErrorAction SilentlyContinue
    if ($null -eq $sevenZip) {
      foreach ($candidate in @(
        (Join-Path $env:ProgramFiles "7-Zip\7z.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "7-Zip\7z.exe"),
        "C:\ProgramData\chocolatey\bin\7z.exe"
      )) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
          $sevenZip = $candidate
          break
        }
      }
    }
    if ($null -eq $sevenZip) {
      throw "7z.exe is required to inspect the NSIS installer."
    }
    $listing = (& $sevenZip l $path 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0) {
      throw "7z.exe could not inspect ${path}:`n$listing"
    }
  }
}
else {
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $path = (Resolve-Path -LiteralPath $PortableZipPath).Path
  $archive = [System.IO.Compression.ZipFile]::OpenRead($path)
  try {
    $listing = ($archive.Entries | ForEach-Object FullName) -join "`n"
  }
  finally {
    $archive.Dispose()
  }
}

foreach ($pattern in $required) {
  if ($listing -notmatch $pattern) {
    throw "ConPTY payload missing from ${path}: $pattern"
  }
}
Write-Host "ConPTY payload verified: $path"
