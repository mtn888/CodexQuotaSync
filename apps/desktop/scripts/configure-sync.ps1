[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('collector', 'viewer')]
    [string]$Role,

    [Parameter(Mandatory = $false)]
    [string]$ServerUrl = '',

    [Parameter(Mandatory = $false)]
    [string]$WriteSecret = '',

    [Parameter(Mandatory = $false)]
    [ValidatePattern('^[A-Za-z0-9._-]{1,64}$')]
    [string]$SourceId = 'windows-main',

    [Parameter(Mandatory = $false)]
    [string]$ActivityStatePath = '',

    [Parameter(Mandatory = $false)]
    [string]$ConfigPath
)

$ErrorActionPreference = 'Stop'

if (-not [string]::IsNullOrWhiteSpace($ServerUrl)) {
    $uri = $null
    if (-not [Uri]::TryCreate($ServerUrl, [UriKind]::Absolute, [ref]$uri) -or $uri.Scheme -ne 'http') {
        throw 'ServerUrl 必须是完整的 http:// 地址，例如 http://nas.example.com:18080。'
    }
    if (-not [string]::IsNullOrEmpty($uri.UserInfo)) {
        throw 'ServerUrl 不能包含用户名或密码。'
    }
    $ServerUrl = $ServerUrl.Trim().TrimEnd('/')
}

if ($Role -eq 'collector' -and -not [string]::IsNullOrWhiteSpace($ServerUrl) -and [string]::IsNullOrEmpty($WriteSecret)) {
    throw 'collector 连接服务器时必须提供与容器 CQS_WRITE_SECRET 相同的 WriteSecret。'
}

if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $ConfigPath = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'io.github.mtn888.codexquotasync\preferences.json'
}
$ConfigPath = [IO.Path]::GetFullPath($ConfigPath)
$directory = Split-Path -Parent $ConfigPath
[IO.Directory]::CreateDirectory($directory) | Out-Null

$defaults = [ordered]@{
    locked            = $false
    alwaysOnTop       = $true
    stayExpanded      = $false
    pinnedProvider    = $null
    autoRotateSeconds = 12
    language          = 'zh-CN'
    syncRole          = 'collector'
    serverUrl         = ''
    sourceId          = 'windows-main'
    writeSecret       = ''
    activityStatePath = ''
}

if (Test-Path -LiteralPath $ConfigPath -PathType Leaf) {
    try {
        $existing = Get-Content -Raw -LiteralPath $ConfigPath | ConvertFrom-Json -AsHashtable
    }
    catch {
        throw "现有配置不是有效 JSON，未作修改：$($_.Exception.Message)"
    }
    foreach ($key in $existing.Keys) {
        $defaults[$key] = $existing[$key]
    }
}

$defaults.syncRole = $Role
$defaults.serverUrl = $ServerUrl
$defaults.sourceId = $SourceId
$defaults.activityStatePath = $ActivityStatePath.Trim()
if ($Role -eq 'collector') {
    if (-not [string]::IsNullOrEmpty($WriteSecret)) {
        $defaults.writeSecret = $WriteSecret
    }
}
else {
    $defaults.writeSecret = ''
}

$temporary = "$ConfigPath.$PID.tmp"
$backup = "$ConfigPath.bak"
$utf8 = [Text.UTF8Encoding]::new($false)
try {
    $json = $defaults | ConvertTo-Json -Depth 20
    [IO.File]::WriteAllText($temporary, $json + [Environment]::NewLine, $utf8)
    if (Test-Path -LiteralPath $ConfigPath -PathType Leaf) {
        [IO.File]::Replace($temporary, $ConfigPath, $backup, $true)
    }
    else {
        [IO.File]::Move($temporary, $ConfigPath)
    }
}
finally {
    if (Test-Path -LiteralPath $temporary -PathType Leaf) {
        Remove-Item -LiteralPath $temporary -Force
    }
}

Write-Host "已写入 Codex Quota Sync $Role 配置：$ConfigPath"
if ($Role -eq 'collector' -and -not [string]::IsNullOrWhiteSpace($ServerUrl)) {
    Write-Host '写入密钥已保存，但不会回显。请重启桌面应用使配置生效。'
}
else {
    Write-Host '请重启桌面应用使配置生效。'
}
