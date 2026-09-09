use serde::{Deserialize, Serialize};

/// 服务唯一 ID 类型别名。
pub type ServiceId = String;

/// 服务类型枚举。
///
/// 枚举携带生成命令所需的数据；UI 只选择类型与填单元/容器名，
/// 真正的 shell 命令由 `ServiceKind::build_*_command` 在内部生成，
/// UI 不感知 systemd / docker 细节。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ServiceKind {
    /// 自定义命令：alive/start/stop/restart 全部由用户在 ServiceDefinition 中填写。
    #[default]
    Command,
    /// systemd 单元，例如 `nginx`、`mysql`。
    Systemd {
        unit: String,
    },
    /// Docker 容器，例如 `web`、`redis`。
    Docker {
        container: String,
    },
}

impl ServiceKind {
    /// 生成存活探活命令。Command 类型使用用户提供的 alive_command（此处返回 None，由上层回退）。
    pub fn build_alive_command(&self) -> Option<String> {
        match self {
            ServiceKind::Command => None,
            ServiceKind::Systemd { unit } => Some(format!("systemctl is-active {}", unit)),
            ServiceKind::Docker { container } => {
                Some(format!(
                    "[ \"$(docker inspect -f '{{{{.State.Running}}}}' {} 2>/dev/null)\" = \"true\" ]",
                    container
                ))
            }
        }
    }

    /// 生成启动命令。
    pub fn build_start_command(&self) -> Option<String> {
        match self {
            ServiceKind::Command => None,
            ServiceKind::Systemd { unit } => Some(format!("systemctl start {}", unit)),
            ServiceKind::Docker { container } => Some(format!("docker start {}", container)),
        }
    }

    /// 生成停止命令。
    pub fn build_stop_command(&self) -> Option<String> {
        match self {
            ServiceKind::Command => None,
            ServiceKind::Systemd { unit } => Some(format!("systemctl stop {}", unit)),
            ServiceKind::Docker { container } => Some(format!("docker stop {}", container)),
        }
    }

    /// 生成重启命令。
    pub fn build_restart_command(&self) -> Option<String> {
        match self {
            ServiceKind::Command => None,
            ServiceKind::Systemd { unit } => Some(format!("systemctl restart {}", unit)),
            ServiceKind::Docker { container } => Some(format!("docker restart {}", container)),
        }
    }
}

/// 命令执行确认策略（仅影响启停/重启等写操作，不影响探活）。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ServiceCommandPolicy {
    /// 任何启停/重启都弹确认框。
    #[default]
    AlwaysConfirm,
    /// 仅对 stop / restart 等危险操作弹确认框，start 直接执行。
    ConfirmDangerous,
    /// 直接执行，不弹确认框。
    Direct,
}

impl ServiceCommandPolicy {
    /// 给定操作类型，是否需要在执行前确认。
    pub fn requires_confirm(&self, op: ServiceOp) -> bool {
        match self {
            ServiceCommandPolicy::AlwaysConfirm => true,
            ServiceCommandPolicy::Direct => false,
            ServiceCommandPolicy::ConfirmDangerous => matches!(op, ServiceOp::Stop | ServiceOp::Restart),
        }
    }
}

/// 受确认策略约束的服务操作类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceOp {
    Start,
    Stop,
    Restart,
}

/// 服务采集/执行后的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceStatus {
    /// 尚未采集（无监控/未开启/刚切换）。
    NotChecked,
    /// 运行中。
    Running,
    /// 已停止。
    Stopped,
    /// 正在启动（start 已下发，等待下次探活确认）。
    Starting,
    /// 正在停止（stop 已下发，等待下次探活确认）。
    Stopping,
    /// 状态无法判定。
    Unknown,
    /// 连接失败（SSH 会话不可用）。
    ConnectionFailed,
    /// 权限被拒绝。
    PermissionDenied,
    /// 采集超时。
    Timeout,
}

impl ServiceStatus {
    /// 是否为过渡态（启动/停止中）。
    pub fn is_transition(&self) -> bool {
        matches!(self, ServiceStatus::Starting | ServiceStatus::Stopping)
    }

    /// 是否为错误态。
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            ServiceStatus::ConnectionFailed | ServiceStatus::PermissionDenied | ServiceStatus::Timeout
        )
    }
}

/// 服务监控配置项（持久化）。
///
/// `session_id` 沿用现有命名，语义等价于 `SshSession.id`（远端服务器身份）：
/// - `Some(id)`：绑定到固定服务器的服务，状态栏监控面板仅在当前激活终端属于该会话时显示；
/// - `None`：未绑定服务器的全局快捷服务，仅可从 Dock 服务监控面板「发送到当前终端」执行。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ServiceDefinition {
    /// 稳定唯一 ID。
    pub id: String,
    /// 用户显示名称。
    pub name: String,
    /// 所属项目 ID。
    #[serde(default)]
    pub project_id: Option<String>,
    /// 服务类型（决定命令生成方式）。
    #[serde(default)]
    pub kind: ServiceKind,
    /// 所属服务器会话：Some(SshSession.id) 固定服务器；None 当前终端执行服务。
    #[serde(default)]
    pub session_id: Option<String>,
    /// 存活状态命令（Command 类型使用；systemd/docker 由 ServiceKind 生成）。
    #[serde(default)]
    pub alive_command: String,
    /// 启动命令（留空由 ServiceKind 生成）。
    #[serde(default)]
    pub start_command: Option<String>,
    /// 关闭命令（留空由 ServiceKind 生成）。
    #[serde(default)]
    pub stop_command: Option<String>,
    /// 重启命令（留空由 ServiceKind 生成）。
    #[serde(default)]
    pub restart_command: Option<String>,
    /// 工作目录（可选，执行命令前 cd 进入）。
    #[serde(default)]
    pub workdir: Option<String>,
    /// 是否开启状态栏监控。
    #[serde(default)]
    pub monitor_enabled: bool,
    /// 启停/重启命令确认策略。
    #[serde(default)]
    pub command_policy: ServiceCommandPolicy,
}

impl Default for ServiceDefinition {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            project_id: None,
            kind: ServiceKind::Command,
            session_id: None,
            alive_command: String::new(),
            start_command: None,
            stop_command: None,
            restart_command: None,
            workdir: None,
            monitor_enabled: false,
            command_policy: ServiceCommandPolicy::AlwaysConfirm,
        }
    }
}

impl ServiceDefinition {
    /// 解析后用于探活的命令（优先用户命令，否则由 ServiceKind 生成）。
    pub fn effective_alive_command(&self) -> Option<String> {
        if !self.alive_command.trim().is_empty() {
            Some(self.alive_command.clone())
        } else {
            self.kind.build_alive_command()
        }
    }

    /// 解析后用于启动的命令。
    pub fn effective_start_command(&self) -> Option<String> {
        self.start_command
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| self.kind.build_start_command())
    }

    /// 解析后用于停止的命令。
    pub fn effective_stop_command(&self) -> Option<String> {
        self.stop_command
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| self.kind.build_stop_command())
    }

    /// 解析后用于重启的命令。
    pub fn effective_restart_command(&self) -> Option<String> {
        self.restart_command
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| self.kind.build_restart_command())
    }
}

/// 服务运行态（仅内存，由采集引擎维护，不持久化）。
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceRuntimeState {
    pub service_id: String,
    pub session_id: String,
    pub status: ServiceStatus,
    /// 上次采集时间戳（秒）。
    pub last_check: i64,
    pub message: Option<String>,
    /// 单调递增版本号，用于防止旧采集结果覆盖新结果。
    pub revision: u64,
}

/// 服务树节点（文件夹或服务）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServiceNode {
    Folder {
        id: String,
        name: String,
        #[serde(default)]
        project_id: Option<String>,
        expanded: bool,
        children: Vec<ServiceNode>,
    },
    Service {
        def: ServiceDefinition,
    },
}

/// 服务树的根（顶层节点列表）。
pub type ServiceTree = Vec<ServiceNode>;

impl ServiceNode {
    pub fn id(&self) -> &str {
        match self {
            ServiceNode::Folder { id, .. } => id,
            ServiceNode::Service { def } => &def.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            ServiceNode::Folder { name, .. } => name,
            ServiceNode::Service { def } => &def.name,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, ServiceNode::Folder { .. })
    }

    pub fn project_id(&self) -> Option<&str> {
        match self {
            ServiceNode::Folder { project_id, .. } => project_id.as_deref(),
            ServiceNode::Service { def } => def.project_id.as_deref(),
        }
    }

    pub fn def(&self) -> Option<&ServiceDefinition> {
        match self {
            ServiceNode::Folder { .. } => None,
            ServiceNode::Service { def } => Some(def),
        }
    }
}

/// 递归收集服务树中的所有目录（Folder）节点，返回 `(目录id, 带平铺路径的显示名)`。
pub fn collect_folder_nodes(
    nodes: &[ServiceNode],
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    for node in nodes {
        if let ServiceNode::Folder {
            id, name, children, ..
        } = node
        {
            let display = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            out.push((id.clone(), display.clone()));
            collect_folder_nodes(children, &display, out);
        }
    }
}
