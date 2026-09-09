use velowork_state::{TunnelNode, TunnelTree};

/// Generate a process-unique id for a tunnel node (folder or tunnel).
pub fn new_tunnel_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering as AO};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, AO::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("tn-{}-{}", nanos, n)
}

/// Mutable lookup of a node by id (recurses into folders).
pub fn tunnel_find_node_mut<'a>(
    nodes: &'a mut [TunnelNode],
    id: &str,
) -> Option<&'a mut TunnelNode> {
    for node in nodes.iter_mut() {
        if node.id() == id {
            return Some(node);
        }
        if let TunnelNode::Folder { children, .. } = node {
            if let Some(found) = tunnel_find_node_mut(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Immutable lookup of a node by id (recurses into folders).
pub fn tunnel_find_node_ref<'a>(nodes: &'a [TunnelNode], id: &str) -> Option<&'a TunnelNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let TunnelNode::Folder { children, .. } = node {
            if let Some(found) = tunnel_find_node_ref(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Check if a folder with `target_name` already exists under `parent_id` (excluding `except_id`).
pub fn tunnel_folder_name_exists(
    nodes: &[TunnelNode],
    parent_id: Option<&str>,
    target_name: &str,
    except_id: Option<&str>,
) -> bool {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return false;
    }
    let siblings: Option<&[TunnelNode]> = if let Some(pid) = parent_id {
        tunnel_find_node_ref(nodes, pid).and_then(|node| match node {
            TunnelNode::Folder { children, .. } => Some(children.as_slice()),
            _ => None,
        })
    } else {
        Some(nodes)
    };

    if let Some(siblings) = siblings {
        siblings.iter().any(|node| {
            if let TunnelNode::Folder { id, name, .. } = node {
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

/// 校验同一目录下（含全部节点类型：文件夹、隧道）是否已存在同名节点。
/// 用于隧道（叶子节点）的新建、编辑与重命名校验；`except_id` 排除节点自身。
/// 不同目录之间允许重名。
pub fn tunnel_node_name_exists(
    nodes: &[TunnelNode],
    parent_id: Option<&str>,
    target_name: &str,
    except_id: Option<&str>,
) -> bool {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return false;
    }
    let siblings: Option<&[TunnelNode]> = if let Some(pid) = parent_id {
        tunnel_find_node_ref(nodes, pid).and_then(|node| match node {
            TunnelNode::Folder { children, .. } => Some(children.as_slice()),
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
pub fn tunnel_parent_id_of(nodes: &[TunnelNode], id: &str) -> Option<Option<String>> {
    for node in nodes {
        match node {
            TunnelNode::Folder {
                id: fid, children, ..
            } => {
                if fid == id {
                    return Some(None);
                }
                if let Some(inner) = tunnel_parent_id_of(children, id) {
                    return Some(match inner {
                        Some(pid) => Some(pid),
                        None => Some(fid.clone()),
                    });
                }
            }
            TunnelNode::Tunnel { profile } => {
                if profile.id == id {
                    return Some(None);
                }
            }
        }
    }
    None
}

/// Insert a node under `parent_id` (or at the root when `parent_id` is `None`).
/// Falls back to the root if `parent_id` does not resolve to a folder.
pub fn tunnel_insert_node(
    nodes: &mut TunnelTree,
    parent_id: Option<&str>,
    node: TunnelNode,
) {
    match parent_id {
        None => {
            nodes.push(node);
            tunnel_sort_siblings(nodes);
        }
        Some(pid) => {
            if let Some(TunnelNode::Folder { children, .. }) = tunnel_find_node_mut(nodes, pid) {
                children.push(node);
                tunnel_sort_siblings(children);
            } else {
                nodes.push(node);
                tunnel_sort_siblings(nodes);
            }
        }
    }
}

/// Remove a node by id, returning it (recurses into folders).
pub fn tunnel_remove_node(nodes: &mut TunnelTree, id: &str) -> Option<TunnelNode> {
    if let Some(pos) = nodes.iter().position(|n| n.id() == id) {
        return Some(nodes.remove(pos));
    }
    for node in nodes.iter_mut() {
        if let TunnelNode::Folder { children, .. } = node {
            if let Some(removed) = tunnel_remove_node(children, id) {
                return Some(removed);
            }
        }
    }
    None
}

/// Return true if `descendant_id` is `ancestor_id` itself or lives inside it.
/// Used to prevent moving a folder into one of its own descendants.
pub fn tunnel_is_self_or_descendant(
    nodes: &[TunnelNode],
    ancestor_id: &str,
    descendant_id: &str,
) -> bool {
    for node in nodes {
        if node.id() == ancestor_id {
            if ancestor_id == descendant_id {
                return true;
            }
            if let TunnelNode::Folder { children, .. } = node {
                if tunnel_contains(children, descendant_id) {
                    return true;
                }
            }
        } else if let TunnelNode::Folder { children, .. } = node {
            if tunnel_is_self_or_descendant(children, ancestor_id, descendant_id) {
                return true;
            }
        }
    }
    false
}

fn tunnel_contains(nodes: &[TunnelNode], id: &str) -> bool {
    for node in nodes {
        if node.id() == id {
            return true;
        }
        if let TunnelNode::Folder { children, .. } = node {
            if tunnel_contains(children, id) {
                return true;
            }
        }
    }
    false
}

/// Move `node_id` to `new_parent_id` (or the root when `None`) at `new_index`.
/// A folder can never be moved into itself or one of its descendants. The node
/// keeps its identity (id) and all of its children intact.
pub fn tunnel_move_node(
    nodes: &mut TunnelTree,
    node_id: &str,
    new_parent_id: Option<&str>,
    new_index: usize,
) {
    if let Some(pid) = new_parent_id {
        if tunnel_is_self_or_descendant(nodes, node_id, pid) {
            return;
        }
    }
    let removed = tunnel_remove_node(nodes, node_id);
    if let Some(node) = removed {
        match new_parent_id {
            None => {
                let idx = new_index.min(nodes.len());
                nodes.insert(idx, node);
                tunnel_sort_siblings(nodes);
            }
            Some(pid) => {
                if let Some(TunnelNode::Folder { children, .. }) = tunnel_find_node_mut(nodes, pid) {
                    let idx = new_index.min(children.len());
                    children.insert(idx, node);
                    tunnel_sort_siblings(children);
                } else {
                    nodes.push(node);
                    tunnel_sort_siblings(nodes);
                }
            }
        }
    }
}

/// Recursively sort each level of the tunnel tree so that tunnel (leaf) nodes
/// always appear before folder (directory) nodes. Order within the same kind is
/// preserved (stable sort). Mirrors the sibling ordering used by the
/// session/quick-command trees.
pub fn tunnel_sort_siblings(nodes: &mut [TunnelNode]) {
    nodes.sort_by(|a, b| {
        let a_is_folder = matches!(a, TunnelNode::Folder { .. });
        let b_is_folder = matches!(b, TunnelNode::Folder { .. });
        // Tunnels (false) sort before folders (true).
        a_is_folder.cmp(&b_is_folder)
    });
    for node in nodes.iter_mut() {
        if let TunnelNode::Folder { children, .. } = node {
            tunnel_sort_siblings(children);
        }
    }
}

/// Collect all folders as `(id, display_path)` pairs, with nested folders shown
/// using a "Parent/Child" path. Used to populate the "所属目录" dropdown.
pub fn tunnel_collect_folders(
    nodes: &[TunnelNode],
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    for node in nodes {
        if let TunnelNode::Folder {
            id, name, children, ..
        } = node
        {
            let display = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            out.push((id.clone(), display.clone()));
            tunnel_collect_folders(children, &display, out);
        }
    }
}

/// Recursively find or create nested folders according to path segments.
/// Returns the ID of the leaf folder.
pub fn ensure_tunnel_folder_path(
    nodes: &mut Vec<TunnelNode>,
    segments: &[String],
    project_id: Option<&str>,
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
                if let Some(TunnelNode::Folder { children, .. }) = tunnel_find_node_mut(nodes, pid) {
                    children.iter().find_map(|child| {
                        if let TunnelNode::Folder { id, name, .. } = child {
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
                if let TunnelNode::Folder { id, name, .. } = node {
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
            let new_id = new_tunnel_id();
            let new_folder = TunnelNode::Folder {
                id: new_id.clone(),
                name: seg_trimmed.to_string(),
                project_id: project_id.map(|s| s.to_string()),
                expanded: true,
                children: Vec::new(),
            };
            tunnel_insert_node(nodes, current_parent_id.as_deref(), new_folder);
            current_parent_id = Some(new_id);
        }
    }

    current_parent_id
}

/// Produce a unique name for a duplicated tunnel under `parent_id`.
pub fn tunnel_unique_duplicate_name(
    nodes: &[TunnelNode],
    parent_id: Option<&str>,
    original: &str,
) -> String {
    let siblings: &[TunnelNode] = match parent_id {
        Some(pid) => match tunnel_find_node_ref(nodes, pid) {
            Some(TunnelNode::Folder { children, .. }) => children.as_slice(),
            _ => return format!("{} (副本)", original),
        },
        None => nodes,
    };
    let existing: Vec<String> = siblings
        .iter()
        .filter_map(|n| match n {
            TunnelNode::Tunnel { profile } => Some(profile.name.clone()),
            TunnelNode::Folder { name, .. } => Some(name.clone()),
        })
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_tunnel_folder_path() {
        let mut nodes = Vec::new();
        let segs = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let leaf_id = ensure_tunnel_folder_path(&mut nodes, &segs, None).unwrap();

        let mut folders = Vec::new();
        tunnel_collect_folders(&nodes, "", &mut folders);
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].1, "A");
        assert_eq!(folders[1].1, "A/B");
        assert_eq!(folders[2].1, "A/B/C");
        assert_eq!(folders[2].0, leaf_id);

        // Idempotent: ensuring A/B should reuse existing
        let segs2 = vec!["A".to_string(), "B".to_string()];
        let ab_id = ensure_tunnel_folder_path(&mut nodes, &segs2, None).unwrap();
        assert_eq!(ab_id, folders[1].0);
    }
}
