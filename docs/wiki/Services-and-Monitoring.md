# 服务管理与实时监控实操手册 (Services & Monitoring)

在多服务器运维中，工程师往往需要同时掌握底层硬件负载与关键业务进程的存活状态。Velowork 提供了**系统硬件实时仪表盘**与**图形化服务监控面板**（快捷键 `Cmd+Shift+S` / `Ctrl+Shift+S`），让您无需不断手动执行 `top`、`htop`、`systemctl status` 或 `docker ps`。

---

## 1. 实时硬件状态栏 (Hardware Status Bar)

位于应用最底部的状态栏提供毫秒级刷新的硬件指标：

### 1.1 关键监控维度
- **CPU 使用率**：多核心平均综合利用率与动态波形走势；
- **内存 (RAM)**：已用物理内存、空闲内存及总容量占比；
- **磁盘 I/O**：当前系统的实时读写速率（KB/s、MB/s）；
- **网络吞吐 (Network)**：上行上传与下行下载瞬时带宽；
- **多协议连接指示**：展示当前活跃的 SSH、Serial、Telnet 连接数量与健康指标。

### 1.2 状态栏自动隐藏模式 (Auto-Hide Mode)
为了在小屏幕（如 13-14 寸笔记本）上给终端视口预留最大竖向空间，Velowork 引入了状态栏自适应隐藏特性：
- 进入 **设置 -> 外观 -> 状态栏模式 (Status Bar Mode)**；
- 可选择：
  - **常驻 (Always)**：固定显示；
  - **自动隐藏 (AutoHide)**：默认自动收起隐藏，仅在鼠标悬停至窗口最底部边缘时平滑浮出，高度严格约束为标准的 `32px` 紧凑尺寸，杜绝布局碰撞；
  - **关闭 (Never)**：彻底隐藏状态栏。

---

## 2. 后台守护服务监控实操 (Services Panel)

按快捷键 `Cmd+Shift+S` (macOS) / `Ctrl+Shift+S` (Linux/Windows) 打开右侧 **服务面板**。该面板支持监控三种核心服务类型：

### 2.1 Systemd 系统服务监控 (Systemd Units)
适用于 Linux 服务器上的 `nginx`、`mysql`、`redis`、`sshd` 等系统守护进程：
1. 点击面板顶部的“添加服务”；
2. **服务类型**：选择 **Systemd**；
3. **关联主机**：选取目标 SSH 会话；
4. **服务单元名称 (Unit Name)**：填入单元全名（如 `nginx.service` 或 `docker.service`）；
5. **探活周期 (Interval)**：设定检查间隔（如 10 秒）；
6. 保存后，系统将自动定期向远端查询 `systemctl is-active <unit>` 状态；
7. **界面直接管控**：
   - 绿色圆点表示 `active (running)`；
   - 鼠标悬停列表项，右侧直接提供 **启动 (Start)**、**停止 (Stop)**、**重启 (Restart)**、**重载 (Reload)** 按钮，点击一键下发指令，无需在终端手敲命令。

### 2.2 Docker 容器监控 (Docker Containers)
适用于微服务与云原生容器集群：
1. 服务类型选择 **Docker**；
2. 填入待监控的容器名称或容器 ID（如 `api-server-prod`）；
3. 系统自动同步容器的运行态（`running`、`restarting`、`exited`、`paused`）与映射端口；
4. 提供一键 `docker restart` 与查看容器最近日志输出功能。

### 2.3 自定义命令服务 (Custom Command Scripts)
适用于通过自定义脚本拉起的私有进程、Python 脚本或 Spring Boot 应用：
1. 服务类型选择 **自定义命令 (Custom)**；
2. **探活脚本 (Health Check Script)**：
   ```bash
   curl -s -f http://127.0.0.1:8080/actuator/health || exit 1
   ```
3. **状态判断规则**：可配置基于退出码（Exit Code 0 为正常）或基于返回文本的正则表达式匹配（如包含 `"status":"UP"`）；
4. **启停脚本 (Start/Stop Commands)**：
   - 启动命令：`nohup /opt/app/start.sh > /dev/null 2>&1 &`
   - 停止命令：`/opt/app/stop.sh`
5. 配置完成后，服务面板即成为该私有应用的图形化启停控制器。

---

## 3. 告警指示与状态联动 (Alerts & Health Indicators)

- **健康状态颜色规范**：
  - 🟢 **健康 (Healthy)**：正常运行且响应达标；
  - 🟡 **警告 (Warning)**：资源占用过高或响应延迟增大；
  - 🔴 **异常 (Down/Failed)**：进程崩溃退出或探活超时；
- **右键查看详细日志**：
  在服务树项上右键选择“查看服务日志”，系统会自动在中央工作区开启一个专用终端标签页，并自动执行 `journalctl -u <unit> -f -n 200` 或 `docker logs -f --tail 200`，快速进入排错状态。
