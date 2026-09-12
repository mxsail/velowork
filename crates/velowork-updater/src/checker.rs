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
pub async fn check_for_update(app_version: String) -> Result<Option<ReleaseAsset>> {
    smol::unblock(move || check_blocking(&app_version)).await
}

fn check_blocking(app_version: &str) -> Result<Option<ReleaseAsset>> {
    // 1. Primary attempt: probe web redirect at https://github.com/mxsail/velowork/releases/latest.
    // This endpoint has NO GitHub REST API rate limit (no 60 req/hr per IP restriction).
    match check_via_web_redirect(app_version) {
        Ok(asset) => return Ok(asset),
        Err(e) => {
            log::warn!(
                "[updater] Web redirect check failed, falling back to GitHub API | error: {:#}",
                e
            );
        }
    }

    // 2. Fallback: GitHub REST API
    check_via_api(app_version)
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

fn check_via_web_redirect(app_version: &str) -> Result<Option<ReleaseAsset>> {
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

    if remote_version <= current_version {
        log::info!(
            "No update available (current={}, latest={})",
            current_version,
            remote_version
        );
        return Ok(None);
    }

    log::info!(
        "Update available: {} -> {}",
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

fn check_via_api(app_version: &str) -> Result<Option<ReleaseAsset>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to create http client")?;

    let http_resp = client
        .get("https://api.github.com/repos/mxsail/velowork/releases/latest")
        .header(reqwest::header::USER_AGENT, format!("velowork/{}", app_version))
        .send()
        .context("failed to fetch latest release")?;

    let status = http_resp.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("GitHub API rate limit exceeded — try again later");
    }
    if !status.is_success() {
        anyhow::bail!("GitHub API returned status {}", status);
    }

    let resp: serde_json::Value = http_resp.json().context("failed to parse release JSON")?;

    let tag = resp["tag_name"].as_str().context("missing tag_name")?;

    let remote_version_str = tag.strip_prefix('v').unwrap_or(tag);
    let remote_version = Version::parse(remote_version_str).context("invalid remote version")?;

    let current_version = Version::parse(app_version).context("invalid current version")?;

    if remote_version <= current_version {
        log::info!(
            "No update available (current={}, latest={})",
            current_version,
            remote_version
        );
        return Ok(None);
    }

    log::info!(
        "Update available: {} -> {}",
        current_version,
        remote_version
    );

    let expected_asset = platform_asset_name();
    let assets = resp["assets"].as_array().context("missing assets array")?;

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
            version: remote_version.to_string(),
            asset_url,
            asset_name,
            checksum_url,
        }));
    }

    log::warn!(
        "Release {} exists but no matching asset '{}' found",
        remote_version,
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
}
