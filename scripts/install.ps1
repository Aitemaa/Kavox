# Builds Kavox from source and installs it for the current user (no admin rights needed).
#
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1               build and install
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -InstallDeps  first install Rust, Node.js and C++ build tools with winget
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Uninstall    remove Kavox (your sounds and settings are kept)
param(
    [switch]$InstallDeps,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$AppId = 'io.github.aitemaa.kavox'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Kavox'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Kavox.lnk'
$Root = Split-Path -Parent $PSScriptRoot
$MinRust = [version]'1.88'
$MinNode = [version]'20.19'

function Say($Message) { Write-Host "==> $Message" -ForegroundColor Magenta }
function Warn($Message) { Write-Host "warning: $Message" -ForegroundColor Yellow }
function Die($Message) { Write-Host "error: $Message" -ForegroundColor Red; exit 1 }
function Has($Command) { [bool](Get-Command $Command -ErrorAction SilentlyContinue) }

# Runs a native command and stops if it fails (PowerShell doesn't by default).
function Run {
    param([string]$File, [string[]]$Arguments)
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { Die "$File $($Arguments -join ' ') failed (exit code $LASTEXITCODE)" }
}

function Get-UserPath { [Environment]::GetEnvironmentVariable('Path', 'User') }

function Add-UserPath($Dir) {
    $parts = @((Get-UserPath) -split ';' | Where-Object { $_ })
    if ($parts -notcontains $Dir) {
        [Environment]::SetEnvironmentVariable('Path', (($parts + $Dir) -join ';'), 'User')
    }
}

function Remove-UserPath($Dir) {
    $parts = @((Get-UserPath) -split ';' | Where-Object { $_ -and $_ -ne $Dir })
    [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
}

function Stop-Kavox {
    $running = Get-Process -Name kavox -ErrorAction SilentlyContinue
    if ($running) {
        Say 'Quitting the running Kavox'
        $running | Stop-Process -Force
        Start-Sleep -Seconds 1
    }
}

function Test-CppBuildTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    $found = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    return [bool]$found
}

if ($Uninstall) {
    Stop-Kavox
    Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $Shortcut -Force -ErrorAction SilentlyContinue
    Remove-UserPath $InstallDir
    Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Kavox' -ErrorAction SilentlyContinue
    Say 'Kavox removed.'
    Write-Host 'Your sounds and settings are still in:'
    Write-Host "  $(Join-Path $env:APPDATA $AppId)"
    Write-Host "  $(Join-Path $env:LOCALAPPDATA $AppId)"
    Write-Host 'Delete those folders to remove everything.'
    exit 0
}

if ($InstallDeps) {
    if (-not (Has winget)) { Die 'winget is missing. Install "App Installer" from the Microsoft Store, or install the tools listed in the README by hand.' }
    $installed = $false
    if (-not (Test-CppBuildTools)) {
        Say 'Installing Visual Studio C++ build tools (large download, may ask for permission)'
        Run winget @('install', '--id', 'Microsoft.VisualStudio.2022.BuildTools', '-e', '--accept-package-agreements', '--accept-source-agreements',
            '--override', '--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
        $installed = $true
    }
    if (-not (Has cargo)) {
        Say 'Installing Rust'
        Run winget @('install', '--id', 'Rustlang.Rustup', '-e', '--accept-package-agreements', '--accept-source-agreements')
        $installed = $true
    }
    if (-not (Has npm)) {
        Say 'Installing Node.js LTS'
        Run winget @('install', '--id', 'OpenJS.NodeJS.LTS', '-e', '--accept-package-agreements', '--accept-source-agreements')
        $installed = $true
    }
    if ($installed) {
        Say 'Tools installed. Open a new PowerShell window and run this script again (without -InstallDeps) so it sees them.'
        exit 0
    }
}

# Check the build tools.
if (-not (Test-CppBuildTools)) { Die 'Visual Studio C++ build tools are missing. Run this script with -InstallDeps.' }
if (-not (Has cargo)) { Die 'Rust is missing. Run this script with -InstallDeps, or get it from https://rustup.rs' }
$rust = [version](((& rustc --version) -split ' ')[1] -replace '-.*$', '')
if ($rust -lt $MinRust) { Die "Rust $rust is too old; Kavox needs $MinRust or newer. Run: rustup update stable" }
if (-not (Has npm)) { Die 'Node.js is missing. Run this script with -InstallDeps, or get it from https://nodejs.org' }
$node = [version]((& node --version).TrimStart('v'))
if ($node -lt $MinNode) { Die "Node.js $node is too old; Kavox needs $MinNode or newer." }

# VB-CABLE is how sounds reach voice apps; Kavox still runs without it, so only warn.
$cable = Get-CimInstance Win32_SoundDevice -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*VB-Audio*' }
if (-not $cable) { Warn 'VB-CABLE isn''t installed yet. Kavox will walk you through it, or get it now from https://vb-audio.com/Cable/' }

# Build.
Push-Location $Root
try {
    Say 'Installing frontend packages'
    if (Test-Path 'package-lock.json') { Run npm.cmd @('ci', '--no-fund', '--no-audit') } else { Run npm.cmd @('install', '--no-fund', '--no-audit') }
    Say 'Building Kavox (the first build takes a few minutes)'
    Run npx.cmd @('tauri', 'build', '--no-bundle')
    Say 'Building kavoxctl'
    Run cargo @('build', '--release', '-p', 'kavoxctl')
}
finally {
    Pop-Location
}

# Install.
Stop-Kavox
Say "Installing into $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item (Join-Path $Root 'target\release\kavox.exe') $InstallDir -Force
Copy-Item (Join-Path $Root 'target\release\kavoxctl.exe') $InstallDir -Force
$exe = Join-Path $InstallDir 'kavox.exe'

$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($Shortcut)
$link.TargetPath = $exe
$link.WorkingDirectory = $InstallDir
$link.IconLocation = "$exe,0"
$link.Description = 'Free soundboard with a virtual microphone'
$link.Save()

Add-UserPath $InstallDir

Say 'Kavox is installed.'
Write-Host '  Start it from the Start menu (search for Kavox).'
Write-Host '  kavoxctl works in new terminal windows, e.g.: kavoxctl play airhorn'
Start-Process $exe
