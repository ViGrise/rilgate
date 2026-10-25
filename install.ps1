# Installs a Rilgate release binary for Windows from GitHub Releases.
#
#   irm https://github.com/ViGrise/rilgate/releases/latest/download/install.ps1 | iex
#
# -Version selects a release tag (default: latest) and -InstallDir the target
# directory (default: %LOCALAPPDATA%\rilgate\bin). The VERSION and INSTALL_DIR
# environment variables supply the same values. The binary is verified against
# SHA256SUMS, and the install directory is added to the user PATH.
[CmdletBinding()]
param (
  [string]$Version = $(if ($env:VERSION) { $env:VERSION } else { "latest" }),
  [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "rilgate\bin" })
)

$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 renders download progress slowly and may default to TLS 1.0.
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Repo = "ViGrise/rilgate"
$BinName = "rilgate.exe"

# OSArchitecture reports the native architecture also when an x64 shell runs under emulation on ARM64.
$Arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
  "X64" { "amd64" }
  "Arm64" { "arm64" }
  default { throw "Unsupported architecture: $_" }
}

$Asset = "rilgate-windows-$Arch.exe"
$Base = if ($Version -eq "latest") {
  "https://github.com/$Repo/releases/latest/download"
} else {
  "https://github.com/$Repo/releases/download/$Version"
}

$WorkDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $WorkDir | Out-Null
try {
  Write-Host "Downloading $Asset ($Version)..."
  $Download = Join-Path $WorkDir $Asset
  $Sums = Join-Path $WorkDir "SHA256SUMS"
  Invoke-WebRequest -Uri "$Base/$Asset" -OutFile $Download -UseBasicParsing
  Invoke-WebRequest -Uri "$Base/SHA256SUMS" -OutFile $Sums -UseBasicParsing

  $Expected = Get-Content $Sums | ForEach-Object {
    $Digest, $Name = $_ -split "\s+", 2
    if ($Name -and $Name.Trim().TrimStart("*") -eq $Asset) { $Digest.ToLowerInvariant() }
  } | Select-Object -First 1
  if (-not $Expected) { throw "SHA256SUMS has no entry for $Asset" }
  $Actual = (Get-FileHash -Algorithm SHA256 -Path $Download).Hash.ToLowerInvariant()
  if ($Expected -ne $Actual) { throw "Checksum mismatch for $Asset" }

  New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
  $Dest = Join-Path $InstallDir $BinName
  Move-Item -Path $Download -Destination $Dest -Force
} finally {
  Remove-Item -Path $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
}

$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
$Entries = if ($UserPath) { $UserPath -split ";" } else { @() }
if ($Entries -notcontains $InstallDir) {
  [Environment]::SetEnvironmentVariable("PATH", (@($Entries | Where-Object { $_ }) + $InstallDir) -join ";", "User")
  $env:PATH = "$env:PATH;$InstallDir"
  Write-Host "Added $InstallDir to the user PATH. Open a new shell to use it."
}

Write-Host "Installed $Dest"
& $Dest --version
if ($LASTEXITCODE -ne 0) { throw "$BinName is installed but cannot start." }
Write-Host "Upgrade later with: rilgate upgrade"
