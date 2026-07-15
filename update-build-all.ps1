[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [switch]$UpdateSource,

    [Parameter(Mandatory = $false)]
    [switch]$InstallHooks,

    [Parameter(Mandatory = $false)]
    [string]$ServerUrl = '',

    [Parameter(Mandatory = $false)]
    [string]$OutputDirectory = '',

    [Parameter(Mandatory = $false)]
    [switch]$SkipTests,

    [Parameter(Mandatory = $false)]
    [switch]$SkipDesktop,

    [Parameter(Mandatory = $false)]
    [switch]$SkipViewer,

    [Parameter(Mandatory = $false)]
    [switch]$SkipAndroid,

    [Parameter(Mandatory = $false)]
    [switch]$NoRestart
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = [IO.Path]::GetFullPath($PSScriptRoot)
$desktopRoot = Join-Path $repositoryRoot 'apps\desktop'
$tauriRoot = Join-Path $desktopRoot 'src-tauri'
$releaseExecutable = Join-Path $tauriRoot 'target\release\codex-quota-sync.exe'
$preferencesPath = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'io.github.mtn888.codexquotasync\preferences.json'
$viewerPackageScript = Join-Path $repositoryRoot 'package-windows-viewer.ps1'
$hooksInstallScript = Join-Path $desktopRoot 'scripts\install-codex-hooks.ps1'
$androidRoot = Join-Path $repositoryRoot 'android'
$gradleWrapper = Join-Path $androidRoot 'gradlew.bat'

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repositoryRoot 'dist'
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null

function Write-Step {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Host ''
    Write-Host ("==== {0} ====" -f $Message) -ForegroundColor Cyan
}

function Invoke-NativeStep {
    param(
        [Parameter(Mandatory = $true)][string]$Title,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $false)][string[]]$ArgumentList = @()
    )

    Write-Step $Title
    Push-Location $WorkingDirectory
    try {
        & $FilePath @ArgumentList
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "$Title 失败，退出码：$exitCode"
        }
    }
    finally {
        Pop-Location
    }
}

function Get-RepositoryCollectorProcesses {
    return @(
        Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and $_.ExecutablePath -ieq $releaseExecutable }
    )
}

function Assert-CommandAvailable {
    param([Parameter(Mandatory = $true)][string]$Name)

    if ($null -eq (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "找不到命令：$Name。请先安装对应工具并加入 PATH。"
    }
}

function Add-Artifact {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][System.Collections.ArrayList]$List,
        [Parameter(Mandatory = $true)][string]$Path
    )

    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        [void]$List.Add((Get-Item -LiteralPath $Path))
    }
}

function Get-JsonPropertyValue {
    param(
        [AllowNull()]$Object,
        [Parameter(Mandatory = $true)][string]$Name,
        [AllowNull()]$DefaultValue = ''
    )

    if ($null -eq $Object) {
        return $DefaultValue
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $DefaultValue
    }
    return $property.Value
}

$preferences = $null
if (Test-Path -LiteralPath $preferencesPath -PathType Leaf) {
    try {
        $preferences = Get-Content -LiteralPath $preferencesPath -Raw | ConvertFrom-Json
    }
    catch {
        throw "现有 Collector 配置不是有效 JSON：$preferencesPath"
    }
}

if ([string]::IsNullOrWhiteSpace($ServerUrl) -and $null -ne $preferences) {
    $ServerUrl = [string](Get-JsonPropertyValue -Object $preferences -Name 'serverUrl')
}
$ServerUrl = $ServerUrl.Trim().TrimEnd('/')

Write-Step '当前配置'
Write-Host "仓库：$repositoryRoot"
Write-Host "输出：$OutputDirectory"
if ($null -ne $preferences) {
    Write-Host ("Collector：role={0}, sourceId={1}, server={2}, writeSecret={3}" -f
        [string](Get-JsonPropertyValue -Object $preferences -Name 'syncRole'),
        [string](Get-JsonPropertyValue -Object $preferences -Name 'sourceId'),
        [string](Get-JsonPropertyValue -Object $preferences -Name 'serverUrl'),
        (-not [string]::IsNullOrEmpty([string](Get-JsonPropertyValue -Object $preferences -Name 'writeSecret'))))
    Write-Host '现有 Collector 配置会原样保留，不会重写密钥。'
}
else {
    Write-Warning "未找到现有 Collector 配置：$preferencesPath。构建仍可继续，但需要首次配置后才能同步。"
}

if ($UpdateSource) {
    Assert-CommandAvailable 'git.exe'
    $dirty = & git.exe -C $repositoryRoot status --porcelain
    if ($LASTEXITCODE -ne 0) {
        throw '无法检查 Git 工作区状态。'
    }
    if (@($dirty).Count -gt 0) {
        throw '工作区存在未提交改动，已拒绝自动拉取。请先提交或移走改动；脚本不会自动 stash/reset。'
    }
    Invoke-NativeStep -Title '拉取最新代码' -WorkingDirectory $repositoryRoot -FilePath 'git.exe' -ArgumentList @('pull', '--ff-only')
}

if (-not $SkipDesktop) {
    Assert-CommandAvailable 'npm.cmd'
    Assert-CommandAvailable 'cargo.exe'
}
if (-not $SkipAndroid) {
    if (-not (Test-Path -LiteralPath $gradleWrapper -PathType Leaf)) {
        throw "找不到 Android Gradle Wrapper：$gradleWrapper"
    }
    if ([string]::IsNullOrWhiteSpace($env:JAVA_HOME) -or -not (Test-Path -LiteralPath (Join-Path $env:JAVA_HOME 'bin\java.exe'))) {
        throw 'Android 构建需要 JDK 17。请设置 JAVA_HOME 后重试。'
    }
    $androidSdk = $env:ANDROID_SDK_ROOT
    if ([string]::IsNullOrWhiteSpace($androidSdk)) {
        $androidSdk = $env:ANDROID_HOME
    }
    if ([string]::IsNullOrWhiteSpace($androidSdk)) {
        $androidSdk = Join-Path $env:LOCALAPPDATA 'Android\Sdk'
    }
    if ([string]::IsNullOrWhiteSpace($androidSdk) -or -not (Test-Path -LiteralPath (Join-Path $androidSdk 'platforms\android-35'))) {
        throw 'Android 构建需要 ANDROID_SDK_ROOT/ANDROID_HOME 指向已安装 Platform 35 的 SDK。'
    }
}
if (-not $SkipViewer) {
    if ([string]::IsNullOrWhiteSpace($ServerUrl)) {
        throw '生成 Viewer 包需要 ServerUrl；请先配置 Collector，或向脚本传入 -ServerUrl。'
    }
    if (-not (Test-Path -LiteralPath $viewerPackageScript -PathType Leaf)) {
        throw "找不到 Viewer 打包脚本：$viewerPackageScript"
    }
}

$collectorWasRunning = @(Get-RepositoryCollectorProcesses).Count -gt 0
$backupExecutable = ''
$buildSucceeded = $false
$desktopReleaseSucceeded = [bool]$SkipDesktop
$artifacts = New-Object System.Collections.ArrayList

try {
    if (-not $SkipDesktop) {
        Invoke-NativeStep -Title '安装/更新 Windows 前端依赖' -WorkingDirectory $desktopRoot -FilePath 'npm.cmd' -ArgumentList @('install')
        if (-not $SkipTests) {
            Invoke-NativeStep -Title '运行 Windows 前端测试' -WorkingDirectory $desktopRoot -FilePath 'npm.cmd' -ArgumentList @('test')
        }
        Invoke-NativeStep -Title '构建 Windows 前端' -WorkingDirectory $desktopRoot -FilePath 'npm.cmd' -ArgumentList @('run', 'build')
        if (-not $SkipTests) {
            Invoke-NativeStep -Title '检查 Rust 格式' -WorkingDirectory $tauriRoot -FilePath 'cargo.exe' -ArgumentList @('fmt', '--all', '--', '--check')
            Invoke-NativeStep -Title '运行 Rust 测试' -WorkingDirectory $tauriRoot -FilePath 'cargo.exe' -ArgumentList @('test', '--all-targets')
        }

        Write-Step '停止仓库内 Collector'
        $runningCollectors = @(Get-RepositoryCollectorProcesses)
        if ($runningCollectors.Count -gt 0) {
            if (Test-Path -LiteralPath $releaseExecutable -PathType Leaf) {
                $backupExecutable = Join-Path ([IO.Path]::GetTempPath()) ("codex-quota-sync-release-backup-{0}.exe" -f [Guid]::NewGuid().ToString('N'))
                Copy-Item -LiteralPath $releaseExecutable -Destination $backupExecutable
            }
            foreach ($process in $runningCollectors) {
                Write-Host "停止 PID $($process.ProcessId)"
                Stop-Process -Id $process.ProcessId -Force
            }
            Start-Sleep -Milliseconds 500
            if (@(Get-RepositoryCollectorProcesses).Count -gt 0) {
                throw 'Collector 未能完全退出，无法覆盖 Release EXE。'
            }
        }
        else {
            Write-Host 'Collector 当前未运行。'
        }

        Invoke-NativeStep -Title '生成 Windows Release、MSI 和 NSIS' -WorkingDirectory $desktopRoot -FilePath 'npm.cmd' -ArgumentList @('run', 'tauri', '--', 'build')
        if (-not (Test-Path -LiteralPath $releaseExecutable -PathType Leaf)) {
            throw "Windows Release 构建完成但未找到 EXE：$releaseExecutable"
        }
        $desktopReleaseSucceeded = $true
    }

    if (-not $SkipViewer) {
        Write-Step '生成 Windows Viewer 便携包'
        & $viewerPackageScript -ServerUrl $ServerUrl -OutputDirectory $OutputDirectory
    }

    if (-not $SkipAndroid) {
        $gradleTasks = @('--no-daemon', ':app:assembleDebug')
        if (-not $SkipTests) {
            $gradleTasks = @('--no-daemon', ':app:testDebugUnitTest', ':app:assembleDebug')
        }
        Invoke-NativeStep -Title '测试并生成 Android Debug APK' -WorkingDirectory $androidRoot -FilePath $gradleWrapper -ArgumentList $gradleTasks

        $metadataPath = Join-Path $androidRoot 'app\build\outputs\apk\debug\output-metadata.json'
        $androidVersion = 'debug'
        $androidApkName = 'app-debug.apk'
        if (Test-Path -LiteralPath $metadataPath -PathType Leaf) {
            $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
            if (@($metadata.elements).Count -gt 0) {
                $androidElement = $metadata.elements[0]
                if ($null -ne $androidElement.PSObject.Properties['versionName'] -and -not [string]::IsNullOrWhiteSpace([string]$androidElement.versionName)) {
                    $androidVersion = [string]$androidElement.versionName
                }
                if ($null -ne $androidElement.PSObject.Properties['outputFile'] -and -not [string]::IsNullOrWhiteSpace([string]$androidElement.outputFile)) {
                    $androidApkName = [string]$androidElement.outputFile
                }
            }
        }
        $androidApk = Join-Path $androidRoot ("app\build\outputs\apk\debug\{0}" -f $androidApkName)
        if (-not (Test-Path -LiteralPath $androidApk -PathType Leaf)) {
            throw "Android 构建完成但未找到 APK：$androidApk"
        }
        $packagedAndroidApk = Join-Path $OutputDirectory ("CodexQuotaSync-android-{0}-debug.apk" -f $androidVersion)
        Copy-Item -LiteralPath $androidApk -Destination $packagedAndroidApk -Force

        $buildToolsDirectory = Get-ChildItem -LiteralPath (Join-Path $androidSdk 'build-tools') -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^35\.[0-9]+\.[0-9]+$' } |
            Sort-Object { [version]$_.Name } -Descending |
            Select-Object -First 1
        $apksignerPath = ''
        if ($null -ne $buildToolsDirectory) {
            $apksignerPath = Join-Path $buildToolsDirectory.FullName 'apksigner.bat'
        }
        if ([string]::IsNullOrWhiteSpace($apksignerPath) -or -not (Test-Path -LiteralPath $apksignerPath -PathType Leaf)) {
            throw 'APK 已生成，但找不到 Android Build Tools 中的 apksigner.bat，无法验证调试签名。'
        }
        Invoke-NativeStep -Title '验证 Android APK 调试签名' -WorkingDirectory $androidRoot -FilePath $apksignerPath -ArgumentList @('verify', '--verbose', '--print-certs', $packagedAndroidApk)
    }

    if ($InstallHooks) {
        if (-not (Test-Path -LiteralPath $hooksInstallScript -PathType Leaf)) {
            throw "找不到 Hooks 安装脚本：$hooksInstallScript"
        }
        Write-Step '安装 Codex Hooks（仅首次/路径变化时需要）'
        & $hooksInstallScript -ExecutablePath $releaseExecutable
        Write-Warning '如果 Hook 定义是首次出现或已变化，请在 Codex /hooks 中审查并信任。'
    }
    else {
        Write-Step '保持现有 Hooks'
        Write-Host '未修改 hooks.json，也不需要重新信任。'
    }

    $buildSucceeded = $true
}
finally {
    if (-not $desktopReleaseSucceeded -and -not [string]::IsNullOrWhiteSpace($backupExecutable) -and (Test-Path -LiteralPath $backupExecutable -PathType Leaf)) {
        Copy-Item -LiteralPath $backupExecutable -Destination $releaseExecutable -Force
    }

    $shouldStartCollector = (-not $NoRestart) -and ($collectorWasRunning -or ((-not $SkipDesktop) -and (Test-Path -LiteralPath $releaseExecutable -PathType Leaf)))
    if ($shouldStartCollector -and @(Get-RepositoryCollectorProcesses).Count -eq 0 -and (Test-Path -LiteralPath $releaseExecutable -PathType Leaf)) {
        Write-Step '启动 Collector'
        Start-Process -FilePath $releaseExecutable -WorkingDirectory (Split-Path -Parent $releaseExecutable) -WindowStyle Hidden | Out-Null
        Start-Sleep -Seconds 2
        if (@(Get-RepositoryCollectorProcesses).Count -eq 0) {
            Write-Warning 'Collector 启动后未保持运行，请手动检查应用日志。'
        }
        else {
            Write-Host 'Collector 已启动。' -ForegroundColor Green
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($backupExecutable) -and (Test-Path -LiteralPath $backupExecutable -PathType Leaf)) {
        Remove-Item -LiteralPath $backupExecutable -Force
    }
}

Add-Artifact -List $artifacts -Path $releaseExecutable
$latestMsi = Get-ChildItem -LiteralPath (Join-Path $tauriRoot 'target\release\bundle\msi') -Filter '*.msi' -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$latestNsis = Get-ChildItem -LiteralPath (Join-Path $tauriRoot 'target\release\bundle\nsis') -Filter '*.exe' -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$latestViewer = Get-ChildItem -LiteralPath $OutputDirectory -Filter 'CodexQuotaSync-viewer-*-win-x64.zip' -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$latestAndroid = Get-ChildItem -LiteralPath $OutputDirectory -Filter 'CodexQuotaSync-android-*-debug.apk' -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
foreach ($artifact in @($latestMsi, $latestNsis, $latestViewer, $latestAndroid)) {
    if ($null -ne $artifact) {
        [void]$artifacts.Add($artifact)
    }
}

Write-Step '全部完成'
foreach ($artifact in @($artifacts | Sort-Object FullName -Unique)) {
    Write-Host ("{0}  ({1:N2} MB)" -f $artifact.FullName, ($artifact.Length / 1MB)) -ForegroundColor Green
}
Write-Host ''
Write-Host '日常更新不会修改 Hooks；仅首次安装或 EXE 路径变化时使用 -InstallHooks。'
