# Installs dist\Windmolen-Setup.exe silently, checks what it put in place,
# uninstalls and checks that nothing is left. Run on a Windows CI runner.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot
$app = Join-Path $env:LOCALAPPDATA "Programs\Windmolen"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{C5FEA1CD-40E6-44C8-AE36-05444E6CACD1}_is1"
$checks = [ordered]@{
  "program"            = { Test-Path (Join-Path $app "windmolen.exe") }
  "Start menu entry"   = { Test-Path (Join-Path ([Environment]::GetFolderPath("Programs")) "Windmolen.lnk") }
  "Settings > Apps"    = { Test-Path $uninstallKey }
  "PNG ProgID"         = { Test-Path "HKCU:\Software\Classes\Windmolen.png" }
  "Open with"          = { $null -ne (Get-ItemProperty "HKCU:\Software\Classes\.png\OpenWithProgids" -ErrorAction SilentlyContinue)."Windmolen.png" }
  "right-click verb"   = { (Get-ItemProperty "HKCU:\Software\Classes\SystemFileAssociations\.jpg\shell\Windmolen\command" -ErrorAction SilentlyContinue)."(default)" -like "*--only*" }
  "Default apps entry" = { $null -ne (Get-ItemProperty "HKCU:\Software\RegisteredApplications" -ErrorAction SilentlyContinue).Windmolen }
}

# Waits for that process only: Start-Process -Wait would also wait for any
# program it starts, such as Windmolen after an update.
function Run($file, $arguments) {
  $p = Start-Process $file -ArgumentList $arguments -PassThru
  $p.WaitForExit()
  $p
}

$p = Run (Join-Path $root "dist\Windmolen-Setup.exe") @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART")
if ($p.ExitCode -ne 0) { throw "Setup failed with exit code $($p.ExitCode)" }
foreach ($c in $checks.GetEnumerator()) {
  if (-not (& $c.Value)) { throw "After installing, missing: $($c.Key)" }
  "installed: $($c.Key)"
}

# An update as the app runs it: the running copy is closed, the program
# replaced and started again.
$setup = Join-Path $root "dist\Windmolen-Setup.exe"
$exe = Join-Path $app "windmolen.exe"
$old = Start-Process $exe -PassThru
Start-Sleep 5
if ($old.HasExited) { throw "Windmolen did not stay open before the update" }
$p = Run $setup @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/UPDATE=1")
if ($p.ExitCode -ne 0) { throw "The update failed with exit code $($p.ExitCode)" }
if (-not $old.WaitForExit(30000)) { throw "The update did not close the running copy" }
Start-Sleep 5
$new = Get-Process windmolen -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
if (-not $new) { throw "The update did not start Windmolen again" }
"updated: closed the running copy and started it again"
$new | Stop-Process -Force
Start-Sleep 2

# The uninstaller hands over to a copy of itself and returns at once: wait
# until its entry is gone.
$null = Run (Join-Path $app "unins000.exe") @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART")
for ($i = 0; $i -lt 60 -and (Test-Path $uninstallKey); $i++) { Start-Sleep 1 }
Start-Sleep 2
foreach ($c in $checks.GetEnumerator()) {
  if (& $c.Value) { throw "After uninstalling, still there: $($c.Key)" }
  "removed: $($c.Key)"
}
