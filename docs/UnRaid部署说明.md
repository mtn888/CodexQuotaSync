# Codex Quota Sync：UnRaid 超详细部署说明

本文面向有公网 IP、域名和自定义端口转发能力的个人 UnRaid。最终形态只有一个 Go 容器和一个 JSON 文件，不需要 MySQL、PostgreSQL、Redis、反向代理或 HTTPS。

## 1. 部署完成后的结构

```text
/mnt/user/appdata/codex-quota-sync/
├── repo/                 # 本仓库源码，可删除后重新 clone
│   └── server/
│       ├── Dockerfile
│       ├── docker-compose.yml
│       └── .env          # 自己创建，不提交 Git
└── data/
    └── state.json        # 首次成功上传后由服务创建
```

容器内部：

- 监听 `8787/tcp`；
- `/data/state.json` 是唯一持久化状态；
- 进程以 UID:GID `99:100` 运行；
- 根文件系统只读，只有 `/data` bind mount 和 `/tmp` tmpfs 可写；
- 健康检查请求 `http://127.0.0.1:8787/healthz`。

建议端口示例：

| 层级 | 示例 | 说明 |
| --- | ---: | --- |
| 容器端口 | `8787` | 保持默认 |
| UnRaid Host 端口 | `18080` | 可换为任意未占用 TCP 端口 |
| 路由器公网端口 | `18080` | 可与 Host 相同，最容易排障 |
| 最终 URL | `http://quota.example.com:18080` | 客户端自动追加 `/v1/status` |

## 2. 部署前检查清单

在开始前确认：

- UnRaid Docker 服务已启用；
- 有 Community Applications 的 Compose Manager 插件，或能在 Terminal 使用 `docker compose`；
- `/mnt/user/appdata` 位于有持久化能力的 share；
- 路由器知道 UnRaid 的固定局域网 IP；
- 公网 IP 不是运营商 CGNAT；
- 域名已经可管理 DNS，或准备使用 DDNS；
- 主 Windows 和 UnRaid 都启用了自动时间同步；HMAC 时间容差只有 5 分钟；
- 你接受 HTTP 明文传输额度和数量。HMAC 保护写入，但不加密内容。

检查 UnRaid 时间：

```bash
date
date -u
```

检查 Docker/Compose：

```bash
docker version
docker compose version
```

## 3. 推荐方式：Compose 部署

### 3.1 创建目录

在 UnRaid Terminal 执行：

```bash
mkdir -p /mnt/user/appdata/codex-quota-sync/repo
mkdir -p /mnt/user/appdata/codex-quota-sync/data
chown -R 99:100 /mnt/user/appdata/codex-quota-sync/data
chmod 700 /mnt/user/appdata/codex-quota-sync/data
```

`repo` 目录可以由当前登录用户拥有；`data` 必须允许容器内 UID 99 写入。

### 3.2 获取代码

远程仓库已经可用时：

```bash
cd /mnt/user/appdata/codex-quota-sync
git clone https://github.com/mtn888/CodexQuotaSync.git repo
cd repo/server
```

如果 `repo` 已存在：

```bash
cd /mnt/user/appdata/codex-quota-sync/repo
git pull --ff-only
cd server
```

也可以从 Windows 下载仓库 ZIP，只把 `server` 目录完整复制到 `/mnt/user/appdata/codex-quota-sync/repo/server`。不要只复制 Dockerfile；Compose、Go 源码和 `go.mod` 都是构建上下文的一部分。

### 3.3 生成写密钥

```bash
openssl rand -hex 32
```

输出示例是 64 位十六进制。真实值只配置在：

1. UnRaid `server/.env`；
2. 主 Windows collector 的 `preferences.json`（通过脚本写入）。

不要配置到 viewer 或 Android，也不要使用 Codex token 充当这个 secret。

### 3.4 创建 `.env`

在 `/mnt/user/appdata/codex-quota-sync/repo/server/.env` 写入：

```dotenv
CQS_WRITE_SECRET=替换为openssl生成的值
CQS_HOST_PORT=18080
CQS_DATA_DIR=/mnt/user/appdata/codex-quota-sync/data
TZ=Asia/Singapore
```

也可以把时区改成 `Asia/Shanghai`。服务端 API 时间仍为 RFC 3339 UTC；TZ 主要影响系统/日志语境。

限制权限：

```bash
chmod 600 /mnt/user/appdata/codex-quota-sync/repo/server/.env
```

检查 Compose 展开结果，但不要把包含 secret 的完整输出贴到公开位置：

```bash
cd /mnt/user/appdata/codex-quota-sync/repo/server
docker compose config --quiet
```

### 3.5 构建镜像

```bash
docker compose build --pull
```

第一次需要下载 `golang:1.24-alpine` 和 `alpine:3.22`，耗时取决于 Docker Hub 网络。构建成功后：

```bash
docker image inspect codex-quota-sync-server:local --format '{{.Id}} {{.Size}}'
```

### 3.6 启动

```bash
docker compose up -d
docker compose ps
docker compose logs --tail=100 codex-quota-sync
```

期望 `docker compose ps` 显示 `Up`，稍后显示 `healthy`。日志应包含监听地址和数据文件路径，但不会打印 secret、请求正文或签名。

### 3.7 Compose Manager 图形界面

如果使用 UnRaid Compose Manager：

1. 新建 Stack，名称 `codex-quota-sync`；
2. Stack 目录指向 `/mnt/user/appdata/codex-quota-sync/repo/server`；
3. 确保 `.env` 与 `docker-compose.yml` 同目录；
4. 先执行 Build，再执行 Compose Up；
5. 在 Stack 日志里检查健康状态。

插件版本不同，按钮名称可能略有差异，但底层仍是同一份 `docker-compose.yml`。

## 4. 备选方式：UnRaid Docker XML 模板

仓库提供 `server/unraid/codex-quota-sync.xml`。它引用本地镜像，所以先构建：

```bash
cd /mnt/user/appdata/codex-quota-sync/repo/server
docker build -t codex-quota-sync-server:local .
```

复制模板：

```bash
cp unraid/codex-quota-sync.xml /boot/config/plugins/dockerMan/templates-user/my-codex-quota-sync.xml
```

如果当前目录是仓库根目录而不是 `server`，使用：

```bash
cp server/unraid/codex-quota-sync.xml /boot/config/plugins/dockerMan/templates-user/my-codex-quota-sync.xml
```

然后在 UnRaid：

1. 打开 **Docker → Add Container**；
2. Template 选择 **Codex Quota Sync Server**；
3. Repository 保持 `codex-quota-sync-server:local`；
4. Web Port 设置为 Host `18080`、Container `8787`；
5. Data 设置为 `/mnt/user/appdata/codex-quota-sync/data` → `/data`；
6. Write Secret 填随机密钥；
7. `PORT=8787`、`DATA_FILE=/data/state.json` 保持默认；
8. 应用并等待容器健康。

Compose 和 XML 二选一，不要同时启动两个同名容器或占用同一个 Host 端口。

## 5. 局域网验证

先不要配置公网转发。浏览器或另一台局域网电脑执行：

```powershell
Invoke-RestMethod 'http://UNRAID局域网IP:18080/healthz'
```

应返回：

```json
{"status":"ok"}
```

collector 尚未上传时：

```powershell
Invoke-WebRequest 'http://UNRAID局域网IP:18080/v1/status'
```

返回 404 是正常的，表示服务已运行但还没有快照。它不同于连接超时或拒绝连接。

查看 UnRaid 监听端口：

```bash
docker port codex-quota-sync-server
ss -lnt | grep 18080
```

## 6. 配置主 Windows collector

在仓库 `apps\desktop` 目录：

```powershell
.\scripts\configure-sync.ps1 `
  -Role collector `
  -ServerUrl 'http://UNRAID局域网IP:18080' `
  -WriteSecret '与CQS_WRITE_SECRET完全一致' `
  -SourceId 'windows-main'
```

重启 Codex Quota Sync，展开悬浮卡片，右下角应从“仅本机/同步未配置”变为“已同步”。点击托盘“立即刷新”后再次请求：

```powershell
$status = Invoke-RestMethod 'http://UNRAID局域网IP:18080/v1/status'
$status | Select-Object schemaVersion,sourceId,revision,collectedAt,receivedAt
$status.activity
$status.lastGoodSnapshot | Select-Object nextResetAt,nextResetWindow
```

不要把整个状态对象作为故障日志公开发送；虽然设计上不含 token，计划名和时间仍可能透露使用习惯。

接着按 [Windows 使用说明](Windows使用说明.md) 安装 Codex Hooks。没有 Hooks 或 Hooks 从未成功写入状态时，额度同步仍能工作，但活动来源会显示 unavailable；Hooks 已可用且当前没有任务时，三个数量才会可信地显示为 0。

## 7. 公网、域名和端口转发

### 7.1 固定 UnRaid 局域网地址

在路由器 DHCP 静态租约中固定 UnRaid，例如 `192.168.1.20`。不要把转发规则指向可能变化的 DHCP 地址。

### 7.2 配置路由器

新增 TCP 转发：

```text
公网 18080/TCP  →  192.168.1.20:18080/TCP
```

只转发 Codex Quota Sync 端口。不要转发 UnRaid Web 管理端口、SSH（除非另有安全方案）或 Docker socket。

### 7.3 配置 DNS/DDNS

- 固定 IPv4：给 `quota.example.com` 添加 A 记录；
- 固定 IPv6：谨慎添加 AAAA，并确认 IPv6 防火墙仅开放 18080；
- 动态公网 IP：在路由器或 UnRaid 配置 DDNS，再让域名 CNAME 到 DDNS 名称。

DNS 解析检查：

```powershell
Resolve-DnsName quota.example.com
```

### 7.4 从真正的外网测试

手机关闭 Wi-Fi，用移动网络打开：

```text
http://quota.example.com:18080/healthz
```

返回 `{"status":"ok"}` 才说明公网链路成功。在内网访问公网域名失败但手机网络成功，通常是路由器不支持 NAT Loopback；内网设备可暂时填局域网 URL，或在本地 DNS 把域名解析到 UnRaid 局域网 IP。

如果路由器 WAN 地址与网站查询到的公网地址不同，通常处于 CGNAT。需要向运营商申请公网 IP，或改用 Tailscale/WireGuard/Cloudflare Tunnel。后两者已经超出本项目“纯 HTTP 公网端口”的默认部署范围。

### 7.5 切换 collector 到公网域名

外网验证后可把 collector URL 也改为域名，或主电脑继续使用局域网地址：

```powershell
.\scripts\configure-sync.ps1 `
  -Role collector `
  -ServerUrl 'http://quota.example.com:18080' `
  -WriteSecret '原写密钥' `
  -SourceId 'windows-main'
```

## 8. 配置其他设备

### 8.1 Windows viewer

```powershell
.\scripts\configure-sync.ps1 `
  -Role viewer `
  -ServerUrl 'http://quota.example.com:18080' `
  -SourceId 'windows-viewer-laptop'
```

viewer 不需要 secret，不需要 Codex 登录，也不安装活动 Hooks。

### 8.2 Android 小组件

1. 安装 `android/app/build/outputs/apk/debug/app-debug.apk`；
2. 长按桌面空白处，添加 **Codex Quota Sync** 小组件；
3. 输入 `http://quota.example.com:18080`；
4. 保存并刷新；
5. 关闭 Wi-Fi 再点右上角刷新，确认公网可用。

Android 自动更新周期为 15 分钟，Doze 可能延迟。点击刷新可立即排队。

## 9. HTTP 明文的明确边界

本部署按需求不使用 HTTPS。公网链路上的运营商、公共 Wi-Fi、路由器和中间设备可以看到或篡改 GET/PUT 正文。当前正文只有额度、重置时间、计划名、任务数量和时间戳，不含 Codex token 或对话。

HMAC 能做到：

- 不知道 secret 的人无法构造有效 PUT；
- 请求体被修改后签名失效；
- 5 分钟外的旧请求被拒绝；
- 旧 revision 不能覆盖新状态。

HMAC 不能做到：

- 加密正文；
- 隐藏访问频率；
- 认证公开 GET 返回值；
- 阻止中间人丢弃请求或返回伪造 viewer 数据。

如果未来加入账号、提示词、历史趋势或其他隐私数据，必须先切换到 HTTPS、VPN 或可信反向代理。

## 10. 状态文件、备份与恢复

查看权限：

```bash
ls -la /mnt/user/appdata/codex-quota-sync/data
```

正常的 `state.json` 在容器写入时权限为 `0600`。备份前无需停止容器，因为替换是原子的，但为了获得可重复的运维流程，推荐：

```bash
cd /mnt/user/appdata/codex-quota-sync/repo/server
docker compose stop codex-quota-sync
cp /mnt/user/appdata/codex-quota-sync/data/state.json \
   /mnt/user/appdata/codex-quota-sync/data/state.json.backup
docker compose start codex-quota-sync
```

恢复：

```bash
docker compose down
cp /mnt/user/appdata/codex-quota-sync/data/state.json.backup \
   /mnt/user/appdata/codex-quota-sync/data/state.json
chown 99:100 /mnt/user/appdata/codex-quota-sync/data/state.json
chmod 600 /mnt/user/appdata/codex-quota-sync/data/state.json
docker compose up -d
```

如果文件损坏且不需要保留：

```bash
docker compose down
mv /mnt/user/appdata/codex-quota-sync/data/state.json \
   /mnt/user/appdata/codex-quota-sync/data/state.json.bad
docker compose up -d
```

服务恢复为 404，等待 collector 上传。不要在容器运行时手工编辑 `state.json`，内存状态不会自动重载。

## 11. 更新、回滚和密钥轮换

### 11.1 更新前备份镜像和数据

```bash
docker image tag codex-quota-sync-server:local codex-quota-sync-server:rollback
cp /mnt/user/appdata/codex-quota-sync/data/state.json \
   /mnt/user/appdata/codex-quota-sync/data/state.json.pre-update
```

### 11.2 更新源码并重建

```bash
cd /mnt/user/appdata/codex-quota-sync/repo
git pull --ff-only
cd server
docker compose build --pull
docker compose up -d
docker compose ps
curl -fsS http://127.0.0.1:18080/healthz
```

### 11.3 回滚镜像

```bash
cd /mnt/user/appdata/codex-quota-sync/repo/server
docker compose down
docker image tag codex-quota-sync-server:rollback codex-quota-sync-server:local
docker compose up -d --force-recreate
```

如果新版本改变了 `state.json` 且旧版拒绝启动，再恢复备份：

```bash
cd /mnt/user/appdata/codex-quota-sync/repo/server
docker compose down
cp /mnt/user/appdata/codex-quota-sync/data/state.json.pre-update \
   /mnt/user/appdata/codex-quota-sync/data/state.json
chown 99:100 /mnt/user/appdata/codex-quota-sync/data/state.json
chmod 600 /mnt/user/appdata/codex-quota-sync/data/state.json
docker compose up -d --force-recreate
curl -fsS http://127.0.0.1:18080/healthz
```

当前 v1 协议下通常无需恢复文件，也无需迁移数据库。

### 11.4 轮换写密钥

1. 生成新 secret；
2. 更新 UnRaid `.env`；
3. 在 collector 运行 `configure-sync.ps1` 写入同一新值；
4. `docker compose up -d --force-recreate`；
5. 重启 collector 并立即刷新；
6. viewer/Android 不需要修改。

短暂切换窗口内 PUT 可能 401，但不会损坏最后有效快照。

## 12. 监控与日常检查

容器状态：

```bash
docker compose ps
docker inspect --format '{{json .State.Health}}' codex-quota-sync-server
docker compose logs --since=24h codex-quota-sync
```

公开状态的新鲜度：

```powershell
$status = Invoke-RestMethod 'http://quota.example.com:18080/v1/status'
[pscustomobject]@{
  Source = $status.sourceId
  CollectedAt = $status.collectedAt
  ReceivedAt = $status.receivedAt
  LatestAttempt = $status.latestAttempt.status
  Executing = $status.activity.executing
  WaitingApproval = $status.activity.waitingOnApproval
  WaitingInput = $status.activity.waitingOnUserInput
}
```

服务端没有历史数据库和内建告警。需要告警时可让 UnRaid 现有监控系统只检查 `/healthz` 和 `collectedAt`，不要抓取或记录完整 JSON。

## 13. 故障排查表

| 现象 | 最可能原因 | 检查/处理 |
| --- | --- | --- |
| 容器反复重启，提示缺 secret | `.env` 未加载或变量为空 | 确认 `.env` 与 Compose 同目录；运行 `docker compose config` |
| `storage_failed` / permission denied | data 目录所有者错误 | `chown -R 99:100 .../data`，`chmod 700 .../data` |
| `/healthz` 连接拒绝 | 端口未映射或容器未启动 | `docker compose ps`、`docker port`、检查 Host 端口冲突 |
| `/v1/status` 404 | 尚无成功 PUT | 查看 collector 右下同步状态和容器日志 |
| collector 401 invalid_timestamp | NAS/Windows 时间差超过 5 分钟 | 两端启用 NTP，比较 UTC 时间 |
| collector 401 invalid_signature | secret 不一致或请求被改写 | 重新复制 secret；确认 URL 直连，不经过会改正文的代理 |
| collector 409 revision_conflict | 服务器 revision 高于本地 | collector 会 GET 当前 revision 并重试一次；持续出现时检查是否有第二个 collector |
| collector 429 | 多个写入源或异常高频 | 只保留一个 collector；等 `Retry-After`；检查代理是否让多个源共用 IP |
| viewer 显示过期 | collector 超过 15 分钟未上传 | 检查主 Windows 是否休眠、退出、断网或 Codex 登录失效 |
| Android 离线但浏览器可访问 | URL、Doze、厂商后台限制 | 用移动网络点击刷新；允许后台运行；加入电池白名单 |
| 外网不通、内网正常 | 端口转发、防火墙、DNS、CGNAT | 从手机网络测试；核对 WAN IP；检查 TCP 18080 转发 |
| 域名内网不通、外网正常 | 路由器无 NAT Loopback | 配置本地 DNS/hosts，或内网 viewer 使用局域网 URL |
| 活动为 0 | Hooks 未安装/未信任/路径失效 | 检查 `/hooks` 与 `%APPDATA%\CodexQuotaSync\activity.json` |

## 14. 为什么不改用飞书共享文档

飞书文档可以人工展示 JSON，但自动写入仍要应用凭据/OAuth，读取仍受分享权限、缓存、格式和限流影响；也无法自然提供 HMAC、revision 冲突、HTTP 状态码和原子替换。已有 UnRaid 的情况下，本容器更少依赖、更易备份和排障。

## 15. 卸载

只停容器、保留数据：

```bash
cd /mnt/user/appdata/codex-quota-sync/repo/server
docker compose down
```

确认不再使用后删除本地镜像：

```bash
docker image rm codex-quota-sync-server:local codex-quota-sync-server:rollback
```

最后才删除 `/mnt/user/appdata/codex-quota-sync`。删除前另存 `.env` 中的 secret 只在计划重装且希望沿用时有意义；全新部署可生成新 secret。
