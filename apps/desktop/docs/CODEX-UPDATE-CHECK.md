# Codex 更新兼容性检查

桌面 collector 依赖 Codex 本机登录文件和官方用量响应。Codex Desktop 更新后，如果登录文件结构、用量接口字段或认证请求头发生变化，直接读取额度可能失效。

在 `apps\desktop` 中运行：

```powershell
npm run check:codex
```

强制完整检查：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\check-codex-update.ps1 -Force
```

跳过真实用量接口探测，只运行本地测试和构建：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\check-codex-update.ps1 -Force -SkipLive
```

脚本检查 Codex 可执行文件指纹、前端测试、Rust 测试、生产构建和可选的真实用量响应结构。它不会打印或保存 token、account ID、原始响应、提示词或聊天记录。成功后的指纹保存在已被 Git 忽略的 `.codex-update-check-state.json`。

真实探测返回 401/403 时，先在 Codex 中重新登录；持续失败可能表示认证头发生变化。无法识别 5 小时窗口时，需要更新 `src-tauri\src\codex.rs` 的解析逻辑。
