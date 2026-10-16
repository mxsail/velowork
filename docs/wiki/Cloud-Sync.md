# 云端多端同步实操手册 (Cloud Synchronization & Multi-Device Backup)

现代研发与运维经常需要在办公台式机、移动笔记本与家庭工作站之间切换。Velowork 提供了**端到端强加密的云同步引擎**，支持 **S3 兼容对象存储** 与 **WebDAV 网盘** 协议，让您的会话资产、快捷指令、外观主题与自定义配置在多设备间保持毫秒级同步。

---

## 1. 支持的同步后端协议 (Storage Providers)

| 协议类型 | 推荐服务商 / 方案 | 适用场景 |
| :--- | :--- | :--- |
| **S3 兼容对象存储** | **AWS S3**、**MinIO**（私有化部署）、**Cloudflare R2**、阿里云 OSS、腾讯云 COS | 推荐方案。高可用、大吞吐、支持私网私有云部署，企业与极客首选 |
| **WebDAV 网盘** | **坚果云 (Nutstore)**、**Nextcloud**、ownCloud、群晖 NAS (Synology WebDAV) | 个人日常多端同步，配置门槛低 |

---

## 2. S3 兼容对象存储详细配置实操 (S3 Configuration)

进入 **设置 -> 同步 (Sync Settings)**，将同步服务商切换为 **S3**：

### 2.1 核心配置参数说明
- **服务地址 (Endpoint)**：
  - AWS S3: `https://s3.amazonaws.com`
  - Cloudflare R2: `https://<account-id>.r2.cloudflarestorage.com`
  - 自建 MinIO 私有云: `http://192.168.1.100:9000` 或域名 `https://minio.example.com`
- **存储桶 (Bucket)**：填入预先在云端创建好的 Bucket 名称（如 `my-velowork-sync`）；
- **存储区域 (Region)**：如 `us-east-1`、`ap-northeast-1`（MinIO 等私有存储通常保持默认即可）；
- **访问密钥 (Access Key ID)** 与 **私有密钥 (Secret Access Key)**：
  - 填入具有该 Bucket 读写权限的凭据；
  - **安全保障**：Secret Key 在保存后会立即送入本地安全密钥库（Keyring）存储，JSON 配置文件中仅记录存储状态标记，绝不落盘明文；
- **存储前缀路径 (Prefix)**：默认为 `velowork/`，便于与其他文件隔离；
- **路径样式 (Force Path-Style)**：
  - 传统虚拟主机样式为 `http://bucket.domain.com/...`；而内网自建 MinIO 或基于 IP 的端点必须使用路径样式 `http://domain.com/bucket/...`；
  - **智能自愈**：Velowork 会自动检测 IP 地址格式的服务端点，并自动开启 Path-Style 支持，彻底消除连通性故障。

### 2.2 测试连接与故障排查 (Test Connection)
- 点击 **测试连接** 按钮，系统会向目标端点尝试写入与读取临时探活包；
- 若配置有误，系统会自动提取底层精简的 XML 错误核心原因（如 Bucket 不存在、签名验证失败或网络不可达），详细调用栈安全下沉至后台日志；
- 测试过程中支持随时点击“取消”中止探测。

---

## 3. WebDAV 网盘同步配置实操 (WebDAV Configuration)

若选择 WebDAV 作为同步载体（如坚果云）：
1. 在 **设置 -> 同步** 中选择 **WebDAV**；
2. **服务器地址 (Server URL)**：如 `https://dav.jianguoyun.com/dav/`；
3. **账号 (Username)**：坚果云注册邮箱；
4. **应用密码 (Application Password)**：请在坚果云网页版“安全设置”中生成专门的 WebDAV 应用密码（切勿填写真实登录密码）；
5. **远端同步路径 (Sync Path)**：默认为 `/velowork/`；
6. 保存并测试连接。

---

## 4. 细粒度同步数据范围选择 (`SyncDataScope`)

与许多“要么不传、要么全传”的工具不同，Velowork 支持针对具体业务数据类型进行精确的勾选开关：

| 数据范围选项 | 涵盖内容 | 推荐设置 |
| :--- | :--- | :--- |
| **SSH 会话与服务器节点 (Sessions)** | 服务器主机、端口、用户名、分组层级 | ✅ 启用 |
| **端口转发与隧道 (Tunnels)** | 本地/远程/动态隧道映射规则 | ✅ 启用 |
| **服务监控 (Services)** | Systemd、Docker 监控单元配置 | ✅ 启用 |
| **快捷指令库 (Quick Commands)** | 运维命令、带动态占位符的代码片段 | ✅ 启用 |
| **AI 对话与消息记录 (AI Chat)** | 与大模型的历史排障对话上下文 | 可选（视个人隐私需求） |
| **终端命令历史 (Command History)** | 终端中执行过的历史命令列表 | 可选 |
| **应用偏好与全局配置 (Settings)** | 字体大小、快捷键、分屏偏好、标题栏设置 | ✅ 启用 |
| **自定义主题 (Themes)** | 本地自定义编写的 JSON 配色方案 | ✅ 启用 |
| **账号与连接凭据 (Credentials)** | 加密后的连接密码与私钥密钥包 | ✅ 启用（受主密码加解密保护） |

---

## 5. 端到端加密与安全冲突策略 (Security & Conflict Resolution)

### 5.1 AES-256-GCM 端到端加密
- 上传到云端（S3 或 WebDAV）的同步数据包**在离开本机前**，均由您设置的**主密码 (Master Password)** 通过 PBKDF2 / Argon2 派生出的根密钥进行全量 AES-256-GCM 加密；
- 即使您的 S3 Bucket 或 WebDAV 账号遭受入侵，攻击者获取到的也仅是高强度密文碎片，绝不可能还原出任何一条服务器密码或私钥。

### 5.2 自动同步与冲突策略
- **启动时自动同步**：每次启动 Velowork，系统静默对比本地与云端的时间戳版本，自动拉取最新配置；
- **定时后台同步**：可自定义同步周期（默认 1800 秒 / 30 分钟）；
- **冲突策略 (Conflict Strategy)**：当两台机器同时产生了不同修改时，系统提供“以云端为准”、“以本地为准”或“保留修改时间更新的一方”的明确处理规则。
