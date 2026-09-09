use velowork_state::{ServiceDefinition, ServiceNode, ServiceTree};

pub fn new_service_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering as AO};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, AO::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("svc-{}-{}", nanos, n)
}

pub fn new_service_folder_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering as AO};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let _n = COUNTER.fetch_add(1, AO::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("svcf-{}", nanos % 0xFFFFFF)
}

pub fn service_find_node_mut<'a>(
    nodes: &'a mut [ServiceNode],
    id: &str,
) -> Option<&'a mut ServiceNode> {
    for node in nodes.iter_mut() {
        if node.id() == id {
            return Some(node);
        }
        if let ServiceNode::Folder { children, .. } = node {
            if let Some(found) = service_find_node_mut(children, id) {
                return Some(found);
            }
        }
    }
    None
}

pub fn service_find_node_ref<'a>(nodes: &'a [ServiceNode], id: &str) -> Option<&'a ServiceNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let ServiceNode::Folder { children, .. } = node {
            if let Some(found) = service_find_node_ref(children, id) {
                return Some(found);
            }
        }
    }
    None
}

pub fn service_folder_name_exists(
    nodes: &[ServiceNode],
    parent_id: Option<&str>,
    target_name: &str,
    except_id: Option<&str>,
) -> bool {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return false;
    }
    let siblings: Option<&[ServiceNode]> = if let Some(pid) = parent_id {
        service_find_node_ref(nodes, pid).and_then(|node| match node {
            ServiceNode::Folder { children, .. } => Some(children.as_slice()),
            _ => None,
        })
    } else {
        Some(nodes)
    };

    if let Some(siblings) = siblings {
        siblings.iter().any(|node| {
            if let ServiceNode::Folder { id, name, .. } = node {
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

pub fn service_node_name_exists(
    nodes: &[ServiceNode],
    parent_id: Option<&str>,
    target_name: &str,
    except_id: Option<&str>,
) -> bool {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return false;
    }
    let siblings: Option<&[ServiceNode]> = if let Some(pid) = parent_id {
        service_find_node_ref(nodes, pid).and_then(|node| match node {
            ServiceNode::Folder { children, .. } => Some(children.as_slice()),
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

pub fn service_parent_id_of(nodes: &[ServiceNode], id: &str) -> Option<Option<String>> {
    for node in nodes {
        match node {
            ServiceNode::Folder {
                id: fid, children, ..
            } => {
                if fid == id {
                    return Some(None);
                }
                if let Some(inner) = service_parent_id_of(children, id) {
                    return Some(match inner {
                        Some(pid) => Some(pid),
                        None => Some(fid.clone()),
                    });
                }
            }
            ServiceNode::Service { def } => {
                if def.id == id {
                    return Some(None);
                }
            }
        }
    }
    None
}

pub fn service_insert_node(nodes: &mut ServiceTree, parent_id: Option<&str>, node: ServiceNode) {
    match parent_id {
        None => {
            nodes.push(node);
        }
        Some(pid) => {
            if let Some(ServiceNode::Folder { children, .. }) = service_find_node_mut(nodes, pid) {
                children.push(node);
            } else {
                nodes.push(node);
            }
        }
    }
}

pub fn service_remove_node(nodes: &mut ServiceTree, id: &str) -> Option<ServiceNode> {
    if let Some(pos) = nodes.iter().position(|n| n.id() == id) {
        return Some(nodes.remove(pos));
    }
    for node in nodes.iter_mut() {
        if let ServiceNode::Folder { children, .. } = node {
            if let Some(removed) = service_remove_node(children, id) {
                return Some(removed);
            }
        }
    }
    None
}

pub fn service_is_self_or_descendant(
    nodes: &[ServiceNode],
    ancestor_id: &str,
    descendant_id: &str,
) -> bool {
    if ancestor_id == descendant_id {
        return true;
    }
    if let Some(node) = service_find_node_ref(nodes, ancestor_id) {
        if let ServiceNode::Folder { children, .. } = node {
            for child in children {
                if service_is_self_or_descendant(children, child.id(), descendant_id) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn service_move_node(
    nodes: &mut ServiceTree,
    source_id: &str,
    target_id: Option<&str>,
    as_child: bool,
) -> bool {
    if let Some(tid) = target_id {
        if service_is_self_or_descendant(nodes, source_id, tid) {
            return false;
        }
    }

    let node = match service_remove_node(nodes, source_id) {
        Some(n) => n,
        None => return false,
    };

    if as_child {
        if let Some(tid) = target_id {
            if let Some(ServiceNode::Folder { children, .. }) = service_find_node_mut(nodes, tid) {
                children.push(node);
                return true;
            }
        }
        nodes.push(node);
        return true;
    }

    match target_id {
        None => {
            nodes.push(node);
        }
        Some(tid) => {
            let parent_id = service_parent_id_of(nodes, tid).flatten();
            let target_list: &mut Vec<ServiceNode> = if let Some(pid) = parent_id.as_deref() {
                if let Some(ServiceNode::Folder { children, .. }) = service_find_node_mut(nodes, pid) {
                    children
                } else {
                    nodes
                }
            } else {
                nodes
            };

            let pos = target_list
                .iter()
                .position(|n| n.id() == tid)
                .unwrap_or(target_list.len());
            target_list.insert(pos, node);
        }
    }
    true
}

pub fn service_rename_node(nodes: &mut ServiceTree, id: &str, new_name: &str) -> bool {
    let name = new_name.trim();
    if name.is_empty() {
        return false;
    }
    if let Some(node) = service_find_node_mut(nodes, id) {
        match node {
            ServiceNode::Folder { name: f_name, .. } => {
                *f_name = name.to_string();
                true
            }
            ServiceNode::Service { def } => {
                def.name = name.to_string();
                true
            }
        }
    } else {
        false
    }
}

pub fn service_toggle_folder(nodes: &mut ServiceTree, id: &str) {
    if let Some(ServiceNode::Folder { expanded, .. }) = service_find_node_mut(nodes, id) {
        *expanded = !*expanded;
    }
}

pub fn service_unique_duplicate_name(
    nodes: &[ServiceNode],
    parent_id: Option<&str>,
    base_name: &str,
) -> String {
    let mut candidate = format!("{} (copy)", base_name);
    let mut index = 2;
    while service_node_name_exists(nodes, parent_id, &candidate, None) {
        candidate = format!("{} (copy {})", base_name, index);
        index += 1;
    }
    candidate
}

pub fn collect_all_service_definitions(nodes: &[ServiceNode], out: &mut Vec<ServiceDefinition>) {
    for node in nodes {
        match node {
            ServiceNode::Folder { children, .. } => {
                collect_all_service_definitions(children, out);
            }
            ServiceNode::Service { def } => {
                out.push(def.clone());
            }
        }
    }
}

/// Recursively find or create nested service folders according to path segments.
/// Returns the ID of the leaf folder.
pub fn ensure_service_folder_path(
    nodes: &mut ServiceTree,
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
                if let Some(ServiceNode::Folder { children, .. }) = service_find_node_mut(nodes, pid) {
                    children.iter().find_map(|child| {
                        if let ServiceNode::Folder { id, name, .. } = child {
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
                if let ServiceNode::Folder { id, name, .. } = node {
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
            let new_id = new_service_folder_id();
            let new_folder = ServiceNode::Folder {
                id: new_id.clone(),
                name: seg_trimmed.to_string(),
                project_id: project_id.map(|s| s.to_string()),
                expanded: true,
                children: Vec::new(),
            };
            service_insert_node(nodes, current_parent_id.as_deref(), new_folder);
            current_parent_id = Some(new_id);
        }
    }

    current_parent_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use velowork_state::collect_folder_nodes;

    #[test]
    fn test_ensure_service_folder_path() {
        let mut nodes = Vec::new();
        let segs = vec!["Services".to_string(), "DB".to_string(), "Redis".to_string()];
        let leaf_id = ensure_service_folder_path(&mut nodes, &segs, None).unwrap();

        let mut folders = Vec::new();
        collect_folder_nodes(&nodes, "", &mut folders);
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].1, "Services");
        assert_eq!(folders[1].1, "Services/DB");
        assert_eq!(folders[2].1, "Services/DB/Redis");
        assert_eq!(folders[2].0, leaf_id);
    }
}
