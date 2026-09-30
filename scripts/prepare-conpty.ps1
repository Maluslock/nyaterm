[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Net.Http
Add-Type -AssemblyName System.IO.Compression.FileSystem

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$tauriRoot = Join-Path $repositoryRoot "src-tauri"
$package = Get-Content (Join-Path $tauriRoot "conpty-package.json") -Raw | ConvertFrom-Json
$packageId = "microsoft.windows.console.conpty"
$url = "https://api.nuget.org/v3-flatcontainer/$packageId/$($package.version)/$packageId.$($package.version).nupkg"
$cacheDir = Join-Path $tauriRoot "target/conpty-package"
$packagePath = Join-Path $cacheDir "$packageId.$($package.version).nupkg"
$outputDir = Join-Path $tauriRoot "resources/windows/conpty"

New-Item -ItemType Directory -Path $cacheDir -Force | Out-Null
if (!(Test-Path -LiteralPath $packagePath -PathType Leaf)) {
  $client = [System.Net.Http.HttpClient]::new()
  try {
    $bytes = $client.GetByteArrayAsync($url).GetAwaiter().GetResult()
    [System.IO.File]::WriteAllBytes($packagePath, $bytes)
  }
  finally {
    $client.Dispose()
  }
}

$hashAlgorithm = [System.Security.Cryptography.SHA256]::Create()
try {
  $packageStream = [System.IO.File]::OpenRead($packagePath)
  try {
    $actualHash = [System.BitConverter]::ToString($hashAlgorithm.ComputeHash($packageStream)).Replace("-", "").ToLowerInvariant()
  }
  finally {
    $packageStream.Dispose()
  }
}
finally {
  $hashAlgorithm.Dispose()
}
if ($actualHash -ne $package.sha256) {
  throw "ConPTY package SHA-256 mismatch: expected $($package.sha256), got $actualHash"
}

$files = @(
  @{ Source = "runtimes/win-x64/native/conpty.dll"; Target = "x64/conpty.dll"; Machine = 0x8664 },
  @{ Source = "build/native/runtimes/x64/OpenConsole.exe"; Target = "x64/x64/OpenConsole.exe"; Machine = 0x8664 },
  @{ Source = "build/native/runtimes/arm64/OpenConsole.exe"; Target = "x64/arm64/OpenConsole.exe"; Machine = 0xAA64 },
  @{ Source = "runtimes/win-arm64/native/conpty.dll"; Target = "arm64/conpty.dll"; Machine = 0xAA64 },
  @{ Source = "build/native/runtimes/arm64/OpenConsole.exe"; Target = "arm64/arm64/OpenConsole.exe"; Machine = 0xAA64 }
)

$archive = [System.IO.Compression.ZipFile]::OpenRead($packagePath)
try {
  foreach ($file in $files) {
    $entry = $archive.GetEntry($file.Source)
    if ($null -eq $entry) {
      throw "ConPTY package is missing $($file.Source)"
    }

    $stream = [System.IO.MemoryStream]::new()
    try {
      $entryStream = $entry.Open()
      try {
        $entryStream.CopyTo($stream)
      }
      finally {
        $entryStream.Dispose()
      }
      $bytes = $stream.ToArray()
    }
    finally {
      $stream.Dispose()
    }

    if ($bytes.Length -lt 64 -or [System.BitConverter]::ToUInt16($bytes, 0) -ne 0x5A4D) {
      throw "ConPTY package contains an invalid PE file: $($file.Source)"
    }
    $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3C)
    if ($peOffset -lt 0 -or $peOffset + 6 -gt $bytes.Length -or
        [System.BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550 -or
        [System.BitConverter]::ToUInt16($bytes, $peOffset + 4) -ne $file.Machine) {
      throw "ConPTY package PE architecture mismatch: $($file.Source)"
    }

    $target = Join-Path $outputDir $file.Target
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    [System.IO.File]::WriteAllBytes($target, $bytes)
  }
}
finally {
  $archive.Dispose()
}

Write-Host "Prepared Microsoft.Windows.Console.ConPTY $($package.version)"
