# Build the NSIS installer from a commit exported by build-windows.sh.
#
# The Windows half of `pnpm --filter desktop build:windows`: build-windows.sh
# copies this file out of the commit it exports, next to source.tar. Every run
# extracts a fresh src\ and installs from the lockfile, while target\
# (CARGO_TARGET_DIR) is kept, so SQLCipher and OpenSSL compile only once.
# Written for Windows PowerShell 5.1, which reads a BOM-less file as ANSI, so
# keep it ASCII.
param(
  [Parameter(Mandatory)][string]$BuildDir,
  [Parameter(Mandatory)][string]$Commit,
  [Parameter(Mandatory)][string]$NodeVersion
)
$ErrorActionPreference = 'Stop'

# Native commands report failure through the exit code. 'Continue' while they
# run, because Windows PowerShell turns a native command's stderr into a
# terminating error under 'Stop' when its own output is redirected.
function Invoke-Native([string]$File, [string[]]$Arguments) {
  $ErrorActionPreference = 'Continue'
  & $File @Arguments
  if ($LASTEXITCODE -ne 0) { throw "$File $($Arguments -join ' ') exited with $LASTEXITCODE" }
}

# A process started from WSL inherits the PATH Windows had when WSL started,
# so re-read it to see tools installed since.
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
  [Environment]::GetEnvironmentVariable('Path', 'User')

$missing = @()
foreach ($tool in 'node', 'pnpm', 'cargo', 'perl', 'tar') {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { $missing += $tool }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$msvc = $null
if (Test-Path $vswhere) {
  $msvc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if (-not $msvc) { $missing += 'MSVC C++ build tools' }
if ($missing) {
  Write-Host "Missing on Windows: $($missing -join ', '). Install them in PowerShell, then rerun:"
  Write-Host '  winget install Rustlang.Rustup StrawberryPerl.StrawberryPerl OpenJS.NodeJS.LTS'
  Write-Host '  winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'
  Write-Host '  npm install -g corepack; corepack enable'
  exit 1
}
$node = (& node --version).TrimStart('v')
if ($node -ne $NodeVersion) { Write-Warning "Node $node differs from .nvmrc ($NodeVersion)." }

Set-Location $BuildDir
$src = Join-Path $BuildDir 'src'
# rmdir removes pnpm's node_modules junctions without following them.
if (Test-Path $src) { Invoke-Native cmd.exe '/d', '/c', 'rmdir', '/s', '/q', $src }
New-Item -ItemType Directory $src | Out-Null
Invoke-Native tar.exe '-xf', (Join-Path $BuildDir 'source.tar'), '-C', $src

$env:CARGO_TARGET_DIR = Join-Path $BuildDir 'target'
Set-Location $src
Invoke-Native pnpm 'install', '--frozen-lockfile'
Invoke-Native pnpm '--filter', 'desktop', 'build:app', '--bundles', 'nsis'

$bundle = Join-Path $env:CARGO_TARGET_DIR 'release\bundle\nsis'
$installer = Get-ChildItem $bundle -Filter '*-setup.exe' |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) { throw "No *-setup.exe under $bundle" }
$out = Join-Path $BuildDir 'out'
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory $out | Out-Null
Copy-Item $installer.FullName $out
Write-Host ''
Write-Host "Installer for $Commit`: $(Join-Path $out $installer.Name)"
Write-Host 'It is not code-signed, so SmartScreen asks on first run: More info > Run anyway.'
