[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [string]$ServerUrl = '',

    [Parameter(Mandatory = $false)]
    [string]$SourceId = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Wait-OnError {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Host ''
    Write-Host "配置失败：$Message" -ForegroundColor Red
    Write-Host '按 Enter 键关闭窗口。'
    [void](Read-Host)
}

try {
    $exePath = Join-Path $PSScriptRoot 'codex-quota-sync.exe'
    $configureScript = Join-Path $PSScriptRoot 'configure-sync.ps1'
    $packageConfigPath = Join-Path $PSScriptRoot 'viewer-settings.json'

    foreach ($requiredPath in @($exePath, $configureScript)) {
        if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
            throw "压缩包不完整，缺少文件：$requiredPath"
        }
    }

    if ([string]::IsNullOrWhiteSpace($ServerUrl)) {
        if (-not (Test-Path -LiteralPath $packageConfigPath -PathType Leaf)) {
            throw '未提供 ServerUrl，且压缩包中缺少 viewer-settings.json。'
        }
        $packageConfig = Get-Content -Raw -LiteralPath $packageConfigPath | ConvertFrom-Json
        $ServerUrl = [string]$packageConfig.serverUrl
    }
    $ServerUrl = $ServerUrl.Trim().TrimEnd('/')

    if ([string]::IsNullOrWhiteSpace($SourceId)) {
        $computerName = [string]$env:COMPUTERNAME
        $safeComputerName = ($computerName.ToLowerInvariant() -replace '[^a-z0-9._-]', '-')
        $safeComputerName = $safeComputerName.Trim('-', '.', '_')
        if ([string]::IsNullOrWhiteSpace($safeComputerName)) {
            $safeComputerName = 'windows'
        }
        $SourceId = "viewer-$safeComputerName"
        if ($SourceId.Length -gt 64) {
            $SourceId = $SourceId.Substring(0, 64).TrimEnd('-', '.', '_')
        }
    }

    Write-Host '正在配置 Codex Quota Sync Viewer...' -ForegroundColor Cyan
    Write-Host "服务器：$ServerUrl"
    Write-Host "设备标识：$SourceId"

    $running = @(
        Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and $_.ExecutablePath -ieq $exePath }
    )
    if ($running.Count -gt 0) {
        Write-Host '正在关闭已有的 Codex Quota Sync 进程...'
        foreach ($process in $running) {
            Stop-Process -Id $process.ProcessId -Force
        }
        Start-Sleep -Milliseconds 500
    }

    & $configureScript -Role viewer -ServerUrl $ServerUrl -SourceId $SourceId

    try {
        $health = Invoke-RestMethod -Uri "$ServerUrl/healthz" -Method Get -TimeoutSec 8
        Write-Host '服务器连接正常。' -ForegroundColor Green
    }
    catch {
        Write-Warning "配置已经保存，但暂时无法访问服务器：$($_.Exception.Message)"
        Write-Warning '悬浮框仍会启动，并在后台自动重试。'
    }

    Start-Process -FilePath $exePath -WorkingDirectory $PSScriptRoot
    Write-Host ''
    Write-Host 'Viewer 配置完成，Codex Quota Sync 已启动。' -ForegroundColor Green
}
catch {
    Wait-OnError -Message $_.Exception.Message
    exit 1
}
