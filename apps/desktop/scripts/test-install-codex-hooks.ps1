[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath
)

$ErrorActionPreference = 'Stop'
$marker = 'Codex Quota Sync: updating activity'
$scriptPath = Join-Path $PSScriptRoot 'install-codex-hooks.ps1'
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath -ErrorAction Stop).Path
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('codex-quota-sync-hook-test-' + [Guid]::NewGuid().ToString('N'))
$codexHome = Join-Path $temporaryRoot '.codex'
$testExecutableDirectory = Join-Path $temporaryRoot 'Executable With Space'
$testExecutable = Join-Path $testExecutableDirectory ([System.IO.Path]::GetFileName($resolvedExecutable))

function Assert-Condition {
    param(
        [bool]$Condition,
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Invoke-HookCommand {
    param(
        [ValidateSet('cmd', 'powershell')]
        [string]$Runner,
        [string]$Command
    )

    $activityPath = Join-Path $temporaryRoot ("activity-$Runner.json")
    $previousActivityPath = $env:CODEX_QUOTA_SYNC_ACTIVITY_PATH
    try {
        $env:CODEX_QUOTA_SYNC_ACTIVITY_PATH = $activityPath
        $inputJson = '{"session_id":"installer-test-session","turn_id":"installer-test-turn","hook_event_name":"UserPromptSubmit"}'
        if ($Runner -eq 'cmd') {
            $output = $inputJson | & $env:COMSPEC /C $Command 2>&1
        }
        else {
            $powerShell = (Get-Command pwsh.exe -ErrorAction SilentlyContinue).Source
            if ([string]::IsNullOrWhiteSpace($powerShell)) {
                $powerShell = (Get-Command powershell.exe -ErrorAction Stop).Source
            }
            $output = $inputJson | & $powerShell -NoProfile -NonInteractive -Command $Command 2>&1
        }
        $exitCode = $LASTEXITCODE
        Assert-Condition ($exitCode -eq 0) "$Runner 执行 commandWindows 失败，退出码 $exitCode，输出：$($output -join [Environment]::NewLine)"
        Assert-Condition (Test-Path -LiteralPath $activityPath -PathType Leaf) "$Runner 执行后未生成活动状态文件。"
        $activity = Get-Content -LiteralPath $activityPath -Raw | ConvertFrom-Json -ErrorAction Stop
        Assert-Condition (@($activity.entries).Count -eq 1) "$Runner 生成的活动状态条目数不正确。"
        Assert-Condition ([string]$activity.entries[0].status -eq 'executing') "$Runner 生成的活动状态不是 executing。"
    }
    finally {
        $env:CODEX_QUOTA_SYNC_ACTIVITY_PATH = $previousActivityPath
    }
}

try {
    [System.IO.Directory]::CreateDirectory($testExecutableDirectory) | Out-Null
    Copy-Item -LiteralPath $resolvedExecutable -Destination $testExecutable
    & $scriptPath -ExecutablePath $testExecutable -CodexHome $codexHome
    $hooksPath = Join-Path $codexHome 'hooks.json'
    Assert-Condition (Test-Path -LiteralPath $hooksPath -PathType Leaf) '安装器未生成 hooks.json。'

    $document = Get-Content -LiteralPath $hooksPath -Raw | ConvertFrom-Json -ErrorAction Stop
    $handlers = @(
        foreach ($eventProperty in $document.hooks.PSObject.Properties) {
            foreach ($group in @($eventProperty.Value)) {
                foreach ($handler in @($group.hooks)) {
                    if ([string]$handler.statusMessage -eq $marker) {
                        $handler
                    }
                }
            }
        }
    )
    Assert-Condition ($handlers.Count -eq 6) "预期安装 6 个 Hook，实际为 $($handlers.Count) 个。"

    $expectedCommand = '"' + $testExecutable + '" --activity-hook'
    $expectedWindowsCommand = 'cmd.exe /d /s /c call "' + $testExecutable + '" --activity-hook'
    foreach ($handler in $handlers) {
        Assert-Condition ([string]$handler.command -eq $expectedCommand) '通用 command 内容不正确。'
        Assert-Condition ([string]$handler.commandWindows -eq $expectedWindowsCommand) 'commandWindows 未使用兼容包装命令。'
    }

    Invoke-HookCommand -Runner cmd -Command $handlers[0].commandWindows
    Invoke-HookCommand -Runner powershell -Command $handlers[0].commandWindows
    Write-Host 'Codex Hooks 安装器回归测试通过：CMD 与 PowerShell 均可执行 commandWindows。'
}
finally {
    if (Test-Path -LiteralPath $temporaryRoot) {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
}
