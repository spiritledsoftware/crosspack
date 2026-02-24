param(
  [string]$Version = "",
  [string]$Repo = "spiritledsoftware/crosspack",
  [string]$BinDir = (Join-Path $env:LOCALAPPDATA "Crosspack\\bin"),
  [string]$CoreName = "core",
  [string]$CoreUrl = "https://github.com/spiritledsoftware/crosspack-registry.git",
  [string]$CoreKind = "git",
  [int]$CorePriority = 100,
  [string]$CoreFingerprint = "65149d198a39db9ecfea6f63d098858ed3b06c118c1f455f84ab571106b830c2",
  [switch]$NoShellSetup
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("crosspack-install-" + [guid]::NewGuid().ToString("N"))

function Update-CrosspackManagedProfileBlock {
  param(
    [string]$ProfilePath,
    [string]$ManagedBlock,
    [string]$BeginMarker,
    [string]$EndMarker
  )

  $current = ""
  if (Test-Path $ProfilePath) {
    $current = Get-Content -Path $ProfilePath -Raw
    if ($null -eq $current) {
      $current = ""
    }
  }

  $escapedBegin = [regex]::Escape($BeginMarker)
  $escapedEnd = [regex]::Escape($EndMarker)
  $pattern = "(?ms)^$escapedBegin`r?`n.*?^$escapedEnd`r?`n?"
  $stripped = [regex]::Replace($current, $pattern, "")
  $stripped = $stripped.TrimEnd("`r", "`n")

  if ([string]::IsNullOrWhiteSpace($stripped)) {
    $next = "$ManagedBlock`r`n"
  } else {
    $next = "$stripped`r`n`r`n$ManagedBlock`r`n"
  }

  Set-Content -Path $ProfilePath -Value $next -Encoding utf8
}

function Configure-CrosspackPowerShellSetup {
  param(
    [string]$CrosspackExe,
    [string]$BinDir,
    [switch]$Disabled
  )

  if ($Disabled) {
    Write-Host "Skipping shell setup because -NoShellSetup was provided."
    return
  }

  $prefixDir = Split-Path -Parent $BinDir
  if ([string]::IsNullOrWhiteSpace($prefixDir)) {
    Write-Warning "Automatic shell setup skipped: unable to infer prefix from bin dir '$BinDir'."
    return
  }

  $completionDir = Join-Path $prefixDir "share\\completions"
  $completionPath = Join-Path $completionDir "crosspack.ps1"
  $profilePath = $PROFILE.CurrentUserCurrentHost
  if ([string]::IsNullOrWhiteSpace($profilePath)) {
    Write-Warning "Automatic shell setup skipped: current PowerShell profile path is unavailable."
    Write-Host "Manual shell setup:"
    Write-Host "  & '$CrosspackExe' completions powershell > '$completionPath'"
    Write-Host "  add to your profile: . '$completionPath'"
    return
  }

  try {
    New-Item -ItemType Directory -Force -Path $completionDir | Out-Null
    & $CrosspackExe completions powershell | Out-File -FilePath $completionPath -Encoding utf8
    if ($LASTEXITCODE -ne 0) {
      throw "failed generating powershell completion script"
    }

    $profileDir = Split-Path -Parent $profilePath
    if (-not [string]::IsNullOrWhiteSpace($profileDir)) {
      New-Item -ItemType Directory -Force -Path $profileDir | Out-Null
    }
    if (-not (Test-Path $profilePath)) {
      New-Item -ItemType File -Path $profilePath -Force | Out-Null
    }

    $begin = "# >>> crosspack shell setup >>>"
    $end = "# <<< crosspack shell setup <<<"
    $managedBlock = @"
$begin
if (Test-Path '$BinDir') {
  if (-not (`$env:PATH -split ';' | Where-Object { `$_ -eq '$BinDir' })) {
    `$env:PATH = '$BinDir;' + `$env:PATH
  }
}
if (Test-Path '$completionPath') {
  . '$completionPath'
}
$end
"@

    Update-CrosspackManagedProfileBlock -ProfilePath $profilePath -ManagedBlock $managedBlock -BeginMarker $begin -EndMarker $end

    Write-Host "Configured PowerShell profile: $profilePath"
    Write-Host "Installed PowerShell completions: $completionPath"
  } catch {
    Write-Warning "Automatic shell setup failed: $($_.Exception.Message)"
    Write-Host "Manual shell setup:"
    Write-Host "  & '$CrosspackExe' completions powershell > '$completionPath'"
    Write-Host "  add to profile '$profilePath': . '$completionPath'"
    Write-Host "  add to current session PATH if needed: `$env:PATH = '$BinDir;' + `$env:PATH"
  }
}

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

  Configure-CrosspackPowerShellSetup -CrosspackExe $crosspackExe -BinDir $BinDir -Disabled:$NoShellSetup

  Write-Host "Installed crosspack ($Version) to $BinDir"
  Write-Host "Configured registry source '$CoreName' and refreshed snapshots."
  Write-Host "Add $BinDir to PATH if needed."
}
finally {
  if (Test-Path $tmpDir) {
    Remove-Item $tmpDir -Recurse -Force
  }
}
