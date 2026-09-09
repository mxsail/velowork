//! Filesystem action handlers — listing, reading, and mutating project files.

use super::{
    ActionResult, Workspace, resolve_new_project_file, resolve_project_file, validate_leaf_name,
};

#[derive(serde::Serialize)]
pub struct FileEntry {
    pub path: String,
    pub is_dir: bool,
}

#[derive(serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(serde::Serialize)]
pub struct FileMatch {
    pub line_number: usize,
    pub line_content: String,
}

#[derive(serde::Serialize)]
pub struct FileSearchResult {
    pub relative_path: String,
    pub matches: Vec<FileMatch>,
}

fn scan_dir_recursive(root: &std::path::Path, dir: &std::path::Path, entries: &mut Vec<FileEntry>) {
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let p = entry.path();
            let is_dir = p.is_dir();
            if let Ok(rel) = p.strip_prefix(root) {
                entries.push(FileEntry {
                    path: rel.display().to_string(),
                    is_dir,
                });
            }
            if is_dir {
                scan_dir_recursive(root, &p, entries);
            }
        }
    }
}

pub(super) fn list_files(ws: &Workspace, project_id: String, _show_ignored: bool) -> ActionResult {
    match ws.project(&project_id) {
        Some(p) => {
            let path = match std::path::Path::new(&p.path).canonicalize() {
                Ok(c) => c,
                Err(e) => return ActionResult::Err(format!("Cannot resolve project path: {}", e)),
            };
            let mut files = Vec::new();
            scan_dir_recursive(&path, &path, &mut files);
            ActionResult::Ok(Some(serde_json::to_value(files).expect("BUG: FileEntry must serialize")))
        }
        None => ActionResult::Err(format!("project not found: {}", project_id)),
    }
}

pub(super) fn list_directory(ws: &Workspace, project_id: String, relative_path: String, _show_ignored: bool) -> ActionResult {
    match ws.project(&project_id) {
        Some(p) => {
            let project_path = match std::path::Path::new(&p.path).canonicalize() {
                Ok(c) => c,
                Err(e) => return ActionResult::Err(format!("Cannot resolve project path: {}", e)),
            };
            let target_dir = project_path.join(&relative_path);
            let read_dir = match std::fs::read_dir(&target_dir) {
                Ok(rd) => rd,
                Err(e) => return ActionResult::Err(format!("Cannot read directory: {}", e)),
            };
            let mut entries = Vec::new();
            for entry in read_dir.flatten() {
                let is_dir = entry.path().is_dir();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                entries.push(DirEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    is_dir,
                    size,
                });
            }
            ActionResult::Ok(Some(
                serde_json::to_value(entries).expect("BUG: DirEntry must serialize"),
            ))
        }
        None => ActionResult::Err(format!("project not found: {}", project_id)),
    }
}

pub(super) fn read_file(ws: &Workspace, project_id: String, relative_path: String) -> ActionResult {
    match ws.project(&project_id) {
        Some(p) => {
            let canonical = match resolve_project_file(&p.path, &relative_path) {
                Ok(c) => c,
                Err(e) => return ActionResult::Err(e),
            };
            match std::fs::read_to_string(&canonical) {
                Ok(content) => ActionResult::Ok(Some(serde_json::json!({ "content": content }))),
                Err(e) => ActionResult::Err(format!("Cannot read file: {}", e)),
            }
        }
        None => ActionResult::Err(format!("project not found: {}", project_id)),
    }
}

/// Server-side ceiling on bytes returned from ReadFileBytes. Mirrors the
/// client's MAX_IMAGE_FILE_SIZE so a misbehaving or older client can't trick
/// the server into reading and base64-encoding arbitrarily large files
/// (each request transiently holds raw + base64 + JSON copies, so the
/// resident multiple is roughly 3-4× the file size).
const MAX_READ_FILE_BYTES: u64 = 20 * 1024 * 1024;

pub(super) fn read_file_bytes(ws: &Workspace, project_id: String, relative_path: String) -> ActionResult {
    use base64::Engine as _;
    match ws.project(&project_id) {
        Some(p) => {
            let canonical = match resolve_project_file(&p.path, &relative_path) {
                Ok(c) => c,
                Err(e) => return ActionResult::Err(e),
            };
            // Enforce the cap from metadata before allocating; std::fs::read
            // alone would happily pull a multi-GB file into memory.
            match std::fs::metadata(&canonical) {
                Ok(m) if m.len() > MAX_READ_FILE_BYTES => {
                    return ActionResult::Err(format!(
                        "File too large ({:.1} MB). Maximum is {} MB.",
                        m.len() as f64 / 1024.0 / 1024.0,
                        MAX_READ_FILE_BYTES / 1024 / 1024
                    ));
                }
                Ok(_) => {}
                Err(e) => return ActionResult::Err(format!("Cannot read file: {}", e)),
            }
            match std::fs::read(&canonical) {
                Ok(bytes) => {
                    if bytes.len() as u64 > MAX_READ_FILE_BYTES {
                        // TOCTOU: file grew between stat and read.
                        return ActionResult::Err(format!(
                            "File too large ({:.1} MB). Maximum is {} MB.",
                            bytes.len() as f64 / 1024.0 / 1024.0,
                            MAX_READ_FILE_BYTES / 1024 / 1024
                        ));
                    }
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    ActionResult::Ok(Some(serde_json::json!({ "content_b64": encoded })))
                }
                Err(e) => ActionResult::Err(format!("Cannot read file: {}", e)),
            }
        }
        None => ActionResult::Err(format!("project not found: {}", project_id)),
    }
}

pub(super) fn file_size(ws: &Workspace, project_id: String, relative_path: String) -> ActionResult {
    match ws.project(&project_id) {
        Some(p) => {
            let canonical = match resolve_project_file(&p.path, &relative_path) {
                Ok(c) => c,
                Err(e) => return ActionResult::Err(e),
            };
            match std::fs::metadata(&canonical) {
                Ok(m) => ActionResult::Ok(Some(serde_json::json!({ "size": m.len() }))),
                Err(e) => ActionResult::Err(format!("Cannot read file: {}", e)),
            }
        }
        None => ActionResult::Err(format!("project not found: {}", project_id)),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn search_content(
    ws: &Workspace,
    project_id: String,
    query: String,
    case_sensitive: bool,
    _mode: String,
    max_results: usize,
    file_glob: Option<String>,
    _context_lines: usize,
) -> ActionResult {
    if let Some(ref glob) = file_glob
        && (glob.contains("..") || glob.starts_with('/')) {
            return ActionResult::Err("file_glob must not contain '..' or start with '/'".to_string());
        }
    match ws.project(&project_id) {
        Some(p) => {
            let path = match std::path::Path::new(&p.path).canonicalize() {
                Ok(c) => c,
                Err(e) => return ActionResult::Err(format!("Cannot resolve project path: {}", e)),
            };
            let mut results = Vec::new();
            let mut total_matches = 0;

            fn search_in_dir(
                root: &std::path::Path,
                dir: &std::path::Path,
                query: &str,
                case_sensitive: bool,
                max_results: usize,
                total_matches: &mut usize,
                results: &mut Vec<FileSearchResult>,
            ) {
                if *total_matches >= max_results {
                    return;
                }
                if let Ok(read_dir) = std::fs::read_dir(dir) {
                    for entry in read_dir.flatten() {
                        if *total_matches >= max_results {
                            break;
                        }
                        let p = entry.path();
                        if p.is_file() {
                            if let Ok(content) = std::fs::read_to_string(&p) {
                                let mut matches = Vec::new();
                                for (idx, line) in content.lines().enumerate() {
                                    let matched = if case_sensitive {
                                        line.contains(query)
                                    } else {
                                        line.to_lowercase().contains(&query.to_lowercase())
                                    };
                                    if matched {
                                        *total_matches += 1;
                                        matches.push(FileMatch {
                                            line_number: idx + 1,
                                            line_content: line.to_string(),
                                        });
                                        if *total_matches >= max_results {
                                            break;
                                        }
                                    }
                                }
                                if !matches.is_empty() {
                                    let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
                                    results.push(FileSearchResult {
                                        relative_path: rel,
                                        matches,
                                    });
                                }
                            }
                        } else if p.is_dir() {
                            search_in_dir(root, &p, query, case_sensitive, max_results, total_matches, results);
                        }
                    }
                }
            }

            search_in_dir(&path, &path, &query, case_sensitive, max_results, &mut total_matches, &mut results);
            ActionResult::Ok(Some(serde_json::to_value(results).expect("BUG: FileSearchResult must serialize")))
        }
        None => ActionResult::Err(format!("project not found: {}", project_id)),
    }
}

pub(super) fn rename_file(ws: &Workspace, project_id: String, relative_path: String, new_name: String) -> ActionResult {
    if let Err(e) = validate_leaf_name(&new_name) {
        return ActionResult::Err(e);
    }
    let project_path = match ws.project(&project_id) {
        Some(p) => p.path.clone(),
        None => return ActionResult::Err(format!("project not found: {}", project_id)),
    };
    let old_path = match resolve_project_file(&project_path, &relative_path) {
        Ok(c) => c,
        Err(e) => return ActionResult::Err(e),
    };
    let parent = match old_path.parent() {
        Some(p) => p,
        None => return ActionResult::Err("cannot rename project root".to_string()),
    };
    let new_path = parent.join(&new_name);
    if new_path.exists() {
        return ActionResult::Err(format!("target already exists: {}", new_name));
    }
    match std::fs::rename(&old_path, &new_path) {
        Ok(()) => ActionResult::Ok(None),
        Err(e) => ActionResult::Err(format!("Cannot rename: {}", e)),
    }
}

pub(super) fn delete_file(ws: &Workspace, project_id: String, relative_path: String) -> ActionResult {
    let project_path = match ws.project(&project_id) {
        Some(p) => p.path.clone(),
        None => return ActionResult::Err(format!("project not found: {}", project_id)),
    };
    let target = match resolve_project_file(&project_path, &relative_path) {
        Ok(c) => c,
        Err(e) => return ActionResult::Err(e),
    };
    let project_root = match std::path::Path::new(&project_path).canonicalize() {
        Ok(r) => r,
        Err(e) => return ActionResult::Err(format!("Cannot resolve project path: {}", e)),
    };
    if target == project_root {
        return ActionResult::Err("cannot delete project root".to_string());
    }
    let result = if target.is_dir() {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_file(&target)
    };
    match result {
        Ok(()) => ActionResult::Ok(None),
        Err(e) => ActionResult::Err(format!("Cannot delete: {}", e)),
    }
}

pub(super) fn create_file(ws: &Workspace, project_id: String, relative_path: String) -> ActionResult {
    let project_path = match ws.project(&project_id) {
        Some(p) => p.path.clone(),
        None => return ActionResult::Err(format!("project not found: {}", project_id)),
    };
    let target = match resolve_new_project_file(&project_path, &relative_path) {
        Ok(c) => c,
        Err(e) => return ActionResult::Err(e),
    };
    if target.exists() {
        return ActionResult::Err("target already exists".to_string());
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
        Ok(_) => ActionResult::Ok(None),
        Err(e) => ActionResult::Err(format!("Cannot create file: {}", e)),
    }
}

pub(super) fn create_directory(ws: &Workspace, project_id: String, relative_path: String) -> ActionResult {
    let project_path = match ws.project(&project_id) {
        Some(p) => p.path.clone(),
        None => return ActionResult::Err(format!("project not found: {}", project_id)),
    };
    let target = match resolve_new_project_file(&project_path, &relative_path) {
        Ok(c) => c,
        Err(e) => return ActionResult::Err(e),
    };
    if target.exists() {
        return ActionResult::Err("target already exists".to_string());
    }
    match std::fs::create_dir(&target) {
        Ok(()) => ActionResult::Ok(None),
        Err(e) => ActionResult::Err(format!("Cannot create directory: {}", e)),
    }
}
