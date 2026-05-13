$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$packageDir = Join-Path $repoRoot "dist\packages"
$appDir = Join-Path $packageDir "win-unpacked"
$zipPath = Join-Path $packageDir "PatchOpsIII.zip"

if (-not (Test-Path -LiteralPath $appDir)) {
  throw "Windows app directory was not produced: $appDir"
}

if (Test-Path -LiteralPath $zipPath) {
  Remove-Item -LiteralPath $zipPath -Force
}

Compress-Archive -Path (Join-Path $appDir "*") -DestinationPath $zipPath -CompressionLevel Optimal -Force

if (-not (Test-Path -LiteralPath $zipPath)) {
  throw "Windows zip was not produced: $zipPath"
}
