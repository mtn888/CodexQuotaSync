# Codex Quota Sync Windows 客户端

该目录包含 Tauri 2 + React 桌面客户端，一个安装包支持两种角色：

- `collector`：在主 Windows 电脑复用现有 Codex 登录态读取额度，聚合本机 Hooks 活动状态，并向 UnRaid 服务上传脱敏快照。
- `viewer`：在其他 Windows 电脑每分钟读取服务端快照，不需要安装、登录或复制 Codex 凭据。

两种角色使用同一套悬浮球、展开卡片、托盘、置顶、拖动、开机启动和中英文界面。

展开卡片右上角齿轮或托盘“设置”可打开独立设置窗口。它支持修改 HTTP 服务器地址和端口、设备标识、Hooks 状态文件与关机脚本路径；collector 还会显示仅写、不回显的写入密钥输入框，留空保存会保留原密钥。

collector 还可用悬浮卡电源按钮或设置页一次性武装“完成后关机”。它只在本运行期可信 Hooks 状态已经观察到执行中任务、随后任务数变为 0 时启动配置的 `.cmd`/`.bat` 脚本；应用重启、保存为 viewer，或保存时变更角色、Hooks 状态文件、关机脚本都会关闭开关，过期/不可用的 Hooks 状态只会清除完成判断基线。服务器离线不阻止已确认完成后的本机脚本执行。

## 配置与使用

完整步骤见：

- [Windows 使用说明](../../docs/Windows使用说明.md)
- [Codex 活动状态 Hooks](docs/ACTIVITY-HOOKS.zh-CN.md)
- [UnRaid 部署说明](../../docs/UnRaid部署说明.md)

collector 常规最多每 5 分钟请求 Codex 用量，距离下一次重置 15 分钟内改为每分钟；活动聚合与服务端同步每分钟执行。viewer 每分钟读取一次服务端。悬浮卡片中的“立即刷新”和托盘刷新会绕过缓存。

## 开发与验证

依赖 Node.js 20+、Rust stable，以及 Tauri 2 的 Windows 构建依赖。

```powershell
npm install
npm test
npm run build

Set-Location .\src-tauri
cargo fmt --all -- --check
cargo test --all-targets

Set-Location ..
npm run tauri -- build
```

未签名 Windows 安装包通常生成在：

```text
src-tauri\target\release\bundle\msi\
src-tauri\target\release\bundle\nsis\
```

未签名包可能触发 Windows SmartScreen，这是开发构建的预期现象。

Codex Desktop 更新后，可运行 `npm run check:codex` 检查本机登录文件和用量响应是否仍兼容。说明见 [Codex 更新兼容性检查](docs/CODEX-UPDATE-CHECK.md)。

## 隐私与来源

客户端不上传 Codex token、account ID、提示词、回复或原始用量响应。同步边界见 [PRIVACY.md](PRIVACY.md) 与 [SECURITY.md](SECURITY.md)。本客户端基于 MIT 许可的 Quota Float `v0.1.5` 改造，署名见仓库根目录的 `LICENSE` 与 `THIRD_PARTY_NOTICES.md`。
