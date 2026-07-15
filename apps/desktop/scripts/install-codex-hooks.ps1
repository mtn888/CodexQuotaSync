[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath,

    [Parameter(Mandatory = $false)]
    [ValidateNotNullOrEmpty()]
    [string]$CodexHome
)

$ErrorActionPreference = 'Stop'
$marker = 'Codex Quota Sync: updating activity'

function ConvertTo-MutableValue {
    param([AllowNull()]$Value)

    if ($null -eq $Value) {
        return $null
    }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        $result = [ordered]@{}
        foreach ($property in $Value.PSObject.Properties) {
            $result[$property.Name] = ConvertTo-MutableValue $property.Value
        }
        return $result
    }
    if ($Value -is [System.Collections.IDictionary]) {
        $result = [ordered]@{}
        foreach ($key in $Value.Keys) {
            $result[[string]$key] = ConvertTo-MutableValue $Value[$key]
        }
        return $result
    }
    if (($Value -is [System.Collections.IEnumerable]) -and -not ($Value -is [string])) {
        $items = @()
        foreach ($item in $Value) {
            $items += ,(ConvertTo-MutableValue $item)
        }
        return ,$items
    }
    return $Value
}

function Test-IsArrayLike {
    param([AllowNull()]$Value)

    return ($null -ne $Value) -and
        ($Value -is [System.Collections.IEnumerable]) -and
        -not ($Value -is [string]) -and
        -not ($Value -is [System.Collections.IDictionary])
}

function Remove-ManagedHandlers {
    param(
        [System.Collections.IDictionary]$Hooks,
        [string]$EventName
    )

    if (-not $Hooks.Contains($EventName)) {
        return
    }
    if (-not (Test-IsArrayLike $Hooks[$EventName])) {
        throw "hooks.$EventName 不是数组。为避免破坏现有配置，安装已中止。"
    }

    $remainingGroups = @()
    foreach ($group in @($Hooks[$EventName])) {
        if (-not ($group -is [System.Collections.IDictionary])) {
            $remainingGroups += ,$group
            continue
        }
        if (-not $group.Contains('hooks')) {
            $remainingGroups += ,$group
            continue
        }
        if (-not (Test-IsArrayLike $group['hooks'])) {
            throw "hooks.$EventName 中的 hooks 不是数组。为避免破坏现有配置，安装已中止。"
        }

        $remainingHandlers = @()
        foreach ($handler in @($group['hooks'])) {
            $owned = ($handler -is [System.Collections.IDictionary]) -and
                $handler.Contains('statusMessage') -and
                ([string]$handler['statusMessage'] -eq $marker)
            if (-not $owned) {
                $remainingHandlers += ,$handler
            }
        }
        if ($remainingHandlers.Count -gt 0) {
            $group['hooks'] = @($remainingHandlers)
            $remainingGroups += ,$group
        }
    }
    $Hooks[$EventName] = @($remainingGroups)
}

function Add-ManagedGroup {
    param(
        [System.Collections.IDictionary]$Hooks,
        [string]$EventName,
        [AllowNull()][string]$Matcher,
        [string]$Command,
        [string]$CommandWindows
    )

    Remove-ManagedHandlers -Hooks $Hooks -EventName $EventName
    $handler = [ordered]@{
        type          = 'command'
        command       = $Command
        commandWindows = $CommandWindows
        timeout       = 5
        statusMessage = $marker
    }
    $group = [ordered]@{
        hooks = @($handler)
    }
    if (-not [string]::IsNullOrWhiteSpace($Matcher)) {
        $group = [ordered]@{
            matcher = $Matcher
            hooks   = @($handler)
        }
    }
    $existingGroups = @()
    if ($Hooks.Contains($EventName)) {
        $existingGroups = @($Hooks[$EventName])
    }
    $Hooks[$EventName] = @($existingGroups) + @($group)
}

function Write-JsonAtomically {
    param(
        [string]$Path,
        [string]$Json
    )

    $directory = Split-Path -Parent $Path
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
    $temporary = Join-Path $directory ('.hooks.{0}.{1}.tmp' -f $PID, [Guid]::NewGuid().ToString('N'))
    $backup = "$Path.codex-quota-sync.bak"
    try {
        $utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($temporary, $Json + [Environment]::NewLine, $utf8WithoutBom)
        if (Test-Path -LiteralPath $Path -PathType Leaf) {
            [System.IO.File]::Replace($temporary, $Path, $backup, $true)
        }
        else {
            [System.IO.File]::Move($temporary, $Path)
        }
    }
    finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
}

$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "找不到 Codex Quota Sync 可执行文件：$ExecutablePath"
}

if ([string]::IsNullOrWhiteSpace($CodexHome)) {
    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        $CodexHome = $env:CODEX_HOME
    }
    else {
        $CodexHome = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex'
    }
}
$resolvedCodexHome = [System.IO.Path]::GetFullPath($CodexHome)
$hooksPath = Join-Path $resolvedCodexHome 'hooks.json'

if (Test-Path -LiteralPath $hooksPath -PathType Leaf) {
    $raw = [System.IO.File]::ReadAllText($hooksPath)
    try {
        $document = ConvertTo-MutableValue ($raw | ConvertFrom-Json -ErrorAction Stop)
    }
    catch {
        throw "现有 hooks.json 不是有效 JSON，未作任何修改：$($_.Exception.Message)"
    }
    if (-not ($document -is [System.Collections.IDictionary])) {
        throw 'hooks.json 根节点不是 JSON 对象，未作任何修改。'
    }
}
else {
    $document = [ordered]@{}
}

if (-not $document.Contains('hooks')) {
    $document['hooks'] = [ordered]@{}
}
if (-not ($document['hooks'] -is [System.Collections.IDictionary])) {
    throw 'hooks.json 中的 hooks 不是 JSON 对象，未作任何修改。'
}

$command = '"' + $resolvedExecutable + '" --activity-hook'
$commandWindows = 'cmd.exe /d /s /c call "' + $resolvedExecutable + '" --activity-hook'
$hooks = $document['hooks']
Add-ManagedGroup -Hooks $hooks -EventName 'SessionStart' -Matcher 'startup|resume|clear' -Command $command -CommandWindows $commandWindows
Add-ManagedGroup -Hooks $hooks -EventName 'UserPromptSubmit' -Matcher $null -Command $command -CommandWindows $commandWindows
Add-ManagedGroup -Hooks $hooks -EventName 'PermissionRequest' -Matcher '*' -Command $command -CommandWindows $commandWindows
Add-ManagedGroup -Hooks $hooks -EventName 'PreToolUse' -Matcher '(^|__)request_user_input$' -Command $command -CommandWindows $commandWindows
Add-ManagedGroup -Hooks $hooks -EventName 'PostToolUse' -Matcher '*' -Command $command -CommandWindows $commandWindows
Add-ManagedGroup -Hooks $hooks -EventName 'Stop' -Matcher $null -Command $command -CommandWindows $commandWindows

$json = $document | ConvertTo-Json -Depth 100
if ($PSCmdlet.ShouldProcess($hooksPath, '安全合并 Codex Quota Sync 活动跟踪 hooks')) {
    Write-JsonAtomically -Path $hooksPath -Json $json
    Write-Host "已安装 Codex Quota Sync hooks：$hooksPath"
    Write-Host '下一步：重新打开 Codex，在 /hooks 中审查并信任标记为 Codex Quota Sync 的 6 组命令。'
    Write-Host "如原文件已存在，安装前版本保存在：$hooksPath.codex-quota-sync.bak"
}
