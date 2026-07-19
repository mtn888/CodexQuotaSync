# 隐私说明

Codex Quota Sync 采用本地优先设计。

## collector 读取的内容

- 从 `CODEX_HOME\auth.json` 或 `%USERPROFILE%\.codex\auth.json` 读取现有 Codex 登录状态。
- access token 只发送到 `https://chatgpt.com` 的 Codex 用量接口。
- account ID 只用于官方用量请求要求的请求头。
- 可选 Hooks 只处理事件类型以及 session/turn 标识；标识写入前会做 SHA-256，不读取提示词、回复、工具参数或 transcript。

## 同步的内容

服务器仅接收归一化后的剩余额度、重置时间、采集状态、时间戳，以及执行中、待审批、待输入数量。不会同步 token、account ID、原始响应、提示词、回复、本机路径或原始 session/turn ID。

按项目需求，同步链路使用明文 HTTP，因此这些状态值可能被网络中间方看到或篡改。写入使用 HMAC 防止无密钥的客户端直接覆盖状态，但 HMAC 不提供传输加密。

## 本地保存

Windows 配置保存在 `%APPDATA%\io.github.mtn888.codexquotasync\preferences.json`。collector 的 HMAC 写密钥保存在该文件中；设置页只在 collector 角色下提供一次性写入的密码输入，已保存的密钥不会由 Rust 通过配置或设置事件返回 WebView。填写非空值会替换密钥，留空则保留原值；保存为 viewer 会清除本机密钥。Android 只保存服务地址、最后成功快照和获取时间，不持有写密钥。

项目不包含遥测、分析、崩溃上报或第三方追踪。
