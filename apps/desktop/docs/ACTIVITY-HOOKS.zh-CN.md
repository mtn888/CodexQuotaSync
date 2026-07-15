# Codex 活动状态 Hooks

Codex Quota Sync 使用 Codex 官方生命周期 Hooks，在本机统计正在执行、等待审批和等待用户输入的任务。Hooks 只维护一份本地 `activity.json`；上传到同步服务器的应当是三个聚合数量，而不是明细。

## 隐私边界

活动跟踪程序会解析 Codex 通过标准输入传入的 JSON，但只保存以下字段：

- `session_id` 的 SHA-256 哈希；
- `turn_id` 的 SHA-256 哈希；
- `executing`、`waitingOnApproval` 或 `waitingOnUserInput` 状态；
- 最后状态变更的 Unix 毫秒时间戳；
- 宿主 Codex PID 与进程启动时间；优先使用 Hook 输入或环境变量，Windows 下缺失时从 Hook 进程的父进程链自动识别。

程序明确不会保存用户提示词、助手回复、工具参数、工具结果、工作目录、模型名、权限原因、对话文件路径或对话文件内容。应用上传服务器时也只需上传聚合数量。

## 本地文件与自愈

默认文件位置：

```text
%APPDATA%\CodexQuotaSync\activity.json
```

便携版可通过环境变量覆盖：

```powershell
$env:CODEX_QUOTA_SYNC_ACTIVITY_PATH = 'D:\CodexQuotaSync\activity.json'
```

文件格式版本为 `1`，示例如下：

```json
{
  "version": 1,
  "updatedAtMs": 1784092800000,
  "entries": [
    {
      "sessionHash": "64 位十六进制 SHA-256",
      "turnHash": "64 位十六进制 SHA-256",
      "status": "waitingOnApproval",
      "updatedAtMs": 1784092800000,
      "hostPid": 12345,
      "hostStartedAtMs": 1784090000000
    }
  ]
}
```

`hostPid` 和 `hostStartedAtMs` 是可选字段。`executing` 的兜底 TTL 为 5 分钟，用来清理中止、强制结束或未触发 `Stop` 的 turn；`waitingOnApproval` 与 `waitingOnUserInput` 的 TTL 仍为 7 天，避免周末期间等待用户的任务被过早删除。同一 session 收到新 turn 时会替换旧 turn。读取时还会检查宿主进程是否存在，并用启动时间防止 PID 重用造成误判。升级到状态文件 V2 时会移除无法判断宿主是否存活的旧版无 PID 条目，后续 Hook 会自动写回当前状态。

Windows 使用名为 `Local\CodexQuotaSyncActivityV1` 的系统互斥量，所有写入都先写临时文件、刷新到磁盘，再用 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` 原子替换。因此多个 Codex 任务同时触发 Hook 时不会互相覆盖，也不会让读取方看到半份 JSON。

## 状态映射

| Codex Hook | 本地变化 |
| --- | --- |
| `UserPromptSubmit` | 新增或更新当前 turn 为 `executing` |
| `PermissionRequest` | 更新为 `waitingOnApproval` |
| `PreToolUse`，工具名为 `request_user_input` | 更新为 `waitingOnUserInput` |
| 任意 `PostToolUse` | 审批或用户输入已经完成，恢复为 `executing` |
| `Stop` | 删除当前 session + turn |
| `SessionStart` 的 `startup`、`resume` 或 `clear` | 删除该 session 的历史残留，并写入 Hooks 可用心跳 |

聚合结果包含：

```json
{
  "total": 3,
  "executing": 1,
  "waitingOnApproval": 1,
  "waitingOnUserInput": 1,
  "stateUpdatedAtMs": 1784092800000,
  "observedAtMs": 1784092860000
}
```

`total` 是三个状态之和，也就是当前尚未结束的顶层 turn 数。

## 安装

安装脚本要求传入当前 Codex Quota Sync 可执行文件的绝对或相对路径。应用内安装按钮应把 `std::env::current_exe()` 的结果传给此参数。

```powershell
Set-Location '应用解压目录\apps\desktop'
.\scripts\install-codex-hooks.ps1 `
  -ExecutablePath 'C:\Program Files\Codex Quota Sync\Codex Quota Sync.exe'
```

如果设置了 `CODEX_HOME`，脚本使用该目录；否则使用 `%USERPROFILE%\.codex`。也可以显式指定：

```powershell
.\scripts\install-codex-hooks.ps1 `
  -ExecutablePath '.\Codex Quota Sync.exe' `
  -CodexHome 'D:\portable-codex-home'
```

脚本会安全合并到 `CODEX_HOME\hooks.json`，并保留所有非 Codex Quota Sync 的事件、matcher 和 handler。重复运行会更新本应用自己的 6 组 handler，不会重复追加。现有文件有效时，原始版本同时备份为：

```text
hooks.json.codex-quota-sync.bak
```

如果现有文件不是合法 JSON，或 `hooks`/事件结构不符合官方数组结构，脚本会直接中止，不会猜测性地改写文件。可以先用 `-WhatIf` 查看目标文件而不写入：

```powershell
.\scripts\install-codex-hooks.ps1 -ExecutablePath '.\Codex Quota Sync.exe' -WhatIf
```

在 Windows 中，安装器会让通用 `command` 继续指向应用本身，并将 `commandWindows` 写成显式的 `cmd.exe /d /s /c call` 包装命令。这样即使 Codex 当前任务环境使用 PowerShell，包含空格的安装路径也能正确启动 Hook。可用实际构建出的 exe 运行安装器回归测试；测试只写入临时 `CODEX_HOME`，并分别验证 CMD 与 PowerShell：

```powershell
.\scripts\test-install-codex-hooks.ps1 `
  -ExecutablePath '.\src-tauri\target\release\codex-quota-sync.exe'
```

### 首次审查与信任

Codex 官方要求非托管命令 Hook 按“完整定义的哈希”进行审查和信任。安装或可执行文件路径变更后，请执行：

1. 完全退出并重新打开 Codex；
2. 在 Codex CLI 输入 `/hooks`；
3. 找到来源为用户级 `~/.codex/hooks.json`、状态消息为 `Codex Quota Sync: updating activity` 的 6 组命令；
4. 展开命令，确认 Windows 命令使用 `cmd.exe /d /s /c call` 启动已安装的 `Codex Quota Sync.exe`，应用参数只有 `--activity-hook`；
5. 信任这些定义；
6. 新建一个测试任务，确认 `%APPDATA%\CodexQuotaSync\activity.json` 出现，并在任务停止后对应条目消失。

不要为了跳过审查而长期使用 `--dangerously-bypass-hook-trust`。程序升级但安装路径不变时定义通常不变；安装路径变化后应重新运行安装脚本并重新审查。

## 卸载

```powershell
Set-Location '应用解压目录\apps\desktop'
.\scripts\uninstall-codex-hooks.ps1
```

指定非默认 `CODEX_HOME`：

```powershell
.\scripts\uninstall-codex-hooks.ps1 -CodexHome 'D:\portable-codex-home'
```

卸载脚本只删除 `statusMessage` 精确等于 `Codex Quota Sync: updating activity` 的 handler；其他 Hook 原样保留。卸载前文件备份为 `hooks.json.codex-quota-sync-uninstall.bak`。卸载后重新启动 Codex。卸载脚本不会删除 `activity.json`，避免卸载过程意外删除用户数据；如确实需要清理，可在应用完全退出后手动删除该文件。

## 已知边界

1. 官方文档说明，当前 `PreToolUse`/`PostToolUse` 只覆盖 Bash、`apply_patch` 和 MCP 工具，并不保证拦截所有内置工具。如果当前 Codex 版本没有把 `request_user_input` 暴露给 Hook，`waitingOnUserInput` 无法仅靠官方 Hook 精确检测，任务会继续显示为 `executing`。安装脚本已经为未来或已支持该工具名的版本配置 matcher，但不能虚构未收到的事件。
2. `PermissionRequest` 能可靠表示审批提示已产生；任意 `PostToolUse` 会在审批完成且工具执行结束后恢复 `executing`。如果 Codex 进程在两者之间崩溃，宿主 PID 自愈会清理残留；无法识别 PID 时等待状态最长保留 7 天。
3. Codex 在重启、中止或子任务结束时不保证产生 `Stop`。同一 session 的新 turn 会替换旧 turn，Codex 宿主重启会通过 PID 清理，剩余 `executing` 残留最长保留 5 分钟。因此极少数连续 5 分钟没有任何 Hook 事件的长推理也可能暂时被视为已停止，下一次 turn/tool Hook 会恢复状态。
4. Hooks 统计的是收到 Hook 的 Codex turn，不读取 Codex 内部私有的 `active / idle / notLoaded` 状态，因此属于 best-effort 观测。精确的等待输入状态若成为硬性要求，需要改用 Codex App Server 事件流或等待官方为内置交互工具补齐 Hook 覆盖。

官方参考：[Codex Hooks](https://learn.chatgpt.com/docs/hooks)。
