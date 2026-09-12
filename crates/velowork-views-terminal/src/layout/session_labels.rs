//! Shared terminal display-label data for the whole app.
//!
//! Three distinct UI surfaces need to render the same launched terminal
//! sessions with identical names *and* identical `:N` duplicate-session
//! suffixes:
//!
//! 1. the terminal **tab strip**,
//! 2. the searchable **tab-list dropdown** ("更多" / overflow),
//! 3. the command panel's **"Selected Sessions"** target host list.
//!
//! To guarantee the `:N` numbers never drift between them, the duplicate
//! counting lives here as a single source of truth. Every consumer reads the
//! same `duplicate_session_suffixes` map instead of recomputing the numbering.
//!
//! The *base* name is also unified here via `terminal_base_name`, so the tab
//! strip, the tab-list dropdown and the command panel's "Selected Sessions"
//! host list render the exact same label for every terminal (the same custom
//! name / OSC title / directory / shell name resolution, not just the
//! numbering).

use std::collections::HashMap;

use velowork_core::shell::ShellType;
use velowork_workspace::state::{LayoutNode, ProjectData, Workspace};
use velowork_workspace::stores::SessionStore;

/// Collect every terminal in the workspace layout, in a deterministic
/// (depth-first, tree-walk) order, as `(terminal_id, shell_type, project)` triples.
///
/// The deterministic order is what makes the assigned `:N` suffixes stable
/// across renders — a `HashMap`-only walk would reassign numbers on every
/// frame and make them flicker. The `project` reference is carried along so
/// the duplicate counter can group local default terminals by their base name.
fn collect_terminals(workspace: &Workspace) -> Vec<(String, ShellType, ProjectRef<'_>)> {
    let mut out = Vec::new();
    for project in workspace.projects() {
        if let Some(layout) = &project.layout {
            collect_terminals_node(layout, project, &mut out);
        }
    }
    out
}

fn collect_terminals_node<'a>(
    node: &LayoutNode,
    project: &'a ProjectData,
    out: &mut Vec<(String, ShellType, ProjectRef<'a>)>,
) {
    match node {
        LayoutNode::Terminal {
            terminal_id,
            shell_type,
            ..
        } => {
            if let Some(id) = terminal_id {
                out.push((id.clone(), shell_type.clone(), ProjectRef(project)));
            }
        }
        LayoutNode::Split { children, .. } | LayoutNode::Tabs { children, .. } => {
            for child in children {
                collect_terminals_node(child, project, out);
            }
        }
    }
}

/// Helper so we can carry a `&ProjectData` through the terminal collector
/// without fighting the borrow checker over `Vec<(.., &ProjectData)>`.
struct ProjectRef<'a>(&'a ProjectData);

/// Global duplicate-session numbering — the single source of truth.
///
/// For every SSH session (identified by its `--id` / `--session-id` flag in the
/// `ssh` shell args) that is launched more than once, assign a 1-based position
/// to each of its terminal instances in deterministic workspace order. Only the
/// 2nd and later occurrences get an entry, so callers render
/// `名称`, `名称:2`, `名称:3`, ... identically everywhere.
///
/// The same numbering is now applied to **local default terminals**: when the
/// same local shell (e.g. two `zsh` tabs) or two local terminals sharing the
/// same custom name are opened, the 2nd and later ones get `:2`, `:3`, ... so
/// they stay distinguishable — matching the long-standing SSH behaviour.
///
/// Returns `terminal_id -> suffix` (absent for the first occurrence and for all
/// non-duplicated sessions).
pub fn duplicate_session_suffixes(workspace: &Workspace) -> HashMap<String, usize> {
    let terminals = collect_terminals(workspace);

    // Group key: custom user-renamed terminals by their custom name;
    // saved sessions (SSH/Serial/Telnet/Local) by their session id;
    // local terminals by their local shell name.
    let group_key = |tid: &str, st: &ShellType, project: &ProjectData| -> Option<String> {
        if let Some(name) = project.terminal_names.get(tid) {
            if !name.trim().is_empty() {
                return Some(format!("custom:{}", name));
            }
        }
        if let Some(sid) = st.session_id() {
            return Some(format!("session:{}", sid));
        }
        if !st.is_remote() {
            return Some(format!("local:{}", st.local_shell_name()));
        }
        None
    };

    // Count how many times each group appears.
    let mut total: HashMap<String, usize> = HashMap::new();
    for (tid, st, project) in &terminals {
        if let Some(key) = group_key(tid, st, project.0) {
            *total.entry(key).or_insert(0) += 1;
        }
    }

    // Walk again in the same order and assign 1-based positions.
    let mut position: HashMap<String, usize> = HashMap::new();
    let mut suffixes = HashMap::new();
    for (tid, st, project) in &terminals {
        if let Some(key) = group_key(tid, st, project.0) {
            let count = total[&key];
            if count > 1 {
                let pos = {
                    let e = position.entry(key).or_insert(0);
                    *e += 1;
                    *e
                };
                if pos > 1 {
                    suffixes.insert(tid.clone(), pos);
                }
            }
        }
    }
    suffixes
}

/// Resolve the user-facing name for a saved session (`ssh`, `serial`, `telnet`, `local`)
/// from its session id argument, falling back to connection details / host / port.
///
/// Returns `None` for non-session shells (e.g. Default local shell, Welcome) so callers
/// can fall back to the custom name / shell name / terminal title. Shared so the tab strip,
/// the tab-list dropdown, and the command panel never disagree on a session's base name.
pub fn session_name(shell_type: &ShellType, store: &SessionStore) -> Option<String> {
    let ShellType::Custom { path, args } = shell_type else {
        return None;
    };
    if path != "ssh" && path != "serial" && path != "telnet" && path != "local" {
        return None;
    }

    if let Some(sid) = shell_type.session_id() {
        if let Some(session) = store.find_session(sid) {
            if !session.name.is_empty() {
                return Some(session.name.clone());
            }
        }
    }

    // Fallback if session is deleted or not found in store
    match path.as_str() {
        "ssh" => {
            let mut host_arg = None;
            let mut i = 0;
            while i < args.len() {
                if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
                    i += 2;
                } else if (args[i] == "-p" || args[i] == "-i") && i + 1 < args.len() {
                    i += 2;
                } else if !args[i].starts_with('-') {
                    host_arg = Some(args[i].clone());
                    i += 1;
                } else {
                    i += 1;
                }
            }
            host_arg
        }
        "serial" => {
            let mut port_arg = None;
            let mut i = 0;
            while i < args.len() {
                if args[i] == "--port" && i + 1 < args.len() {
                    port_arg = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            port_arg.or_else(|| Some("Serial".to_string()))
        }
        "telnet" => {
            let mut host_arg = None;
            let mut port_arg = None;
            let mut i = 0;
            while i < args.len() {
                if args[i] == "--host" && i + 1 < args.len() {
                    host_arg = Some(args[i + 1].clone());
                    i += 2;
                } else if args[i] == "--port" && i + 1 < args.len() {
                    port_arg = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            host_arg
                .map(|h| format!("{}:{}", h, port_arg.unwrap_or_else(|| "23".to_string())))
                .or_else(|| Some("Telnet".to_string()))
        }
        "local" => Some("Local".to_string()),
        _ => None,
    }
}

/// Backward-compatible alias for [`session_name`].
pub fn ssh_session_name(shell_type: &ShellType, store: &SessionStore) -> Option<String> {
    session_name(shell_type, store)
}

/// Canonical *base* display name for a single terminal — the single source of
/// truth shared by the tab strip, the tab-list dropdown and the command panel's
/// "Selected Sessions" host list. The `:N` duplicate suffix is layered on top
/// by the callers via `duplicate_session_suffixes`.
///
/// Priority:
///
/// 1. User explicitly renamed the tab via right-click (`terminal_names`).
/// 2. Saved session (`ssh`, `serial`, `telnet`, `local`) → saved session name.
/// 3. Local terminal (non-remote shell on a non-remote backend) → local shell name.
/// 4. Remote backend (whole terminal connected to a host) →
///    custom name > non-prompt OSC title > directory name.
pub fn terminal_base_name(
    terminal_id: &str,
    shell_type: &ShellType,
    backend_is_remote: bool,
    osc_title: Option<&str>,
    project: &ProjectData,
    store: &SessionStore,
) -> String {
    // 1. User explicitly renamed the tab via right-click → highest precedence.
    if let Some(custom_name) = project.terminal_names.get(terminal_id) {
        if !custom_name.trim().is_empty() {
            return custom_name.clone();
        }
    }
    // 2. Saved session (SSH, Serial, Telnet, Local) → saved session name.
    if let Some(name) = session_name(shell_type, store) {
        return name;
    }
    // 3. Local terminal → local shell name.
    if !shell_type.is_remote() && !backend_is_remote {
        return shell_type.local_shell_name();
    }
    // 4. Remote backend → custom name > OSC title > directory name.
    project.terminal_display_name(terminal_id, osc_title.map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use velowork_state::{SessionProtocol, SessionTreeNode, SshSession};

    use velowork_state::SshSessionConfig;

    fn make_session(id: &str, name: &str, protocol: SessionProtocol) -> SshSession {
        let mut session = SshSession::default();
        session.id = id.to_string();
        session.name = name.to_string();
        session.protocol = protocol;
        session
    }

    fn make_store_with_sessions(sessions: Vec<SshSession>) -> SessionStore {
        let mut config = SshSessionConfig::default();
        for session in sessions {
            config.tree.push(SessionTreeNode::Session { session });
        }
        SessionStore::from_config(config)
    }

    fn make_project(id: &str) -> ProjectData {
        ProjectData {
            id: id.to_string(),
            name: "Test Project".to_string(),
            path: "/tmp".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_session_name_for_all_protocols() {
        let store = make_store_with_sessions(vec![
            make_session("ssh-1", "Prod SSH Server", SessionProtocol::Ssh),
            make_session("serial-1", "Router Console", SessionProtocol::Serial),
            make_session("telnet-1", "Switch Telnet", SessionProtocol::Telnet),
            make_session("local-1", "Build Environment", SessionProtocol::Local),
        ]);

        let ssh_shell = ShellType::Custom {
            path: "ssh".to_string(),
            args: vec!["--id".to_string(), "ssh-1".to_string(), "root@host".to_string()],
        };
        assert_eq!(session_name(&ssh_shell, &store), Some("Prod SSH Server".to_string()));

        let serial_shell = ShellType::Custom {
            path: "serial".to_string(),
            args: vec!["--id".to_string(), "serial-1".to_string(), "--port".to_string(), "COM3".to_string()],
        };
        assert_eq!(session_name(&serial_shell, &store), Some("Router Console".to_string()));

        let telnet_shell = ShellType::Custom {
            path: "telnet".to_string(),
            args: vec!["--id".to_string(), "telnet-1".to_string(), "--host".to_string(), "10.0.0.1".to_string()],
        };
        assert_eq!(session_name(&telnet_shell, &store), Some("Switch Telnet".to_string()));

        let local_shell = ShellType::Custom {
            path: "local".to_string(),
            args: vec!["--id".to_string(), "local-1".to_string()],
        };
        assert_eq!(session_name(&local_shell, &store), Some("Build Environment".to_string()));
    }

    #[test]
    fn test_terminal_base_name_precedence() {
        let store = make_store_with_sessions(vec![
            make_session("serial-1", "Router Console", SessionProtocol::Serial),
        ]);
        let serial_shell = ShellType::Custom {
            path: "serial".to_string(),
            args: vec!["--id".to_string(), "serial-1".to_string()],
        };

        let mut project = make_project("p1");

        // 1. Without rename, uses saved session name
        assert_eq!(
            terminal_base_name("t1", &serial_shell, false, None, &project, &store),
            "Router Console"
        );

        // 2. With explicit user rename, rename overrides saved session name
        project.terminal_names.insert("t1".to_string(), "Overridden Name".to_string());
        assert_eq!(
            terminal_base_name("t1", &serial_shell, false, None, &project, &store),
            "Overridden Name"
        );
    }
}
