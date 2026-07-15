# Windows 构建、配置与使用说明

## 1. 系统要求

- Windows 10/11 x64；
- Microsoft Edge WebView2 Runtime；Windows 11 通常已自带；
- 构建时需要 Node.js 20+、npm、Rust stable MSVC；
- 使用 collector 时，本机 Codex 已登录且能正常显示用量；
- 安装 Hooks 时需要能写入 `%USERPROFILE%\.codex\hooks.json` 或自定义 `CODEX_HOME`。

开发阶段产物未做商业代码签名。Windows SmartScreen 可能显示“未知发布者”，这是未签名调试/个人分发包的预期行为。

## 2. 从源码构建

推荐在仓库根目录直接双击：

```text
update-build-all.cmd
```

也可以在 PowerShell 中运行同一入口：

```powershell
.\update-build-all.ps1
```

默认流程会自动完成：

1. 安装/更新前端依赖并运行前端、Rust 测试；
2. 生成 Windows Release EXE、MSI 和 NSIS；
3. 从现有 Collector 配置读取服务器地址，生成 Windows viewer 便携 ZIP；
4. 运行 Android 单元测试，生成并验证已调试签名的 Debug APK；
5. 重新启动仓库内的 Collector。

脚本不会重新配置 Collector，因此现有 NAS 地址、`sourceId` 和写入密钥都会保留；日志只显示密钥是否存在，不显示内容。日常更新也不会修改或重新信任 Hooks。只有首次安装 Hooks 或 Release EXE 的绝对路径发生变化时才运行：

```powershell
.\update-build-all.ps1 -InstallHooks
```

可选的 `-UpdateSource` 会先执行 `git pull --ff-only`，但只允许在完全干净的工作区运行，不会自动 stash/reset。`-SkipTests`、`-SkipViewer`、`-SkipAndroid` 和 `-NoRestart` 可用于临时跳过对应步骤。

以下是脚本内部执行的手动等价流程：

```powershell
Set-Location '<仓库根目录>\apps\desktop'
npm install
npm test
npm run build

Set-Location '.\src-tauri'
cargo fmt --all -- --check
cargo test --all-targets

Set-Location '..'
npm run tauri -- build
```

主要产物位于：

```text
apps\desktop\src-tauri\target\release\codex-quota-sync.exe
apps\desktop\src-tauri\target\release\bundle\msi\
apps\desktop\src-tauri\target\release\bundle\nsis\
dist\CodexQuotaSync-viewer-<版本>-win-x64.zip
dist\CodexQuotaSync-android-<版本>-debug.apk
```

只需要快速调试可执行文件时：

```powershell
Set-Location '.\src-tauri'
cargo build
```

产物为 `target\debug\codex-quota-sync.exe`。调试版也支持 `--activity-hook`。

## 3. collector 配置

主 Windows 电脑是唯一 collector。先从仓库的 `apps\desktop` 目录执行：

```powershell
.\scripts\configure-sync.ps1 `
  -Role collector `
  -ServerUrl 'http://nas.example.com:18080' `
  -WriteSecret '与 UnRaid CQS_WRITE_SECRET 完全相同的值' `
  -SourceId 'windows-main'
```

如果暂时只想在本机使用悬浮框、不上传服务器：

```powershell
.\scripts\configure-sync.ps1 -Role collector -SourceId 'windows-main'
```

脚本把配置写入：

```text
%APPDATA%\io.github.mtn888.codexquotasync\preferences.json
```

配置采用临时文件和原子替换；已有文件会备份为 `preferences.json.bak`。脚本不会回显密钥。修改后完全退出并重新启动 Codex Quota Sync。

collector 的行为：

- 继续直接使用本机 Codex 登录态，无额外登录页面；
- 常规最多每 5 分钟请求一次用量；临近重置时每分钟；
- 每分钟读取一次活动聚合并上传；
- 如果服务器离线，本地额度仍正常显示，右下角同步状态标为“服务器离线”；
- 写密钥不会传给前端 WebView，也不会上传到其他设备。

## 4. 安装 Codex 活动 Hooks

先确定以后长期使用的可执行文件绝对路径。安装路径变化后 Hook 定义也必须重新安装。以源码 release exe 为例：

```powershell
$exe = (Resolve-Path '.\src-tauri\target\release\codex-quota-sync.exe').Path
.\scripts\install-codex-hooks.ps1 -ExecutablePath $exe
```

使用安装器后，请把参数改为实际安装目录中的 exe。可在任务管理器中对 Codex Quota Sync 选择“打开文件所在的位置”确认。

脚本会合并 6 组官方 Hook，不覆盖现有用户 Hook，并备份原文件。随后：

1. 完全退出并重新打开 Codex；
2. 在 Codex 中打开 `/hooks`；
3. 找到状态消息为 `Codex Quota Sync: updating activity` 的命令；
4. 确认 Windows 命令通过 `cmd.exe /d /s /c call` 启动上述 exe，应用参数只有 `--activity-hook`；
5. 审查并信任；
6. 新建任务，确认 `%APPDATA%\CodexQuotaSync\activity.json` 出现；
7. 触发一次工具审批，展开卡片确认“待审批”数量变化。

完整隐私、事件和卸载说明见 [Codex 活动状态 Hooks](../apps/desktop/docs/ACTIVITY-HOOKS.zh-CN.md)。

## 5. viewer 配置

其他 Windows 电脑安装相同程序，但不需要安装 Codex、不需要登录，也不需要写密钥：

```powershell
.\scripts\configure-sync.ps1 `
  -Role viewer `
  -ServerUrl 'http://nas.example.com:18080' `
  -SourceId 'windows-viewer-laptop'
```

也可以在仓库根目录直接生成便携版压缩包：

```powershell
.\package-windows-viewer.ps1
```

默认输出到 `dist\CodexQuotaSync-viewer-<版本>-win-x64.zip`。脚本会把 release EXE、viewer 配置脚本和服务器地址一起打包。其他电脑完整解压后，右键运行 `setup-windows-viewer.ps1` 即可自动生成设备标识、配置 viewer、检查服务器并启动悬浮框。

打包脚本和包内初始化脚本兼容 Windows PowerShell 5.1 与 PowerShell 7。初始化脚本可以重复运行；已有配置会先备份为 `preferences.json.bak`。

需要临时更换打包进去的服务器地址时：

```powershell
.\package-windows-viewer.ps1 -ServerUrl 'http://nas.example.com:18080'
```

viewer 每分钟读取 `/v1/status`。连接失败时继续显示进程内最后一份远端快照并标为离线；`collectedAt` 超过 15 分钟后标为过期。重启 viewer 且服务器仍离线时，因为桌面端不把远端 JSON 持久化到磁盘，会显示不可用，直到服务器恢复。

## 6. 悬浮框操作

- 默认是 80×80 悬浮球，显示当前主要额度；
- 鼠标移入展开为 320×320 卡片；
- 右上角图标可保持展开或切换始终置顶；
- 右下三格依次为执行中、待审批、待输入；需要操作时使用琥珀/珊瑚色强调；
- 悬浮球右上红色角标是“待审批 + 待输入”的总数；左下绿点表示仍有执行中任务；
- 托盘菜单可显示/隐藏、立即刷新、解锁鼠标穿透、切换中英文、开机启动或退出；
- 拖到屏幕边缘后会吸附，展开时自动避开工作区边界和任务栏。

## 7. 手工检查配置

只检查非敏感字段：

```powershell
$path = Join-Path $env:APPDATA 'io.github.mtn888.codexquotasync\preferences.json'
$config = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
$config | Select-Object syncRole,serverUrl,sourceId,activityStatePath
```

不要把整个配置文件贴到 issue、聊天或截图中，因为 collector 文件含 `writeSecret`。

## 8. 更新与回滚

更新桌面程序前：

1. 记录当前 exe 的实际路径；
2. 备份 `preferences.json`；
3. 安装或替换新版本；
4. 如果 exe 路径变化，重新运行 `install-codex-hooks.ps1` 并在 `/hooks` 重新审查；
5. 手动刷新一次，检查同步状态。

回滚时恢复旧 exe/安装包即可。协议仍为 `schemaVersion=1` 时无需修改服务器数据。若新版本已经把配置结构写回，脚本生成的 `preferences.json.bak` 可用于恢复。

项目已移除 Quota Float 上游自动更新器、公钥和 release endpoint，避免从另一个仓库自动安装不兼容版本。当前更新通过本仓库构建或未来的 `mtn888/CodexQuotaSync` 发布包进行。

## 9. 卸载

先删除 Hooks：

```powershell
.\scripts\uninstall-codex-hooks.ps1
```

再卸载程序。确认不再需要后，可手动删除：

```text
%APPDATA%\io.github.mtn888.codexquotasync
%APPDATA%\CodexQuotaSync\activity.json
```

卸载脚本不会自动删除活动文件，以免误删仍在使用的数据。

## 10. 常见问题

### collector 显示“请先登录 Codex”

先打开 Codex Desktop 确认登录有效。Codex Quota Sync 不提供自己的登录流程，也不接收手工 token。

### 显示“同步未配置”

重新运行配置脚本，确认 URL 以 `http://` 开头。collector 使用服务器时还必须提供 secret。

### 显示“服务器离线”

```powershell
Invoke-RestMethod 'http://nas.example.com:18080/healthz'
Invoke-RestMethod 'http://nas.example.com:18080/v1/status'
```

若局域网可用而公网不可用，排查端口转发、域名、运营商 CGNAT 和防火墙。

### 活动数量始终为 0

检查 `activity.json` 是否存在、Hook 是否在 `/hooks` 中被信任、Hook 命令的 exe 路径是否仍有效。等待输入统计是 best-effort，具体限制见 Hooks 文档。

### 重启后执行中数量仍有残留

新版会自动记录 Codex 宿主 PID：Codex 重启后，Collector 下一次读取会清理旧进程对应的状态；同一 session 的新 turn 也会替换旧 turn。无法通过生命周期事件清理的 `executing` 最长保留 5 分钟，而“待审批”和“待输入”仍按 7 天保留。首次从旧版升级时会一次性清理无法判断宿主是否存活的 V1 无 PID 条目。
