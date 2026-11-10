<#
.SYNOPSIS
  Builds PatchOpsIII.msi and the portable PatchOpsIII.exe from target\release\patchops.exe.

.DESCRIPTION
  Needs: a release build (`cargo build --release --locked -p patchops`) and the WiX v5 CLI
  (`dotnet tool install --global wix --version 5.0.2`; the UI extension is added here, pinned).
  Output goes to dist\packages (the directory the legacy electron-builder build used):
    PatchOpsIII.msi   per-machine installer (upgrades the legacy Electron-era MSI)
    PatchOpsIII.exe   the bare application binary (portable; replaces the old NSIS "transition" exe)
    hash.log          SHA-256 of the MSI (what the release workflow publishes)
#>
[CmdletBinding()]
param(
  [string]$ExePath = 'target\release\patchops.exe',
  [string]$OutDir = 'dist\packages',
  [string]$WixUiVersion = '5.0.2'
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $repoRoot

if (-not (Test-Path -LiteralPath $ExePath)) { throw "Release binary not found: $ExePath (run cargo build --release -p patchops first)" }

# Single source of truth: [workspace.package] version in Cargo.toml.
$cargo = Get-Content -Raw -LiteralPath 'Cargo.toml'
if ($cargo -notmatch '(?ms)\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') { throw 'Could not read the workspace version from Cargo.toml' }
$semver = $Matches[1]
# MSI ProductVersion is numeric a.b.c.d (same mapping as electron-builder: "1.3.0-beta3" -> 1.3.0.0).
$parts = ($semver -split '[-+]')[0] -split '\.'
$nums = @(0, 0, 0)
for ($i = 0; $i -lt 3 -and $i -lt $parts.Count; $i++) { $nums[$i] = [int]$parts[$i] }
$msiVersion = '{0}.{1}.{2}.0' -f $nums[0], $nums[1], $nums[2]
Write-Host "App version $semver -> MSI ProductVersion $msiVersion"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$outDirFull = (Resolve-Path $OutDir).Path
$msi = Join-Path $outDirFull 'PatchOpsIII.msi'
$exeOut = Join-Path $outDirFull 'PatchOpsIII.exe'
Remove-Item -LiteralPath $msi, $exeOut -Force -ErrorAction SilentlyContinue

& wix extension add --global "WixToolset.UI.wixext/$WixUiVersion"
if ($LASTEXITCODE -ne 0) { throw 'wix extension add failed' }

& wix build (Join-Path $PSScriptRoot 'PatchOpsIII.wxs') `
  -arch x64 `
  -ext "WixToolset.UI.wixext/$WixUiVersion" `
  -d "Version=$msiVersion" `
  -d "ExePath=$((Resolve-Path $ExePath).Path)" `
  -d "IconPath=$(Join-Path $repoRoot 'packaging\assets\PatchOpsIII.ico')" `
  -d "BrandingDir=$(Join-Path $repoRoot 'packaging\windows')" `
  -sval `
  -o $msi
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }

Copy-Item -LiteralPath $ExePath -Destination $exeOut -Force

$hash = (Get-FileHash -Algorithm SHA256 -Path $msi).Hash.ToLowerInvariant()
Set-Content -Path (Join-Path $outDirFull 'hash.log') -Value $hash
Get-ChildItem $outDirFull | Format-Table Name, Length
