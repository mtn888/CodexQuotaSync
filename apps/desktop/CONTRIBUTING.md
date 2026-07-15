# 参与开发

提交问题或代码前，不要附带 token、account ID、原始后端响应、完整 `preferences.json`、本机认证路径或含个人信息的截图。

桌面端改动至少运行：

```powershell
npm test
npm run build
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --manifest-path .\src-tauri\Cargo.toml --all-targets
```

修改额度解析、同步契约、快照合并、活动计数或时间格式时，应同步更新测试和中文文档。不得加入原始响应日志、遥测，或把写密钥返回 WebView。
