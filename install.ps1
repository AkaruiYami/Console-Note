#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $HOME 'bin'),
    [string]$Version = 'latest',
    [switch]$AddToPath
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'AkaruiYami/Console-Note'
$Headers = @{ 'User-Agent' = 'console-note-installer' }

switch -Regex ($env:PROCESSOR_ARCHITECTURE) {
    '^(AMD64|ARM64)$' { $target = 'x86_64-pc-windows-msvc' }
    default { throw "Unsupported architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}

if ($Version -eq 'latest') {
    $release = Invoke-RestMethod @Headers -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $tag = $release.tag_name
} else {
    $tag = $Version
}

$asset = "console_note-$target.zip"
$baseUrl = "https://github.com/$Repo/releases/download/$tag"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("console_note_" + [Guid]::NewGuid().ToString('N'))

Write-Host "Installing console_note $tag -> $InstallDir"

try {
    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    $zipPath = Join-Path $tempRoot $asset

    Invoke-WebRequest @Headers -Uri "$baseUrl/$asset" -OutFile $zipPath
    Invoke-WebRequest @Headers -Uri "$baseUrl/$asset.sha256" -OutFile "$zipPath.sha256"

    $expected = ((Get-Content "$zipPath.sha256" -Raw).Split() | Where-Object { $_ }).ToLower()
    $actual = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "Checksum mismatch: expected $expected, got $actual"
    }

    $extractDir = Join-Path $tempRoot 'extracted'
    Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

    $exe = Join-Path $extractDir 'console_note.exe'
    if (-not (Test-Path $exe)) {
        throw "Archive did not contain console_note.exe"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $exe -Destination (Join-Path $InstallDir 'console_note.exe') -Force
} finally {
    if (Test-Path $tempRoot) {
        Remove-Item -Recurse -Force $tempRoot
    }
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$onPath = @(($userPath -split ';') + ($env:Path -split ';')) -contains $InstallDir

if (-not $onPath) {
    if ($AddToPath) {
        $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path += ";$InstallDir"
        Write-Host "Added $InstallDir to your user PATH."
    } else {
        Write-Warning "$InstallDir is not on your PATH."
        Write-Host "Add it with:"
        Write-Host "  `$dir = '$InstallDir'"
        Write-Host "  `$user = [Environment]::GetEnvironmentVariable('Path', 'User')"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"`$user;`$dir`", 'User')"
        Write-Host "Or re-run this installer with -AddToPath."
    }
}

& (Join-Path $InstallDir 'console_note.exe') --help
