[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [string]$ServerUrl = 'http://mtn_ekelly.juiceyoga.cn:18080',

    [Parameter(Mandatory = $false)]
    [string]$OutputDirectory = '',

    [Parameter(Mandatory = $false)]
    [string]$Version = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-HttpServerUrl {
    param([Parameter(Mandatory = $true)][string]$Value)

    $uri = $null
    if (-not [Uri]::TryCreate($Value, [UriKind]::Absolute, [ref]$uri) -or $uri.Scheme -ne 'http') {
        throw 'ServerUrl must be a complete http:// URL, for example http://nas.example.com:18080.'
    }
    if (-not [string]::IsNullOrEmpty($uri.UserInfo)) {
        throw 'ServerUrl must not contain a user name or password.'
    }
}

Assert-HttpServerUrl -Value $ServerUrl
$ServerUrl = $ServerUrl.Trim().TrimEnd('/')

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $PSScriptRoot 'dist'
}

$exePath = Join-Path $PSScriptRoot 'apps\desktop\src-tauri\target\release\codex-quota-sync.exe'
$configureScript = Join-Path $PSScriptRoot 'apps\desktop\scripts\configure-sync.ps1'
$setupScript = Join-Path $PSScriptRoot 'setup-windows-viewer.ps1'
$tauriConfigPath = Join-Path $PSScriptRoot 'apps\desktop\src-tauri\tauri.conf.json'

foreach ($requiredPath in @($exePath, $configureScript, $setupScript, $tauriConfigPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required packaging file is missing: $requiredPath"
    }
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $tauriConfig = Get-Content -Raw -LiteralPath $tauriConfigPath | ConvertFrom-Json
    $Version = [string]$tauriConfig.version
}
if ([string]::IsNullOrWhiteSpace($Version) -or $Version -notmatch '^[0-9A-Za-z._-]+$') {
    throw "Unable to determine a valid version: $Version"
}

$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null

$packageName = "CodexQuotaSync-viewer-$Version-win-x64"
$archivePath = Join-Path $OutputDirectory "$packageName.zip"
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$stagingPath = [IO.Path]::GetFullPath((Join-Path $temporaryRoot "$packageName-$PID"))
if (-not $stagingPath.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Unsafe viewer staging path: $stagingPath"
}

if (Test-Path -LiteralPath $stagingPath) {
    Remove-Item -LiteralPath $stagingPath -Recurse -Force
}
[IO.Directory]::CreateDirectory($stagingPath) | Out-Null

try {
    Copy-Item -LiteralPath $exePath -Destination (Join-Path $stagingPath 'codex-quota-sync.exe')

    # Windows PowerShell 5.1 treats BOM-less UTF-8 scripts as an ANSI code page.
    # Write package scripts with a UTF-8 BOM so their Chinese messages parse correctly.
    $utf8Bom = [Text.UTF8Encoding]::new($true)
    [IO.File]::WriteAllText(
        (Join-Path $stagingPath 'configure-sync.ps1'),
        [IO.File]::ReadAllText($configureScript),
        $utf8Bom
    )
    [IO.File]::WriteAllText(
        (Join-Path $stagingPath 'setup-windows-viewer.ps1'),
        [IO.File]::ReadAllText($setupScript),
        $utf8Bom
    )

    $packageConfig = [ordered]@{
        serverUrl = $ServerUrl
    }
    $utf8 = [Text.UTF8Encoding]::new($false)
    $configJson = $packageConfig | ConvertTo-Json
    [IO.File]::WriteAllText(
        (Join-Path $stagingPath 'viewer-settings.json'),
        $configJson + [Environment]::NewLine,
        $utf8
    )

    $instructions = @"
Codex Quota Sync Windows Viewer

1. Extract the entire archive to a permanent directory.
2. Right-click setup-windows-viewer.ps1 and select Run with PowerShell.
3. The script configures viewer mode, checks the server, and starts the widget.

Viewer does not require Codex, a Codex login, or Codex Hooks.
"@
    [IO.File]::WriteAllText(
        (Join-Path $stagingPath 'README.txt'),
        $instructions.Trim() + [Environment]::NewLine,
        $utf8
    )

    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    Compress-Archive -Path (Join-Path $stagingPath '*') -DestinationPath $archivePath -CompressionLevel Optimal
}
finally {
    if (Test-Path -LiteralPath $stagingPath) {
        Remove-Item -LiteralPath $stagingPath -Recurse -Force
    }
}

$archive = Get-Item -LiteralPath $archivePath
Write-Host ''
Write-Host 'Windows Viewer archive created:' -ForegroundColor Green
Write-Host $archive.FullName
Write-Host ("Size: {0:N2} MB" -f ($archive.Length / 1MB))
Write-Host ("Server: {0}" -f $ServerUrl)
