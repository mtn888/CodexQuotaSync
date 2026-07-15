# 安全说明

本项目面向单用户自建环境，按需求允许公网明文 HTTP。额度、重置时间和活动数量不视为敏感信息；Codex 登录凭据仍不得离开 collector。

实现边界：

- Codex token 不写入本项目配置、日志或同步 JSON。
- 登录文件读取上限为 256 KiB，用量响应读取上限为 1 MiB，官方请求不跟随重定向。
- 服务端 PUT 正文上限为 16 KiB，并校验 HMAC 时间戳、正文哈希、revision 与严格 JSON 字段。
- collector 的写密钥以明文保存在本机配置和 UnRaid 环境变量中；不要把完整配置、`.env` 或容器模板截图公开。
- viewer 与 Android 不需要写密钥。
- 未签名 Windows/Android 调试包仅用于开发和自用，系统可能显示来源警告。

报告问题前请移除 token、account ID、原始响应、完整配置、本机路径和含个人信息的截图。
