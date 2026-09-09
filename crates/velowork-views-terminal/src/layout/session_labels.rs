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

    // Group key: SSH sessions by their session id; local terminals by their
    // base name (custom name if set, otherwise the local shell name). Remote
    // non-SSH shells keep their existing (un-numbered) behaviour.
    let group_key = |tid: &str, st: &ShellType, project: &ProjectData| -> Option<String> {
        if let Some(sid) = st.ssh_session_id() {
            return Some(format!("ssh:{}", sid));
        }
        if !st.is_remote() {
            if let Some(name) = project.terminal_names.get(tid) {
                return Some(format!("local:{}", name));
            }
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

/// Resolve the user-facing name for an SSH (`ssh` custom) shell from its
/// `--id` / `--session-id` flag, falling back to the raw host argument.
///
/// Returns `None` for non-SSH shells so callers can fall back to the shell
/// name / terminal title. Shared so the tab strip and the command panel never
/// disagree on a session's base name.
pub fn ssh_session_name(shell_type: &ShellType, store: &SessionStore) -> Option<String> {
    let ShellType::Custom { path, args } = shell_type else {
        return None;
    };
    if path != "ssh" {
        return None;
    }

    let mut session_id = None;
    let mut host_arg = None;
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
            session_id = Some(args[i + 1].clone());
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

    if let Some(sid) = session_id {
        if let Some(session) = store.find_session(&sid) {
            if !session.name.is_empty() {
                return Some(session.name.clone());
            }
        }
    }
    host_arg
}

/// Canonical *base* display name for a single terminal — the single source of
/// truth shared by the tab strip, the tab-list dropdown and the command panel's
/// "Selected Sessions" host list. The `:N` duplicate suffix is layered on top
/// by the callers via `duplicate_session_suffixes`.
///
/// Priority (identical to the historical tab-strip logic):
///
/// 1. `ssh` custom shell → the saved SSH session name.
/// 2. Local terminal (non-remote shell on a non-remote backend) → the user-set
///    custom terminal name, else the local shell name (`zsh`, `bash`, ...).
/// 3. Remote backend (whole terminal connected to a host) →
///    custom name > non-prompt OSC title > directory name.
///
/// `backend_is_remote` is the caller's notion of whether the terminal runs on a
/// remote host: the tab strip passes its pane's `backend.is_remote()`, while the
/// command panel passes the owning project's `is_remote` flag — both are the
/// correct analogue for their context, and the name *logic* below is identical.
pub fn terminal_base_name(
    terminal_id: &str,
    shell_type: &ShellType,
    backend_is_remote: bool,
    osc_title: Option<&str>,
    project: &ProjectData,
    store: &SessionStore,
) -> String {
    // 1. SSH shell → saved session name.
    if let Some(name) = ssh_session_name(shell_type, store) {
        return name;
    }
    // 2. Local terminal → custom name or local shell name.
    if !shell_type.is_remote() && !backend_is_remote {
        if let Some(name) = project.terminal_names.get(terminal_id) {
            return name.clone();
        }
        return shell_type.local_shell_name();
    }
    // 3. Remote backend → custom name > OSC title > directory name.
    project.terminal_display_name(terminal_id, osc_title.map(|s| s.to_string()))
}
