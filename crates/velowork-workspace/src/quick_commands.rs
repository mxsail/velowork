//! 快捷指令（Quick Commands）数据结构与树形操作工具。
//!
//! 包含 `QuickCommandNode`（文件夹与指令节点）、变量定义以及节点查找、插入、删除、移动、排序和多级目录构建等操作。

use serde::{Deserialize, Serialize};

/// Variable definition inside a quick command.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuickCommandVar {
    pub name: String,
    pub default_value: String,
    pub hint: String,
}

/// A node in the quick-command tree: either a folder (group) or a command.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuickCommandNode {
    Folder {
        id: String,
        name: String,
        expanded: bool,
        children: Vec<QuickCommandNode>,
    },
    Command {
        id: String,
        name: String,
        command: String,
        variables: Vec<QuickCommandVar>,
    },
}

impl QuickCommandNode {
    pub fn id(&self) -> &str {
        match self {
            QuickCommandNode::Folder { id, .. } => id,
            QuickCommandNode::Command { id, .. } => id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            QuickCommandNode::Folder { name, .. } => name,
            QuickCommandNode::Command { name, .. } => name,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, QuickCommandNode::Folder { .. })
    }
}

/// Generate a process-unique id for a quick-command node. Combines a
/// monotonically increasing counter with the current nanosecond timestamp so
/// ids stay unique across process restarts (persisted ids are reused as-is).
pub fn new_quick_command_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering as AO};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, AO::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("qc-{}-{}", nanos, n)
}

/// Mutable lookup of a node by id (recurses into folders).
pub fn qc_find_node_mut<'a>(
    nodes: &'a mut [QuickCommandNode],
    id: &str,
) -> Option<&'a mut QuickCommandNode> {
    for node in nodes.iter_mut() {
        if node.id() == id {
            return Some(node);
        }
        if let QuickCommandNode::Folder { children, .. } = node {
            if let Some(found) = qc_find_node_mut(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Immutable lookup of a node by id (recurses into folders).
pub fn qc_find_node_ref<'a>(
    nodes: &'a [QuickCommandNode],
    id: &str,
) -> Option<&'a QuickCommandNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let QuickCommandNode::Folder { children, .. } = node {
            if let Some(found) = qc_find_node_ref(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Check if a folder with `target_name` already exists under `parent_id` (excluding `except_id`).
pub fn qc_folder_name_exists(
    nodes: &[QuickCommandNode],
    parent_id: Option<&str>,
    target_name: &str,
    except_id: Option<&str>,
) -> bool {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return false;
    }
    let siblings: Option<&[QuickCommandNode]> = if let Some(pid) = parent_id {
        qc_find_node_ref(nodes, pid).and_then(|node| match node {
            QuickCommandNode::Folder { children, .. } => Some(children.as_slice()),
            _ => None,
        })
    } else {
        Some(nodes)
    };

    if let Some(siblings) = siblings {
        siblings.iter().any(|node| {
            if let QuickCommandNode::Folder { id, name, .. } = node {
                if let Some(eid) = except_id {
                    if id == eid {
                        return false;
                    }
                }
                name.trim() == target_name
            } else {
                false
            }
        })
    } else {
        false
    }
}

/// 校验同一目录下（含全部节点类型：文件夹、指令）是否已存在同名节点。
/// 用于指令（叶子节点）的新建、编辑与重命名校验；`except_id` 排除节点自身。
/// 不同目录之间允许重名。
pub fn qc_node_name_exists(
    nodes: &[QuickCommandNode],
    parent_id: Option<&str>,
    target_name: &str,
    except_id: Option<&str>,
) -> bool {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return false;
    }
    let siblings: Option<&[QuickCommandNode]> = if let Some(pid) = parent_id {
        qc_find_node_ref(nodes, pid).and_then(|node| match node {
            QuickCommandNode::Folder { children, .. } => Some(children.as_slice()),
            _ => None,
        })
    } else {
        Some(nodes)
    };

    if let Some(siblings) = siblings {
        siblings.iter().any(|node| {
            let (id, name) = (node.id(), node.name());
            if let Some(eid) = except_id {
                if id == eid {
                    return false;
                }
            }
            name.trim() == target_name
        })
    } else {
        false
    }
}

/// Return the parent id of `id`, or `None` if it lives at the root level.
/// Returns `Some(...)` only when the node exists; `None` means "not found".
pub fn qc_parent_id_of(nodes: &[QuickCommandNode], id: &str) -> Option<Option<String>> {
    for node in nodes {
        match node {
            QuickCommandNode::Folder {
                id: fid, children, ..
            } => {
                if fid == id {
                    return Some(None);
                }
                if let Some(inner) = qc_parent_id_of(children, id) {
                    return Some(match inner {
                        Some(pid) => Some(pid),
                        None => Some(fid.clone()),
                    });
                }
            }
            QuickCommandNode::Command { id: cid, .. } => {
                if cid == id {
                    return Some(None);
                }
            }
        }
    }
    None
}

/// Insert a node under `parent_id` (or at the root when `parent_id` is `None`).
/// Falls back to the root if `parent_id` does not resolve to a folder.
pub fn qc_insert_node(
    nodes: &mut Vec<QuickCommandNode>,
    parent_id: Option<&str>,
    node: QuickCommandNode,
) {
    match parent_id {
        None => {
            nodes.push(node);
            qc_sort_siblings(nodes);
        }
        Some(pid) => {
            if let Some(QuickCommandNode::Folder { children, .. }) = qc_find_node_mut(nodes, pid) {
                children.push(node);
                qc_sort_siblings(children);
            } else {
                nodes.push(node);
                qc_sort_siblings(nodes);
            }
        }
    }
}

/// Collect the names of all *command* (leaf) nodes that are direct children of
/// `parent_id`. When `parent_id` is `None`, returns the names of all root-level
/// command nodes. Used to pick a non-colliding name when duplicating a command.
fn qc_sibling_command_names(nodes: &[QuickCommandNode], parent_id: Option<&str>) -> Vec<String> {
    let siblings: &[QuickCommandNode] = match parent_id {
        Some(pid) => match qc_find_node_ref(nodes, pid) {
            Some(QuickCommandNode::Folder { children, .. }) => children.as_slice(),
            _ => return Vec::new(),
        },
        None => nodes,
    };
    siblings
        .iter()
        .filter_map(|n| match n {
            QuickCommandNode::Command { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// Produce a unique name for a duplicated quick command under `parent_id`.
pub fn qc_unique_duplicate_name(
    nodes: &[QuickCommandNode],
    parent_id: Option<&str>,
    original: &str,
) -> String {
    let existing = qc_sibling_command_names(nodes, parent_id);
    let first = format!("{} (副本)", original);
    if !existing.contains(&first) {
        return first;
    }
    let mut n = 2u32;
    loop {
        let cand = format!("{} (副本) {}", original, n);
        if !existing.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Remove a node by id, returning it (recurses into folders).
pub fn qc_remove_node(nodes: &mut Vec<QuickCommandNode>, id: &str) -> Option<QuickCommandNode> {
    if let Some(pos) = nodes.iter().position(|n| n.id() == id) {
        return Some(nodes.remove(pos));
    }
    for node in nodes.iter_mut() {
        if let QuickCommandNode::Folder { children, .. } = node {
            if let Some(removed) = qc_remove_node(children, id) {
                return Some(removed);
            }
        }
    }
    None
}

/// Return true when `id` exists anywhere inside `nodes` (recurses into folders).
fn qc_contains(nodes: &[QuickCommandNode], id: &str) -> bool {
    for node in nodes {
        if node.id() == id {
            return true;
        }
        if let QuickCommandNode::Folder { children, .. } = node {
            if qc_contains(children, id) {
                return true;
            }
        }
    }
    false
}

/// Return true if `descendant_id` is `ancestor_id` itself or lives inside it.
pub fn qc_is_self_or_descendant(
    nodes: &[QuickCommandNode],
    ancestor_id: &str,
    descendant_id: &str,
) -> bool {
    for node in nodes {
        if node.id() == ancestor_id {
            if ancestor_id == descendant_id {
                return true;
            }
            if let QuickCommandNode::Folder { children, .. } = node {
                if qc_contains(children, descendant_id) {
                    return true;
                }
            }
        } else if let QuickCommandNode::Folder { children, .. } = node {
            if qc_is_self_or_descendant(children, ancestor_id, descendant_id) {
                return true;
            }
        }
    }
    false
}

/// Move `node_id` to `new_parent_id` (or the root when `None`) at `new_index`.
pub fn qc_move_node(
    nodes: &mut Vec<QuickCommandNode>,
    node_id: &str,
    new_parent_id: Option<&str>,
    new_index: usize,
) {
    if let Some(pid) = new_parent_id {
        if qc_is_self_or_descendant(nodes, node_id, pid) {
            return;
        }
    }
    let removed = qc_remove_node(nodes, node_id);
    if let Some(node) = removed {
        match new_parent_id {
            None => {
                let idx = new_index.min(nodes.len());
                nodes.insert(idx, node);
                qc_sort_siblings(nodes);
            }
            Some(pid) => {
                if let Some(QuickCommandNode::Folder { children, .. }) =
                    qc_find_node_mut(nodes, pid)
                {
                    let idx = new_index.min(children.len());
                    children.insert(idx, node);
                    qc_sort_siblings(children);
                } else {
                    nodes.push(node);
                    qc_sort_siblings(nodes);
                }
            }
        }
    }
}

/// Recursively sort each level of the quick-command tree so that command nodes
/// always appear before folder (directory) nodes.
pub fn qc_sort_siblings(nodes: &mut [QuickCommandNode]) {
    nodes.sort_by(|a, b| {
        let a_is_folder = matches!(a, QuickCommandNode::Folder { .. });
        let b_is_folder = matches!(b, QuickCommandNode::Folder { .. });
        a_is_folder.cmp(&b_is_folder)
    });
    for node in nodes.iter_mut() {
        if let QuickCommandNode::Folder { children, .. } = node {
            qc_sort_siblings(children);
        }
    }
}

/// Collect all folders as `(id, display_path)` pairs, with nested folders shown
/// using a "Parent/Child" path.
pub fn qc_collect_folders(
    nodes: &[QuickCommandNode],
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    for node in nodes {
        if let QuickCommandNode::Folder {
            id, name, children, ..
        } = node
        {
            let display = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            out.push((id.clone(), display.clone()));
            qc_collect_folders(children, &display, out);
        }
    }
}

/// Recursively find or create nested quick-command folders according to path segments.
/// Returns the ID of the leaf folder.
pub fn qc_ensure_folder_path(
    nodes: &mut Vec<QuickCommandNode>,
    segments: &[String],
) -> Option<String> {
    if segments.is_empty() {
        return None;
    }

    let mut current_parent_id: Option<String> = None;

    for seg in segments {
        let seg_trimmed = seg.trim();
        if seg_trimmed.is_empty() {
            continue;
        }

        let existing_id = match current_parent_id.as_deref() {
            Some(pid) => {
                if let Some(QuickCommandNode::Folder { children, .. }) = qc_find_node_mut(nodes, pid) {
                    children.iter().find_map(|child| {
                        if let QuickCommandNode::Folder { id, name, .. } = child {
                            if name == seg_trimmed {
                                return Some(id.clone());
                            }
                        }
                        None
                    })
                } else {
                    None
                }
            }
            None => nodes.iter().find_map(|node| {
                if let QuickCommandNode::Folder { id, name, .. } = node {
                    if name == seg_trimmed {
                        return Some(id.clone());
                    }
                }
                None
            }),
        };

        if let Some(found_id) = existing_id {
            current_parent_id = Some(found_id);
        } else {
            let new_id = new_quick_command_id();
            let new_folder = QuickCommandNode::Folder {
                id: new_id.clone(),
                name: seg_trimmed.to_string(),
                expanded: true,
                children: Vec::new(),
            };
            qc_insert_node(nodes, current_parent_id.as_deref(), new_folder);
            current_parent_id = Some(new_id);
        }
    }

    current_parent_id
}

/// Default quick-command tree seeded on first run (empty settings).
pub fn default_quick_commands() -> Vec<QuickCommandNode> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qc_ensure_folder_path() {
        let mut tree = Vec::new();
        let segs = vec!["常用".to_string(), "Git".to_string(), "分支".to_string()];
        let leaf_id = qc_ensure_folder_path(&mut tree, &segs).unwrap();

        let mut folders = Vec::new();
        qc_collect_folders(&tree, "", &mut folders);
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].1, "常用");
        assert_eq!(folders[1].1, "常用/Git");
        assert_eq!(folders[2].1, "常用/Git/分支");
        assert_eq!(folders[2].0, leaf_id);
    }
}