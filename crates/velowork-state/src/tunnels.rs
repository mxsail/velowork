use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

pub type SessionId = String;
pub type TunnelId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelKind {
    /// 本地端口转发 (-L): 将 local_bind 流量通过 SSH 转发至 remote_target
    Local {
        local_bind: SocketAddr,
        remote_target: String,
    },
    /// 远程端口转发 (-R): 将 SSH 服务器上的 remote_bind 流量回传至 local_target
    Remote {
        remote_bind: String,
        local_target: SocketAddr,
    },
    /// 动态 SOCKS5 代理 (-D): 本地启动 SOCKS5 监听，根据握手动态开辟 direct-tcpip Channel
    Dynamic {
        local_bind: SocketAddr,
    },
}

impl TunnelKind {
    pub fn local_bind_addr(&self) -> Option<SocketAddr> {
        match self {
            TunnelKind::Local { local_bind, .. } => Some(*local_bind),
            TunnelKind::Dynamic { local_bind } => Some(*local_bind),
            TunnelKind::Remote { .. } => None,
        }
    }

    pub fn display_summary(&self) -> String {
        match self {
            TunnelKind::Local { local_bind, remote_target } => {
                format!("{} ➔ {}", local_bind, remote_target)
            }
            TunnelKind::Remote { remote_bind, local_target } => {
                format!("{} ➔ {}", remote_bind, local_target)
            }
            TunnelKind::Dynamic { local_bind } => {
                format!("SOCKS5 {}", local_bind)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ReconnectPolicy {
    Never,
    #[default]
    InheritSession,
    Always,
}

/// 一等资源：TunnelProfile
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelProfile {
    pub id: TunnelId,
    pub name: String,
    #[serde(default)]
    pub project_id: Option<String>,
    pub session_id: SessionId,
    pub enabled: bool,
    pub auto_start: bool,
    pub reconnect: ReconnectPolicy,
    pub kind: TunnelKind,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelStatus {
    Stopped,
    Running,
    Reconnecting { attempt: u32, backoff_secs: u64 },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelRuntimeInfo {
    pub status: TunnelStatus,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub active_connections: usize,
    #[serde(default)]
    pub bound_port: Option<u16>,
}

impl Default for TunnelRuntimeInfo {
    fn default() -> Self {
        Self {
            status: TunnelStatus::Stopped,
            rx_bytes: 0,
            tx_bytes: 0,
            active_connections: 0,
            bound_port: None,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// 树形结构：文件夹节点 + 隧道节点（支持无限层级嵌套）
// ───────────────────────────────────────────────────────────────────────────

/// 隧道树节点：既可以是文件夹（含子节点），也可以是叶子隧道。
///
/// 与快捷指令 (`QuickCommandNode`) 同构，便于在面板中复用通用树组件与
/// 文件夹辅助函数。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelNode {
    Folder {
        id: String,
        name: String,
        #[serde(default)]
        project_id: Option<String>,
        expanded: bool,
        children: Vec<TunnelNode>,
    },
    Tunnel {
        profile: TunnelProfile,
    },
}

/// 隧道树的根（顶层节点列表）。
pub type TunnelTree = Vec<TunnelNode>;

impl TunnelNode {
    pub fn id(&self) -> &str {
        match self {
            TunnelNode::Folder { id, .. } => id,
            TunnelNode::Tunnel { profile } => &profile.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            TunnelNode::Folder { name, .. } => name,
            TunnelNode::Tunnel { profile } => &profile.name,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, TunnelNode::Folder { .. })
    }

    pub fn project_id(&self) -> Option<&str> {
        match self {
            TunnelNode::Folder { project_id, .. } => project_id.as_deref(),
            TunnelNode::Tunnel { profile } => profile.project_id.as_deref(),
        }
    }

    /// 若为隧道叶子节点，返回其 `TunnelProfile` 引用。
    pub fn profile(&self) -> Option<&TunnelProfile> {
        match self {
            TunnelNode::Tunnel { profile } => Some(profile),
            TunnelNode::Folder { .. } => None,
        }
    }
}

/// 递归收集某节点子树中所有的隧道 profile（含自身若为隧道）。
pub fn collect_tunnel_profiles(node: &TunnelNode, out: &mut Vec<TunnelProfile>) {
    match node {
        TunnelNode::Tunnel { profile } => out.push(profile.clone()),
        TunnelNode::Folder { children, .. } => {
            for c in children {
                collect_tunnel_profiles(c, out);
            }
        }
    }
}
