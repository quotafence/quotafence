param(
  [string]$InstallDir = "$env:LOCALAPPDATA\QuotaFence\bin",
  [switch]$KeepPath
)

$ErrorActionPreference = "Stop"

Remove-Item -LiteralPath (Join-Path $InstallDir "qfence.exe") -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath (Join-Path $InstallDir "quotafence.exe") -Force -ErrorAction SilentlyContinue

if (-not $KeepPath) {
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $segments = @($userPath -split ";" | Where-Object {
    $_ -and $_.TrimEnd("\") -ine $InstallDir.TrimEnd("\")
  })
  [Environment]::SetEnvironmentVariable("Path", ($segments -join ";"), "User")
}

if ((Test-Path -LiteralPath $InstallDir) -and
    -not (Get-ChildItem -LiteralPath $InstallDir -Force | Select-Object -First 1)) {
  Remove-Item -LiteralPath $InstallDir -Force
}

Write-Host "Removed qfence.exe and quotafence.exe from $InstallDir"
if (-not $KeepPath) {
  Write-Host "Open a new terminal to reload the user PATH."
}

