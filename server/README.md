# Codex Quota Sync Server

这是 Codex Quota Sync 的轻量同步服务端，面向个人 UnRaid NAS：

- 只有一个 Go 进程和一个 JSON 状态文件，不需要数据库；
- `GET /v1/status` 公开读取，支持 ETag；
- `PUT /v1/status` 使用 HMAC-SHA256 防止未授权覆盖；
- 临时文件写入、`fsync` 后原子替换 `/data/state.json`；
- 容器默认以 UnRaid 常用的 `99:100`（`nobody:users`）运行；
- 只提供 HTTP。公网链路上的用量、任务数和重置时间**不会被加密**。

## 1. 最快部署：UnRaid Compose

本节适合只复制 `server` 子目录的独立部署，示例源码目录为 `source`、Host 端口为 `8787`。若使用完整仓库 clone、域名和公网端口，建议直接按更完整的 [UnRaid 部署说明](../docs/UnRaid部署说明.md) 操作；其中统一使用 `repo/server` 和 Host 端口 `18080`。

### 1.1 准备目录

1. 在 UnRaid 上创建目录：

   ```text
   /mnt/user/appdata/codex-quota-sync/source
   /mnt/user/appdata/codex-quota-sync/data
   ```

2. 将本 `server` 目录中的全部文件复制到：

   ```text
   /mnt/user/appdata/codex-quota-sync/source
   ```

3. 在 UnRaid Terminal 中确保数据目录属于 `nobody:users`：

   ```bash
   chown -R 99:100 /mnt/user/appdata/codex-quota-sync/data
   chmod 700 /mnt/user/appdata/codex-quota-sync/data
   ```

### 1.2 生成写密钥

在 UnRaid Terminal 中运行：

```bash
openssl rand -hex 32
```

保存输出的 64 位十六进制文本。它只用于 collector 写入，不需要配置到只读 Windows 或 Android 设备。不要使用示例值、短口令或 Codex token。

### 1.3 配置环境变量

在 `source` 目录中新建 `.env`，内容如下：

```dotenv
CQS_WRITE_SECRET=替换为上一步生成的随机值
CQS_HOST_PORT=8787
CQS_DATA_DIR=/mnt/user/appdata/codex-quota-sync/data
TZ=Asia/Singapore
```

- `CQS_HOST_PORT` 是 UnRaid 对外开放的 TCP 端口，可改为任意未占用端口。
- `CQS_DATA_DIR` 必须是持久化目录，不能放在容器临时文件系统中。
- `TZ` 使用 IANA 名称，例如 `Asia/Shanghai`、`Asia/Singapore`。服务端写入的 `receivedAt` 始终为 UTC，此变量主要用于日志时间。

限制 `.env` 的读取权限：

```bash
chmod 600 /mnt/user/appdata/codex-quota-sync/source/.env
```

### 1.4 构建并启动

使用 UnRaid 的 Compose Manager 插件时，把 stack 路径指向 `source/docker-compose.yml`，再执行 **Compose Up**。

也可以在 Terminal 中运行：

```bash
cd /mnt/user/appdata/codex-quota-sync/source
docker compose up -d --build
docker compose ps
docker compose logs --tail=100 codex-quota-sync
```

首次构建需要下载 Go/Alpine 基础镜像。后续容器重启直接使用本地镜像和已有状态文件。

### 1.5 验证

在局域网电脑上运行：

```bash
curl -i http://UNRAID局域网IP:8787/healthz
```

期望结果：

```http
HTTP/1.1 200 OK
Content-Type: application/json; charset=utf-8

{"status":"ok"}
```

collector 第一次成功上传前，状态接口返回 `404` 是正常行为：

```bash
curl -i http://UNRAID局域网IP:8787/v1/status
```

collector 上传后应返回 `200` 和最新 JSON。再次请求时可以携带上一次的 ETag：

```bash
curl -i -H 'If-None-Match: "上次响应中的ETag"' http://UNRAID局域网IP:8787/v1/status
```

内容未变化时返回 `304 Not Modified`，不传输 JSON 正文。

## 2. UnRaid Docker 模板部署

仓库提供 [`unraid/codex-quota-sync.xml`](unraid/codex-quota-sync.xml) 草案。模板引用的是本地镜像 `codex-quota-sync-server:local`，因此先在 Terminal 中构建镜像：

```bash
cd /mnt/user/appdata/codex-quota-sync/source
docker build -t codex-quota-sync-server:local .
```

然后：

1. 将 XML 复制到 `/boot/config/plugins/dockerMan/templates-user/`；
2. 打开 UnRaid **Docker → Add Container**；
3. 从 Template 列表选择 **Codex Quota Sync Server**；
4. 设置 Host Web Port、`/data` 路径、写密钥和时区；
5. 保持内部 `PORT=8787`、`DATA_FILE=/data/state.json`；
6. 应用并使用 `/healthz` 验证。

模板只是草案，不依赖尚未发布的公共镜像。若后续项目发布 GHCR 镜像，可将 Repository 换为实际镜像地址并省略本地构建。

## 3. 公网端口配置

完成局域网验证后，再配置公网访问：

1. 在路由器把一个公网 TCP 端口转发到 `UnRaid_IP:CQS_HOST_PORT`；
2. 若有域名，为域名添加指向公网 IP 的 A/AAAA 记录；动态公网 IP 则配置 DDNS；
3. 在 Windows viewer/Android 中填写 `http://域名:公网端口`；
4. 用手机关闭 Wi-Fi 后访问 `/healthz`，确认测试流量确实走公网；
5. 只开放容器端口，不要把 UnRaid 管理界面或 Docker socket 暴露到公网。

本服务按需求不提供 HTTPS。网络运营商、公共 Wi-Fi、路由路径上的设备都可能看到同步正文，也可能篡改公开 GET 响应；HMAC 只保护写入端点，**不会加密传输，也不会认证读取响应**。若未来同步内容扩大或需要抵抗中间人，应改为 HTTPS、VPN/Tailscale 或反向代理。

## 4. 服务配置

| 环境变量 | 默认值 | 必填 | 说明 |
|---|---:|---:|---|
| `PORT` | `8787` | 否 | 容器内 HTTP 监听端口，范围 1–65535 |
| `DATA_FILE` | `/data/state.json` | 否 | 最新状态 JSON 的持久化路径 |
| `CQS_WRITE_SECRET` | 无 | **是** | HMAC 共享写密钥；为空时服务拒绝启动 |
| `TZ` | `UTC` | 否 | IANA 时区；无效时服务拒绝启动 |

HTTP 服务设置了读取、写入、空闲和请求头超时。PUT 请求体最大为 16 KiB，并对每个直接连接 IP 采用每分钟 30 次的固定窗口限流。服务不信任 `X-Forwarded-For`；若将来增加反向代理，默认看到的是代理 IP，所有写入会共享同一限流桶。

## 5. HTTP API

### `GET /healthz`

存活检查。成功返回 `200`：

```json
{"status":"ok"}
```

它只表示进程能够处理 HTTP 请求，不返回用量，也不检查 collector 是否新鲜。

### `GET /v1/status`

- 未收到任何快照：`404 Not Found`；
- 有状态：`200 OK`，返回符合 `schema/status-v1.schema.json` 的 JSON；
- 请求的 `If-None-Match` 命中当前 ETag：`304 Not Modified`；
- 公开读取，不需要 secret；
- 响应头使用 `Cache-Control: no-cache`，允许缓存但要求复验。

### `PUT /v1/status`

请求体必须是符合 `schema/status-v1.schema.json` 的 UTF-8 JSON，且最多 16,384 字节。服务端会忽略客户端给出的 `receivedAt` 值并用接收时刻覆盖它。

必须提供且只能提供各一个请求头：

```http
X-CQS-Timestamp: 1784109600
X-CQS-Signature: v1=<64位小写或大写十六进制HMAC>
```

`X-CQS-Timestamp` 是十进制 Unix 秒：不允许前导 `+`、前导零或空格，必须在服务器当前时间前后 300 秒内。NAS 和主 Windows 应启用 NTP 自动对时。

签名算法如下：

1. 对**实际发送的原始请求体字节**计算 SHA-256，表示为小写十六进制；
2. 使用下面的精确字符串，换行是单个 LF，末尾没有换行：

   ```text
   PUT
   /v1/status
   <X-CQS-Timestamp 原文>
   <SHA256(body) 小写十六进制>
   ```

3. 以 `CQS_WRITE_SECRET` 的 UTF-8 字节作为 HMAC key，计算 HMAC-SHA256；
4. 十六进制编码后在前面加 `v1=`，写入 `X-CQS-Signature`。

即：

```text
signature = "v1=" + HEX(HMAC-SHA256(secret, "PUT\n/v1/status\n" + timestamp + "\n" + HEX(SHA256(body))))
```

状态的 `revision` 必须严格单调递增。收到小于或等于当前 revision 的请求会返回 `409 Conflict`，不会覆盖文件；collector 重试一次相同请求时，应先生成更大的 revision。

成功时返回 `200 OK`、服务器最终保存的 JSON 和对应 ETag。常见错误：

| HTTP | error.code | 含义 |
|---:|---|---|
| 400 | `invalid_status` | JSON 或 schema 字段无效 |
| 401 | `invalid_auth` / `invalid_timestamp` / `invalid_signature` | 请求头、时间或 HMAC 无效 |
| 404 | `status_not_found` | 尚无快照（仅 GET） |
| 409 | `revision_conflict` | revision 没有严格递增 |
| 413 | `body_too_large` | 请求体超过 16 KiB |
| 429 | `rate_limited` | 单个来源 IP 写入过多；参考 `Retry-After` |
| 500 | `storage_failed` | NAS 数据目录不可写或存储故障 |

错误正文统一为：

```json
{
  "error": {
    "code": "invalid_signature",
    "message": "HMAC 签名无效"
  }
}
```

服务日志不会记录 secret、签名、请求正文或 Codex 信息。

## 6. 状态文件、备份和恢复

服务只持有“最新一份”状态：内存中一份，`DATA_FILE` 一份。PUT 流程为：

1. 在同目录创建权限为 `0600` 的临时文件；
2. 写入完整 JSON 并执行文件 `fsync`；
3. 原子替换 `state.json`；
4. 同步目录元数据后更新内存状态。

启动时会读取并严格校验已有状态。文件损坏、超过 16 KiB 或不符合 schema 时，服务会拒绝启动，避免悄悄返回错误数据。

备份时复制 `/mnt/user/appdata/codex-quota-sync/data/state.json` 即可。它只是可重新采集的最新快照，通常无需专门备份。恢复时停止容器、放回文件并保持所有者 `99:100`、权限 `0600`，再启动容器。

若确认文件损坏且不需要保留快照：

```bash
docker compose down
mv /mnt/user/appdata/codex-quota-sync/data/state.json /mnt/user/appdata/codex-quota-sync/data/state.json.bad
docker compose up -d
```

服务会恢复为“尚无快照”的 404 状态，等待 collector 下一次上传。不要在容器运行时手工编辑状态文件，因为内存副本不会自动重载。

## 7. 更新与卸载

更新源码后：

```bash
cd /mnt/user/appdata/codex-quota-sync/source
docker compose build --pull
docker compose up -d
docker image prune
```

`docker compose up -d` 会优雅停止旧容器；服务最多等待 10 秒完成在途请求。`/data` 是 bind mount，重建容器不会删除状态。

卸载容器但保留数据：

```bash
docker compose down
```

确认不再需要历史快照和 secret 后，才手工删除 `source` 与 `data` 目录。

## 8. 故障排查

### 容器反复重启并提示 `必须设置 CQS_WRITE_SECRET`

检查 `.env` 是否与 `docker-compose.yml` 同目录，变量名是否准确，Compose Manager 是否加载该环境文件。不要把 secret 写进公开日志或截图。

### 提示 `permission denied` 或 `storage_failed`

```bash
chown -R 99:100 /mnt/user/appdata/codex-quota-sync/data
chmod 700 /mnt/user/appdata/codex-quota-sync/data
docker compose restart codex-quota-sync
```

### collector 收到 `invalid_timestamp`

确认 UnRaid 和 Windows 都启用了自动时间同步；`date -u` 与 Windows `Get-Date -AsUTC` 应接近。签名必须使用同一个时间戳文本，不能签名后再重新生成头。

### collector 收到 `invalid_signature`

依次检查：两端 secret 完全一致；body 在签名后没有重新格式化；body hash 是小写十六进制；签名字符串路径固定为 `/v1/status`；换行使用 LF；HMAC 结果有 `v1=` 前缀。

### GET 始终是 `404`

这表示还没有成功 PUT。查看 collector 状态和服务日志，重点排查服务器 URL、时间同步、secret 与 revision。健康检查成功不代表已有快照。

### 公网无法访问但局域网正常

检查路由器端口转发、UnRaid 防火墙、运营商是否提供真实公网 IP、域名解析和 CGNAT。若处于 CGNAT，需要向运营商申请公网地址，或改用 Tailscale/Cloudflare Tunnel 等方案。

## 9. 本地开发和测试

需要 Go 1.22 或更高版本。在 Windows PowerShell 中：

```powershell
Set-Location '<仓库根目录>\server'
gofmt -w (Get-ChildItem -Filter '*.go' | ForEach-Object FullName)
go test ./...
go test -race ./...
go vet ./...
```

构建本机二进制：

```powershell
go build -trimpath -o codex-quota-sync-server.exe .
```

本机运行时必须设置 secret；若不希望写 `/data`，同时覆盖数据文件：

```powershell
$env:CQS_WRITE_SECRET = '仅用于本地测试的随机值'
$env:DATA_FILE = '.\data\state.json'
$env:PORT = '8787'
go run .
```
