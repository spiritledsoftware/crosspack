param(
  [string]$Version = "v0.0.3",
  [string]$Repo = "spiritledsoftware/crosspack",
  [string]$BinDir = (Join-Path $env:LOCALAPPDATA "Crosspack\\bin")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$asset = "crosspack-$Version-x86_64-pc-windows-msvc.zip"
$baseUrl = "https://github.com/$Repo/releases/download/$Version"
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("crosspack-install-" + [guid]::NewGuid().ToString("N"))

try {
  New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

  $zipPath = Join-Path $tmpDir $asset
  $checksumsPath = Join-Path $tmpDir "SHA256SUMS.txt"

  Write-Host "==> Downloading $asset"
  Invoke-WebRequest -Uri "$baseUrl/$asset" -OutFile $zipPath
  Invoke-WebRequest -Uri "$baseUrl/SHA256SUMS.txt" -OutFile $checksumsPath

  $expected = (
    Get-Content $checksumsPath |
      Where-Object { $_ -match [regex]::Escape($asset) + '$' } |
      Select-Object -First 1
  )

  if (-not $expected) {
    throw "Checksum for $asset not found in SHA256SUMS.txt"
  }

  $expectedHash = ($expected -split '\s+')[0].ToLowerInvariant()
  $actualHash = (Get-FileHash -Algorithm SHA256 -Path $zipPath).Hash.ToLowerInvariant()

  if ($expectedHash -ne $actualHash) {
    throw "Checksum mismatch for $asset (expected $expectedHash, got $actualHash)"
  }

  Write-Host "==> Installing to $BinDir"
  Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

  New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
  Copy-Item (Join-Path $tmpDir "crosspack.exe") (Join-Path $BinDir "crosspack.exe") -Force
  Copy-Item (Join-Path $tmpDir "crosspack.exe") (Join-Path $BinDir "cpk.exe") -Force

  Write-Host "Installed crosspack ($Version) to $BinDir"
  Write-Host "Add $BinDir to PATH if needed."
}
finally {
  if (Test-Path $tmpDir) {
    Remove-Item $tmpDir -Recurse -Force
  }
}
