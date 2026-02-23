param(
  [string]$Version = "",
  [string]$Repo = "spiritledsoftware/crosspack",
  [string]$BinDir = (Join-Path $env:LOCALAPPDATA "Crosspack\\bin"),
  [string]$CoreName = "core",
  [string]$CoreUrl = "https://github.com/spiritledsoftware/crosspack-registry.git",
  [string]$CoreKind = "git",
  [int]$CorePriority = 100,
  [string]$CoreFingerprint = "65149d198a39db9ecfea6f63d098858ed3b06c118c1f455f84ab571106b830c2"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("crosspack-install-" + [guid]::NewGuid().ToString("N"))

try {
  if ([string]::IsNullOrWhiteSpace($Version)) {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $release.tag_name
    if ([string]::IsNullOrWhiteSpace($Version)) {
      throw "Failed to resolve latest release tag from GitHub API"
    }
  }

  $asset = "crosspack-$Version-x86_64-pc-windows-msvc.zip"
  $baseUrl = "https://github.com/$Repo/releases/download/$Version"
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

  $crosspackExe = Join-Path $BinDir "crosspack.exe"
  Write-Host "==> Configuring default registry source ($CoreName)"
  $addOutput = & $crosspackExe registry add $CoreName $CoreUrl --kind $CoreKind --priority $CorePriority --fingerprint $CoreFingerprint 2>&1
  if ($LASTEXITCODE -ne 0) {
    $listOutput = & $crosspackExe registry list 2>&1
    if ($LASTEXITCODE -ne 0 -or ($listOutput -notmatch [regex]::Escape($CoreName))) {
      throw "Failed to configure registry source '$CoreName'.`n$addOutput"
    }
    Write-Host "Registry source '$CoreName' already present"
  } else {
    Write-Host "Added registry source '$CoreName'"
  }

  & $crosspackExe update | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "Failed to update registry snapshots after configuring '$CoreName'"
  }

  Write-Host "Installed crosspack ($Version) to $BinDir"
  Write-Host "Configured registry source '$CoreName' and refreshed snapshots."
  Write-Host "Add $BinDir to PATH if needed."
}
finally {
  if (Test-Path $tmpDir) {
    Remove-Item $tmpDir -Recurse -Force
  }
}
