param(
  [string]$BinaryPath = ".\qfence.exe",
  [string]$InstallDir = "$env:LOCALAPPDATA\QuotaFence\bin",
  [switch]$SkipPathUpdate
)

$ErrorActionPreference = "Stop"

$resolvedBinary = Resolve-Path -LiteralPath $BinaryPath -ErrorAction Stop
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$qfencePath = Join-Path $InstallDir "qfence.exe"
$legacyPath = Join-Path $InstallDir "quotafence.exe"
Copy-Item -LiteralPath $resolvedBinary -Destination $qfencePath -Force
Copy-Item -LiteralPath $resolvedBinary -Destination $legacyPath -Force

if (-not $SkipPathUpdate) {
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $segments = @($userPath -split ";" | Where-Object { $_ })
  $alreadyPresent = $segments | Where-Object {
    $_.TrimEnd("\") -ieq $InstallDir.TrimEnd("\")
  }

  if (-not $alreadyPresent) {
    $updatedPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $updatedPath, "User")
  }

  if (-not (($env:Path -split ";") -contains $InstallDir)) {
    $env:Path = "$env:Path;$InstallDir"
  }
}

Write-Host "Installed qfence.exe and quotafence.exe in $InstallDir"
if (-not $SkipPathUpdate) {
  Write-Host "Open a new terminal, then run: qfence help"
}

