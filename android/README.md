# Codex Quota Sync Android 小组件

这是 `Codex Quota Sync` 的只读 Android 客户端。它通过 HTTP `GET /v1/status` 读取同步服务中的状态，不包含上传接口，也不保存服务器写入密钥。

## 功能

- 显示 5 小时额度剩余百分比和周额度剩余百分比。
- 显示下一次额度重置时间，并标明最近将重置的是 5 小时额度还是周额度。
- 离线快照中的重置时间已经过去时，不再把历史时间误标为“下次重置”；若另一个额度窗口仍有未来时间，则改为显示该窗口。
- 显示“执行中”和“待处理”任务数；待处理等于待审批与待输入之和。
- 显示在线、过期、离线、刷新中和未配置状态。
- 成功同步后缓存最后一份有效快照；离线时继续展示缓存并明确标注“离线”。
- 提供 1×1 悬浮球、4×1 四格条和 4×3 完整卡片三种桌面组件。
- 使用 WorkManager 每 15 分钟请求一次，网络失败会指数退避重试；AppWidget 每 30 分钟提供备用触发。
- 支持 UnRaid 上的明文 HTTP 服务和自定义端口。

## 环境要求

- JDK 17。
- Android SDK Platform 35 和 Build Tools 35.x。
- 构建使用 Gradle Wrapper，无需单独安装 Gradle。

工程配置为 `minSdk 26`、`targetSdk 35`、`compileSdk 35`。

## 构建与测试

推荐在仓库根目录双击 `update-build-all.cmd`，它会同时生成 Windows、Windows viewer 和 Android 产物；也可以在 PowerShell 中运行：

```powershell
.\update-build-all.ps1
```

经过签名验证并重命名后的 APK 会复制到：

```text
dist\CodexQuotaSync-android-<版本>-debug.apk
```

只构建 Android 或需要手动排查时，在 Windows PowerShell 中执行：

```powershell
Set-Location '<仓库根目录>\android'
.\gradlew.bat :app:testDebugUnitTest :app:assembleDebug
```

生成的调试 APK 位于：

```text
android\app\build\outputs\apk\debug\app-debug.apk
```

安装到已连接的 Android 设备：

```powershell
adb install -r .\app\build\outputs\apk\debug\app-debug.apk
```

调试包使用 Android 自动生成的调试签名，适合开发和个人侧载，不能作为正式商店发布包。

## 首次配置

1. 安装 APK。
2. 长按手机桌面空白处，选择“小组件”。
3. 找到 `Codex Quota Sync`，选择 1×1、4×1 或 4×3 组件并拖到桌面。
4. 在配置页输入同步服务的 Base URL，例如 `http://nas.example.com:18080`。
5. 点击“保存并刷新小组件”。

点击应用图标或小组件主体会进入状态详情页，集中显示两个额度窗口、重置时间、执行中、待处理、连接状态和最近采集结果；“服务器设置”按钮才进入配置页。已有 Base URL 时，添加第二种组件不会重复要求配置。直连本项目 Go 服务时，Base URL 应为 `http://主机:端口`，客户端会在末尾追加 `/v1/status`。只有在反向代理会剥离路径前缀时，才可填写 `http://主机:端口/codex` 一类地址。如果直接填入以 `/v1/status` 结尾的完整地址，也不会重复追加。

三种组件使用相同状态底色：执行中和待处理都为 0 时是灰色；执行中大于 0 且待处理为 0 时是绿色；待处理大于 0 时黄色优先。

应用允许 `http://` 明文流量。也兼容 `https://`，但不会强制使用 HTTPS。

## 接口字段

解析逻辑遵循仓库根目录的 `schema/status-v1.schema.json`，当前使用下列字段：

| JSON 字段 | 小组件用途 |
| --- | --- |
| `schemaVersion` | 必须为 `1` |
| `collectedAt` | 最后更新时间和 45 分钟过期判断 |
| `activity.executing` | 执行中任务数 |
| `activity.waitingOnApproval` | 等待用户审批的任务数 |
| `activity.waitingOnUserInput` | 等待用户输入的任务数 |
| `activity.stale` | 活动统计是否过期 |
| `latestAttempt.status` | 最近采集是否成功 |
| `lastGoodSnapshot.shortWindow.remainingPercent` | 5 小时额度剩余百分比 |
| `lastGoodSnapshot.weeklyWindow.remainingPercent` | 周额度剩余百分比 |
| `lastGoodSnapshot.nextResetAt` | 下一次重置时间 |
| `lastGoodSnapshot.nextResetWindow` | 最近将重置的类型：`5h` 或 `weekly` |
| `lastGoodSnapshot.status` | 额度快照状态 |

`lastGoodSnapshot` 为 `null` 或单个窗口为 `null` 时，相应额度显示为 `—`，应用不会崩溃。响应超过 512 KiB、Schema 版本不支持、必填字段缺失或时间格式非法时，本次同步视为失败。

## 更新行为与 Android 限制

WorkManager 的周期任务最短间隔是 15 分钟，但这不是精确定时器。Doze、厂商省电策略、无网络和后台限制都可能延后执行。临时网络失败会从 1 分钟开始指数退避重试；系统 AppWidget 的 30 分钟更新回调还会补排一次任务。点击刷新会立即排队一次带联网约束的请求。45 分钟过期阈值用于容纳系统调度抖动，不代表系统承诺 45 分钟内一定运行。

如果某些厂商系统长期不更新：

1. 在澎湃 OS 的应用电量设置中选择“不限制”。
2. 允许 `Codex Quota Sync` 后台运行和自启动，并加入电池优化白名单。
3. 确认公网地址和自定义端口可从手机网络访问。
4. 点击刷新图标，根据“离线”提示验证连接。

## 数据和安全边界

- 客户端只发起匿名 `GET`，不会调用写入接口。
- 本地只保存 Base URL、最后一份成功 JSON 和成功获取时间。
- 配置页拒绝 URL 中的 `user:password@host`，避免把凭据误存入配置。
- 明文 HTTP 内容可能被网络中间方读取或篡改；此选择适用于需求中明确不敏感的状态数据。
