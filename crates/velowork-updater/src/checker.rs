use anyhow::{Context, Result};
use semver::Version;
use std::time::Duration;

/// Info about an available release asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub version: String,
    pub asset_url: String,
    pub asset_name: String,
    pub checksum_url: Option<String>,
}

/// Check GitHub for the latest release.
/// `app_version` should be the host application's version (e.g. from the root Cargo.toml).
/// `include_beta` specifies whether pre-release / beta releases are eligible.
pub async fn check_for_update(app_version: String, include_beta: bool) -> Result<Option<ReleaseAsset>> {
    smol::unblock(move || check_blocking(&app_version, include_beta)).await
}

fn check_blocking(app_version: &str, include_beta: bool) -> Result<Option<ReleaseAsset>> {
    // 1. Primary attempt: GitHub Releases Atom feed (unthrottled, covers all releases)
    match check_via_atom_feed(app_version, include_beta) {
        Ok(asset) => return Ok(asset),
        Err(e) => {
            log::warn!(
                "[updater] Atom feed check failed, falling back to web redirect | error: {:#}",
                e
            );
        }
    }

    // 2. Secondary attempt: probe web redirect at https://github.com/mxsail/velowork/releases/latest.
    match check_via_web_redirect(app_version, include_beta) {
        Ok(asset) => return Ok(asset),
        Err(e) => {
            log::warn!(
                "[updater] Web redirect check failed, falling back to GitHub API | error: {:#}",
                e
            );
        }
    }

    // 3. Fallback: GitHub REST API
    check_via_api(app_version, include_beta)
}

pub(crate) fn extract_tags_from_atom(atom_body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let marker = "/releases/tag/";
    for part in atom_body.split(marker).skip(1) {
        if let Some(tag_end) = part.find(&['"', '/', '?', '#', '<', ' '][..]) {
            let tag = &part[..tag_end];
            if !tag.is_empty() && !tags.iter().any(|t| t == tag) {
                tags.push(tag.to_string());
            }
        }
    }
    tags
}

fn check_via_atom_feed(app_version: &str, include_beta: bool) -> Result<Option<ReleaseAsset>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to create http client for atom feed")?;

    let resp = client
        .get("https://github.com/mxsail/velowork/releases.atom")
        .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
        .send()
        .context("failed to fetch releases atom feed")?;

    if !resp.status().is_success() {
        anyhow::bail!("releases atom feed returned status {}", resp.status());
    }

    let body = resp.text().context("failed to read atom feed body")?;
    let tags = extract_tags_from_atom(&body);
    if tags.is_empty() {
        anyhow::bail!("no tags found in releases atom feed");
    }

    let current_version = Version::parse(app_version).context("invalid current version")?;

    let mut best_candidate: Option<(Version, String)> = None;
    for tag in &tags {
        let remote_version_str = tag.strip_prefix('v').unwrap_or(tag);
        let Ok(remote_version) = Version::parse(remote_version_str) else {
            continue;
        };

        // If beta channel is disabled, skip all pre-releases
        if !include_beta && !remote_version.pre.is_empty() {
            continue;
        }

        if remote_version > current_version {
            if let Some((best_ver, _)) = &best_candidate {
                if remote_version > *best_ver {
                    best_candidate = Some((remote_version, tag.clone()));
                }
            } else {
                best_candidate = Some((remote_version, tag.clone()));
            }
        }
    }

    let Some((target_version, target_tag)) = best_candidate else {
        log::info!(
            "No update available (current={}, include_beta={})",
            current_version,
            include_beta
        );
        return Ok(None);
    };

    log::info!(
        "Update available via atom feed: {} -> {} (tag: {})",
        current_version,
        target_version,
        target_tag
    );

    let expected_asset = platform_asset_name();
    let asset_url = format!(
        "https://github.com/mxsail/velowork/releases/download/{target_tag}/{expected_asset}"
    );

    // Verify that the asset exists using standard client
    let probe_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to create asset probe client")?;

    let asset_check = probe_client
        .head(&asset_url)
        .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
        .send();

    match asset_check {
        Ok(res) if res.status().is_success() || res.status().is_redirection() => {
            // Asset exists and is downloadable!
        }
        _ => {
            log::warn!(
                "Release {} exists but asset '{}' not accessible at {}",
                target_version,
                expected_asset,
                asset_url
            );
            return Ok(None);
        }
    }

    // Check optional checksum file
    let mut checksum_url = None;
    for cs_name in &["SHA256SUMS", "sha256sums.txt"] {
        let cs_url = format!(
            "https://github.com/mxsail/velowork/releases/download/{target_tag}/{cs_name}"
        );
        if let Ok(res) = probe_client
            .head(&cs_url)
            .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
            .send()
            && (res.status().is_success() || res.status().is_redirection())
        {
            checksum_url = Some(cs_url);
            break;
        }
    }

    Ok(Some(ReleaseAsset {
        version: target_version.to_string(),
        asset_url,
        asset_name: expected_asset.to_string(),
        checksum_url,
    }))
}

pub(crate) fn extract_tag_from_location(location: &str) -> Option<&str> {
    let marker = "/releases/tag/";
    let idx = location.find(marker)?;
    let after = &location[idx + marker.len()..];
    let tag = after.split(&['/', '?', '#'][..]).next()?;
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

fn check_via_web_redirect(app_version: &str, include_beta: bool) -> Result<Option<ReleaseAsset>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to create http client for redirect probe")?;

    let resp = client
        .head("https://github.com/mxsail/velowork/releases/latest")
        .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
        .send()
        .context("failed to send redirect probe")?;

    let status = resp.status();
    if !status.is_redirection() {
        anyhow::bail!("expected redirect status, got {}", status);
    }

    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .context("missing Location header in redirect response")?
        .to_str()
        .context("invalid Location header string")?;

    let tag = extract_tag_from_location(location)
        .with_context(|| format!("failed to extract tag from location '{}'", location))?;

    let remote_version_str = tag.strip_prefix('v').unwrap_or(tag);
    let remote_version = Version::parse(remote_version_str).context("invalid remote version")?;
    let current_version = Version::parse(app_version).context("invalid current version")?;

    if !include_beta && !remote_version.pre.is_empty() {
        log::info!(
            "Ignoring pre-release {} via redirect because include_beta is false",
            remote_version
        );
        return Ok(None);
    }

    if remote_version <= current_version {
        log::info!(
            "No update available (current={}, latest={})",
            current_version,
            remote_version
        );
        return Ok(None);
    }

    log::info!(
        "Update available via redirect: {} -> {}",
        current_version,
        remote_version
    );

    let expected_asset = platform_asset_name();
    let asset_url = format!(
        "https://github.com/mxsail/velowork/releases/download/{tag}/{expected_asset}"
    );

    // Verify that the asset exists using standard client
    let probe_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to create asset probe client")?;

    let asset_check = probe_client
        .head(&asset_url)
        .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
        .send();

    match asset_check {
        Ok(res) if res.status().is_success() || res.status().is_redirection() => {
            // Asset exists and is downloadable!
        }
        _ => {
            log::warn!(
                "Release {} exists but asset '{}' not accessible at {}",
                remote_version,
                expected_asset,
                asset_url
            );
            return Ok(None);
        }
    }

    // Check optional checksum file
    let mut checksum_url = None;
    for cs_name in &["SHA256SUMS", "sha256sums.txt"] {
        let cs_url = format!(
            "https://github.com/mxsail/velowork/releases/download/{tag}/{cs_name}"
        );
        if let Ok(res) = probe_client
            .head(&cs_url)
            .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
            .send()
            && (res.status().is_success() || res.status().is_redirection())
        {
            checksum_url = Some(cs_url);
            break;
        }
    }

    Ok(Some(ReleaseAsset {
        version: remote_version.to_string(),
        asset_url,
        asset_name: expected_asset.to_string(),
        checksum_url,
    }))
}

fn check_via_api(app_version: &str, include_beta: bool) -> Result<Option<ReleaseAsset>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to create http client")?;

    let endpoint = if include_beta {
        "https://api.github.com/repos/mxsail/velowork/releases?per_page=10"
    } else {
        "https://api.github.com/repos/mxsail/velowork/releases/latest"
    };

    let http_resp = client
        .get(endpoint)
        .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
        .send()
        .context("failed to fetch release from github api")?;

    let status = http_resp.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("GitHub API rate limit exceeded — try again later");
    }
    if !status.is_success() {
        anyhow::bail!("GitHub API returned status {}", status);
    }

    let resp_val: serde_json::Value = http_resp.json().context("failed to parse release JSON")?;
    let current_version = Version::parse(app_version).context("invalid current version")?;

    let releases: Vec<&serde_json::Value> = if let Some(arr) = resp_val.as_array() {
        arr.iter().collect()
    } else if resp_val.is_object() {
        vec![&resp_val]
    } else {
        anyhow::bail!("unexpected release JSON format");
    };

    let mut best_candidate: Option<(Version, &serde_json::Value)> = None;
    for rel in releases {
        let tag = rel["tag_name"].as_str().unwrap_or_default();
        let remote_version_str = tag.strip_prefix('v').unwrap_or(tag);
        let Ok(remote_version) = Version::parse(remote_version_str) else {
            continue;
        };

        if !include_beta && !remote_version.pre.is_empty() {
            continue;
        }

        if remote_version > current_version {
            if let Some((best_ver, _)) = &best_candidate {
                if remote_version > *best_ver {
                    best_candidate = Some((remote_version, rel));
                }
            } else {
                best_candidate = Some((remote_version, rel));
            }
        }
    }

    let Some((target_version, best_rel)) = best_candidate else {
        log::info!(
            "No update available via api (current={}, include_beta={})",
            current_version,
            include_beta
        );
        return Ok(None);
    };

    log::info!(
        "Update available via api: {} -> {}",
        current_version,
        target_version
    );

    let expected_asset = platform_asset_name();
    let assets = best_rel["assets"].as_array().context("missing assets array")?;

    let mut found_asset: Option<(String, String)> = None;
    let mut checksum_url: Option<String> = None;

    for asset in assets {
        let name = asset["name"].as_str().unwrap_or_default();
        if name == expected_asset {
            let url = asset["browser_download_url"]
                .as_str()
                .context("missing download URL")?
                .to_string();
            found_asset = Some((name.to_string(), url));
        } else if (name == "SHA256SUMS" || name == "sha256sums.txt")
            && let Some(url) = asset["browser_download_url"].as_str()
        {
            checksum_url = Some(url.to_string());
        }
    }

    if let Some((asset_name, asset_url)) = found_asset {
        return Ok(Some(ReleaseAsset {
            version: target_version.to_string(),
            asset_url,
            asset_name,
            checksum_url,
        }));
    }

    log::warn!(
        "Release {} exists but no matching asset '{}' found",
        target_version,
        expected_asset
    );
    Ok(None)
}

fn platform_asset_name() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "velowork-linux-x64.tar.gz";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "velowork-linux-arm64.tar.gz";
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "velowork-macos-arm64.zip";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "velowork-macos-x64.zip";
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "velowork-windows-x64.zip";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "velowork-windows-arm64.zip";

    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    compile_error!("unsupported platform for auto-update");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tag_from_location() {
        assert_eq!(
            extract_tag_from_location("https://github.com/mxsail/velowork/releases/tag/v0.1.0-beta.2"),
            Some("v0.1.0-beta.2")
        );
        assert_eq!(
            extract_tag_from_location("/mxsail/velowork/releases/tag/v1.0.0"),
            Some("v1.0.0")
        );
        assert_eq!(
            extract_tag_from_location("https://github.com/mxsail/velowork/releases/tag/v2.1.3?utm=foo#section"),
            Some("v2.1.3")
        );
        assert_eq!(
            extract_tag_from_location("https://github.com/mxsail/velowork/releases/tag/"),
            None
        );
        assert_eq!(
            extract_tag_from_location("https://github.com/mxsail/velowork"),
            None
        );
    }

    #[test]
    fn test_extract_tags_from_atom() {
        let sample_atom = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/">
  <entry>
    <id>tag:github.com,2008:Repository/12345/v0.1.0-beta.16</id>
    <link rel="alternate" type="text/html" href="https://github.com/mxsail/velowork/releases/tag/v0.1.0-beta.16"/>
    <title>v0.1.0-beta.16</title>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/12345/v0.1.0-beta.15</id>
    <link rel="alternate" type="text/html" href="https://github.com/mxsail/velowork/releases/tag/v0.1.0-beta.15"/>
    <title>v0.1.0-beta.15</title>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/12345/v0.1.0</id>
    <link rel="alternate" type="text/html" href="https://github.com/mxsail/velowork/releases/tag/v0.1.0"/>
    <title>v0.1.0</title>
  </entry>
</feed>"#;

        let tags = extract_tags_from_atom(sample_atom);
        assert_eq!(tags, vec!["v0.1.0-beta.16", "v0.1.0-beta.15", "v0.1.0"]);
    }

    #[test]
    fn test_platform_asset_name_not_empty() {
        let name = platform_asset_name();
        assert!(!name.is_empty());
        assert!(name.starts_with("velowork-"));
    }

    #[test]
    fn test_version_parsing_with_prefix() {
        let tag = "v0.1.0-beta.2";
        let remote_str = tag.strip_prefix('v').unwrap_or(tag);
        let parsed = Version::parse(remote_str);
        assert!(parsed.is_ok());

        let current = Version::parse("0.1.0-beta.1").unwrap();
        assert!(parsed.unwrap() > current);
    }

    #[test]
    fn test_semver_pre_release_comparison() {
        let current_beta15 = Version::parse("0.1.0-beta.15").unwrap();
        let new_beta16 = Version::parse("0.1.0-beta.16").unwrap();
        let stable = Version::parse("0.1.0").unwrap();
        let old_beta14 = Version::parse("0.1.0-beta.14").unwrap();

        // New beta is higher than old beta
        assert!(new_beta16 > current_beta15);
        // Stable is higher than any pre-release of the same version
        assert!(stable > current_beta15);
        assert!(stable > new_beta16);
        // Old beta is lower
        assert!(old_beta14 < current_beta15);

        // Pre-release detection
        assert!(!current_beta15.pre.is_empty());
        assert!(stable.pre.is_empty());
    }
}
