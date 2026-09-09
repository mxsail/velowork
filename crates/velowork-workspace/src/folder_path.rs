//! 统一多级目录路径解析与校验工具。
//!
//! 支持解析由 `/` 或 `\` 分隔的多级目录路径（如 `A/B/C`），
//! 并对各级目录名称进行合法性校验（非空、首字符禁止为特殊符号等）。

/// 解析并校验多级目录路径字符串。
///
/// # 规则
/// - 以 `/` 或 `\` 作为多级目录层级分隔符；
/// - 自动去除各分段首尾空白；
/// - 忽略首尾多余的分隔符；
/// - 每个分段必须非空；
/// - 每个分段的首字符必须为常规字符（字母、数字、中文等 `is_alphanumeric`），严禁以特殊符号（如 `.`, `_`, `-`, `@`, `!` 等）开头；
///
/// # 返回
/// - `Ok(Vec<String>)`：按层级顺序排列的各级有效目录名列表；
/// - `Err(String)`：校验失败原因提示信息。
pub fn parse_and_validate_folder_path(input: &str) -> Result<Vec<String>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("目录路径不能为空".to_string());
    }

    let raw_segments: Vec<&str> = trimmed
        .split(|c| c == '/' || c == '\\')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if raw_segments.is_empty() {
        return Err("目录路径不能为空".to_string());
    }

    let mut segments = Vec::with_capacity(raw_segments.len());
    for seg in raw_segments {
        let first_char = match seg.chars().next() {
            Some(c) => c,
            None => return Err("目录名称不能为空".to_string()),
        };

        if !first_char.is_alphanumeric() {
            return Err(format!(
                "目录名称「{}」不能以特殊符号「{}」开头",
                seg, first_char
            ));
        }

        // 检查是否有控制字符
        if seg.chars().any(|c| c.is_control()) {
            return Err(format!("目录名称「{}」包含非法控制字符", seg));
        }

        segments.push(seg.to_string());
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_single_and_multi_paths() {
        assert_eq!(
            parse_and_validate_folder_path("服务器").unwrap(),
            vec!["服务器".to_string()]
        );
        assert_eq!(
            parse_and_validate_folder_path("dev/backend/api").unwrap(),
            vec!["dev".to_string(), "backend".to_string(), "api".to_string()]
        );
        assert_eq!(
            parse_and_validate_folder_path("项目A\\微服务/用户中心").unwrap(),
            vec![
                "项目A".to_string(),
                "微服务".to_string(),
                "用户中心".to_string()
            ]
        );
        assert_eq!(
            parse_and_validate_folder_path(" / 生产环境 / 数据库 / ").unwrap(),
            vec!["生产环境".to_string(), "数据库".to_string()]
        );
    }

    #[test]
    fn test_invalid_paths_empty() {
        assert!(parse_and_validate_folder_path("").is_err());
        assert!(parse_and_validate_folder_path("   ").is_err());
        assert!(parse_and_validate_folder_path(" / / ").is_err());
    }

    #[test]
    fn test_invalid_paths_special_characters_at_start() {
        assert!(parse_and_validate_folder_path(".hidden").is_err());
        assert!(parse_and_validate_folder_path("_internal").is_err());
        assert!(parse_and_validate_folder_path("-dash").is_err());
        assert!(parse_and_validate_folder_path("valid/-invalid").is_err());
        assert!(parse_and_validate_folder_path("valid/@user").is_err());
        assert!(parse_and_validate_folder_path("valid/#hashtag").is_err());
    }
}
