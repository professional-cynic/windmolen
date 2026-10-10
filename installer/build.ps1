# Builds dist\Windmolen-Setup.exe from dist\windmolen.exe with Inno Setup,
# installing a pinned, checksum-verified Inno Setup first.
# Usage (after cargo build --release): pwsh installer/build.ps1 -Version 0.1.0
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = "Stop"

$inno = "6.5.4"
$sha256 = "FA73BF47A4DA250D185D07561C2BFDA387E5E20DB77E4570004CF6A133CC10B1"
$temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$dir = Join-Path $temp "inno-$inno"
$iscc = Join-Path $dir "ISCC.exe"
if (-not (Test-Path $iscc)) {
  $setup = Join-Path $temp "innosetup-$inno.exe"
  Invoke-WebRequest "https://github.com/jrsoftware/issrc/releases/download/is-$($inno.Replace('.', '_'))/innosetup-$inno.exe" -OutFile $setup
  if ((Get-FileHash $setup -Algorithm SHA256).Hash -ne $sha256) { throw "Inno Setup $inno does not match its pinned checksum" }
  $p = Start-Process $setup -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/CURRENTUSER", "/DIR=`"$dir`"" -Wait -PassThru
  if ($p.ExitCode -ne 0 -or -not (Test-Path $iscc)) { throw "Inno Setup did not install (exit code $($p.ExitCode))" }
}

$root = Split-Path $PSScriptRoot
$exe = Join-Path $root "dist\windmolen.exe"
# build.rs writes the icon next to its other outputs.
$ico = Get-ChildItem (Join-Path $root "target\release\build\windmolen-*\out\windmolen.ico") | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $ico) { throw "windmolen.ico not found: build the program first" }
& $iscc /Qp "/DVersion=$Version" "/DSource=$exe" "/DIcon=$($ico.FullName)" "/O$(Join-Path $root 'dist')" (Join-Path $PSScriptRoot "windmolen.iss")
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
