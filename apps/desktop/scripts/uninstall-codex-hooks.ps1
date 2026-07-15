[CmdletBinding(SupportsShouldProcess = $true)]
param(
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

function Write-JsonAtomically {
    param(
        [string]$Path,
        [string]$Json
    )

    $directory = Split-Path -Parent $Path
    $temporary = Join-Path $directory ('.hooks.{0}.{1}.tmp' -f $PID, [Guid]::NewGuid().ToString('N'))
    $backup = "$Path.codex-quota-sync-uninstall.bak"
    try {
        $utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($temporary, $Json + [Environment]::NewLine, $utf8WithoutBom)
        [System.IO.File]::Replace($temporary, $Path, $backup, $true)
    }
    finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
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
if (-not (Test-Path -LiteralPath $hooksPath -PathType Leaf)) {
    Write-Host "未找到 hooks.json，无需卸载：$hooksPath"
    return
}

$raw = [System.IO.File]::ReadAllText($hooksPath)
try {
    $document = ConvertTo-MutableValue ($raw | ConvertFrom-Json -ErrorAction Stop)
}
catch {
    throw "现有 hooks.json 不是有效 JSON，未作任何修改：$($_.Exception.Message)"
}
if (-not ($document -is [System.Collections.IDictionary]) -or
    -not $document.Contains('hooks') -or
    -not ($document['hooks'] -is [System.Collections.IDictionary])) {
    Write-Host '未发现可卸载的 Codex Quota Sync hooks。'
    return
}

$hooks = $document['hooks']
$changed = $false
foreach ($eventName in @('SessionStart', 'UserPromptSubmit', 'PermissionRequest', 'PreToolUse', 'PostToolUse', 'Stop')) {
    if (-not $hooks.Contains($eventName)) {
        continue
    }
    if (-not (Test-IsArrayLike $hooks[$eventName])) {
        throw "hooks.$eventName 不是数组。为避免破坏现有配置，卸载已中止。"
    }
    $remainingGroups = @()
    foreach ($group in @($hooks[$eventName])) {
        if (-not ($group -is [System.Collections.IDictionary]) -or -not $group.Contains('hooks')) {
            $remainingGroups += ,$group
            continue
        }
        if (-not (Test-IsArrayLike $group['hooks'])) {
            throw "hooks.$eventName 中的 hooks 不是数组。为避免破坏现有配置，卸载已中止。"
        }
        $remainingHandlers = @()
        foreach ($handler in @($group['hooks'])) {
            $owned = ($handler -is [System.Collections.IDictionary]) -and
                $handler.Contains('statusMessage') -and
                ([string]$handler['statusMessage'] -eq $marker)
            if ($owned) {
                $changed = $true
            }
            else {
                $remainingHandlers += ,$handler
            }
        }
        if ($remainingHandlers.Count -gt 0) {
            $group['hooks'] = @($remainingHandlers)
            $remainingGroups += ,$group
        }
    }
    if ($remainingGroups.Count -gt 0) {
        $hooks[$eventName] = @($remainingGroups)
    }
    else {
        $hooks.Remove($eventName)
    }
}

if (-not $changed) {
    Write-Host '未发现可卸载的 Codex Quota Sync hooks。'
    return
}

$json = $document | ConvertTo-Json -Depth 100
if ($PSCmdlet.ShouldProcess($hooksPath, '仅移除 Codex Quota Sync 活动跟踪 hooks')) {
    Write-JsonAtomically -Path $hooksPath -Json $json
    Write-Host "已卸载 Codex Quota Sync hooks：$hooksPath"
    Write-Host "卸载前版本保存在：$hooksPath.codex-quota-sync-uninstall.bak"
}
