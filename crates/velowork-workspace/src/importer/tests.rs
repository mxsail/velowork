use std::fs;
use tempfile::tempdir;
use velowork_state::{SessionProtocol, SessionTreeNode, SshAuthType};

use super::common::parse_ini_sections;
use super::finalshell::FinalShellImporter;
use super::merge::merge_imported_sessions_into_tree;
use super::mobaxterm::parse_mobaxterm_content;
use super::registry::ImporterRegistry;
use super::windterm::parse_windterm_content;
use super::xshell::parse_xsh_content;
use super::{DuplicateStrategy, ImportContext, ImportedSession, SessionImporter};

#[test]
fn test_parse_ini_sections() {
    let ini = r#"
; comment
[CONNECTION]
Host=192.168.1.100
Port=2222
Protocol=SSH

[CONNECTION:AUTHENTICATION]
UserName=admin
UserKey=id_ed25519
"#;
    let sections = parse_ini_sections(ini);
    assert_eq!(sections.get("CONNECTION").unwrap().get("Host").unwrap(), "192.168.1.100");
    assert_eq!(sections.get("CONNECTION").unwrap().get("Port").unwrap(), "2222");
    assert_eq!(sections.get("CONNECTION:AUTHENTICATION").unwrap().get("UserName").unwrap(), "admin");
    assert_eq!(sections.get("CONNECTION:AUTHENTICATION").unwrap().get("UserKey").unwrap(), "id_ed25519");
}

#[test]
fn test_xshell_content_parsing() {
    let content = r#"
[CONNECTION]
Host=10.0.0.1
Port=22
Protocol=SSH

[CONNECTION:AUTHENTICATION]
UserName=deploy
UserKey=deploy_key
"#;
    let sess = parse_xsh_content(content, "Xshell/Sessions/Cluster/Node1.xsh").unwrap();
    assert_eq!(sess.name, "Node1");
    assert_eq!(sess.host, "10.0.0.1");
    assert_eq!(sess.port, 22);
    assert_eq!(sess.username, "deploy");
    assert_eq!(sess.group_path, Some(vec!["Cluster".to_string()]));
    assert_eq!(sess.protocol, SessionProtocol::Ssh);
    assert!(matches!(sess.auth_type, SshAuthType::PrivateKey { key_path, .. } if key_path == "deploy_key"));
}

#[test]
fn test_mobaxterm_content_parsing() {
    let content = r#"
[Bookmarks_1]
SubRep=Cloud\Production
ImgNum=41
Prod-Web=#109#0%172.16.0.5%22%ubuntu%0%0%%-1%0%0%0%%1080%%0%0%1#MobaFont%10%0%0%-1%15%236,236,236%0,0,0%180,180,180%0%-1%0%%-1%0%0%0#0# #-1
"#;
    let sessions = parse_mobaxterm_content(content).unwrap();
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s.name, "Prod-Web");
    assert_eq!(s.host, "172.16.0.5");
    assert_eq!(s.port, 22);
    assert_eq!(s.username, "ubuntu");
    assert_eq!(s.group_path, Some(vec!["Cloud".to_string(), "Production".to_string()]));
}

#[test]
fn test_windterm_content_parsing() {
    let json_content = r#"[
  {
    "session.protocol": "SSH",
    "session.target": "root@192.168.50.1",
    "session.port": 2200,
    "session.label": "Gateway Router",
    "session.group": "Office > Network",
    "session.description": "Main office gateway",
    "session.autoLogin": "{\"PasswordEnabled\": true, \"Password\": \"secret123\"}"
  }
]"#;
    let sessions = parse_windterm_content(json_content, None, None).unwrap();
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s.name, "Gateway Router");
    assert_eq!(s.host, "192.168.50.1");
    assert_eq!(s.port, 2200);
    assert_eq!(s.username, "root");
    assert_eq!(s.group_path, Some(vec!["Office".to_string(), "Network".to_string()]));
    assert_eq!(s.description, Some("Main office gateway".to_string()));
    assert_eq!(s.auth_type, SshAuthType::Password { password: Some("secret123".to_string()) });
}

#[test]
fn test_finalshell_directory_parsing() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Write folder.json
    let folder_json = r#"{
      "id": "f_1",
      "name": "Database Cluster",
      "parent_id": "root",
      "delete_time": 0
    }"#;
    fs::write(root.join("folder.json"), folder_json).unwrap();

    // Write connect config
    let conn_json = r#"{
      "name": "MySQL Master",
      "host": "10.10.10.5",
      "port": 3306,
      "user_name": "dbadmin",
      "parent_id": "f_1",
      "conection_type": 100,
      "description": "Primary database server",
      "delete_time": 0
    }"#;
    fs::write(root.join("mysql_connect_config.json"), conn_json).unwrap();

    let importer = FinalShellImporter;
    let ctx = ImportContext::new(root);
    let sessions = importer.parse(&ctx).unwrap();

    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s.name, "MySQL Master");
    assert_eq!(s.host, "10.10.10.5");
    assert_eq!(s.port, 3306);
    assert_eq!(s.username, "dbadmin");
    assert_eq!(s.group_path, Some(vec!["Database Cluster".to_string()]));
    assert_eq!(s.description, Some("Primary database server".to_string()));
}

#[test]
fn test_importer_registry() {
    let registry = ImporterRegistry::default_registry();
    assert_eq!(registry.all_importers().len(), 4);

    let xshell = registry.find_by_id("xshell").unwrap();
    assert_eq!(xshell.id(), "xshell");

    let moba = registry.find_for_path(std::path::Path::new("/tmp/test.mxtsessions")).unwrap();
    assert_eq!(moba.id(), "mobaxterm");

    let windterm = registry.find_for_path(std::path::Path::new("/tmp/user.sessions")).unwrap();
    assert_eq!(windterm.id(), "windterm");
}

#[test]
fn test_merge_imported_sessions_with_collision_and_folders() {
    let mut tree: Vec<SessionTreeNode> = Vec::new();

    let imported = vec![
        ImportedSession {
            name: "Server A".to_string(),
            protocol: SessionProtocol::Ssh,
            host: "1.1.1.1".to_string(),
            port: 22,
            username: "root".to_string(),
            auth_type: SshAuthType::Password { password: None },
            group_path: Some(vec!["Datacenter".to_string(), "Rack 1".to_string()]),
            description: None,
        },
        ImportedSession {
            name: "Server A".to_string(), // Duplicate name in same folder
            protocol: SessionProtocol::Ssh,
            host: "1.1.1.2".to_string(),
            port: 22,
            username: "root".to_string(),
            auth_type: SshAuthType::Password { password: None },
            group_path: Some(vec!["Datacenter".to_string(), "Rack 1".to_string()]),
            description: None,
        },
        ImportedSession {
            name: "Server B".to_string(),
            protocol: SessionProtocol::Ssh,
            host: "2.2.2.2".to_string(),
            port: 22,
            username: "admin".to_string(),
            auth_type: SshAuthType::Password { password: None },
            group_path: Some(vec!["Datacenter".to_string()]),
            description: None,
        },
    ];

    let result = merge_imported_sessions_into_tree(
        &mut tree,
        imported.clone(),
        DuplicateStrategy::Rename,
    );

    assert_eq!(result.total_found, 3);
    assert_eq!(result.imported_sessions, 3);
    assert_eq!(result.created_folders, 2); // "Datacenter" and "Rack 1"
    assert_eq!(result.renamed_sessions, 1); // Second "Server A" renamed to "Server A (1)"

    assert_eq!(tree.len(), 1);
    match &tree[0] {
        SessionTreeNode::Folder { name, children, .. } => {
            assert_eq!(name, "Datacenter");
            assert_eq!(children.len(), 2);
            assert_eq!(children[0].name(), "Server B");
            match &children[1] {
                SessionTreeNode::Folder { name, children, .. } => {
                    assert_eq!(name, "Rack 1");
                    assert_eq!(children.len(), 2);
                    assert_eq!(children[0].name(), "Server A");
                    assert_eq!(children[1].name(), "Server A (1)");
                }
                _ => panic!("Expected folder Rack 1"),
            }
        }
        _ => panic!("Expected folder Datacenter"),
    }

    // Test Overwrite Strategy
    let mut tree_overwrite: Vec<SessionTreeNode> = Vec::new();
    let res_overwrite = merge_imported_sessions_into_tree(
        &mut tree_overwrite,
        imported.clone(),
        DuplicateStrategy::Overwrite,
    );
    assert_eq!(res_overwrite.imported_sessions, 3);
    assert_eq!(res_overwrite.overwritten_sessions, 1);

    // Test Skip Strategy
    let mut tree_skip: Vec<SessionTreeNode> = Vec::new();
    let res_skip = merge_imported_sessions_into_tree(
        &mut tree_skip,
        imported,
        DuplicateStrategy::Skip,
    );
    assert_eq!(res_skip.imported_sessions, 2);
    assert_eq!(res_skip.skipped_sessions, 1);
}
