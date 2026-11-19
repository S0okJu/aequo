use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

pub const CHECK_INTERVAL: i64 = 6 * 60 * 60;
pub const MAX_MANIFEST: usize = 256 * 1024;
pub const MAX_PACKAGE: usize = 256 * 1024 * 1024;
pub const REPOSITORY: &str = match option_env!("AEQUO_UPDATE_REPOSITORY") {
    Some(value) if cfg!(feature = "updater-test") => value,
    _ => "S0okJu/aequo",
};
pub const ENDPOINT: &str = match option_env!("AEQUO_UPDATE_ENDPOINT") {
    Some(value) if cfg!(feature = "updater-test") => value,
    _ => "https://github.com/S0okJu/aequo/releases/latest/download/latest.json",
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub release_url: String,
}

pub fn validate_manifest(
    raw: &serde_json::Value,
    current: &str,
) -> Result<(Release, bool), String> {
    let version = raw["version"].as_str().ok_or("metadata")?;
    let version =
        Version::parse(version.strip_prefix('v').unwrap_or(version)).map_err(|_| "metadata")?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("metadata".into());
    }
    let platform = &raw["platforms"]["darwin-aarch64"];
    let url = platform["url"].as_str().ok_or("metadata")?;
    let parsed = Url::parse(url).map_err(|_| "metadata")?;
    let prefix = format!("/{REPOSITORY}/releases/download/v{version}/");
    let test_origin = cfg!(feature = "updater-test")
        && Url::parse(ENDPOINT).is_ok_and(|endpoint| parsed.origin() == endpoint.origin());
    if (!test_origin && (parsed.scheme() != "https" || parsed.host_str() != Some("github.com")))
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.path().starts_with(&prefix)
        || !parsed.path().ends_with(".app.tar.gz")
        || parsed.path()[prefix.len()..].contains('/')
    {
        return Err("metadata".into());
    }
    let signature = platform["signature"].as_str().ok_or("metadata")?;
    if signature.is_empty() || signature.len() > 8192 {
        return Err("metadata".into());
    }
    let date = match raw.get("pub_date") {
        Some(value) => {
            let date = value.as_str().ok_or("metadata")?;
            chrono::DateTime::parse_from_rfc3339(date).map_err(|_| "metadata")?;
            Some(date.to_string())
        }
        None => None,
    };
    let notes = match raw.get("notes") {
        Some(value) => value.as_str().ok_or("metadata")?,
        None => "",
    };
    let current = Version::parse(current).map_err(|_| "metadata")?;
    Ok((
        Release {
            version: version.to_string(),
            notes: notes.chars().take(8000).collect(),
            published_at: date,
            release_url: format!("https://github.com/{REPOSITORY}/releases/tag/v{version}"),
        },
        version > current,
    ))
}

pub fn retry_delay(attempt: usize, retry_after: Option<i64>) -> i64 {
    let backoff = [60, 300, 1800]
        .get(attempt)
        .copied()
        .unwrap_or(CHECK_INTERVAL);
    backoff.max(retry_after.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(version: &str) -> serde_json::Value {
        serde_json::json!({"version":version, "platforms":{"darwin-aarch64":{
            "url":format!("https://github.com/{REPOSITORY}/releases/download/v{version}/Aequo.app.tar.gz"),
            "signature":"signature"
        }}})
    }
    #[test]
    fn stable_semver_and_platform_are_required_even_for_old_releases() {
        assert!(validate_manifest(&manifest("1.0.10"), "1.0.9").unwrap().1);
        assert!(!validate_manifest(&manifest("1.0.1"), "1.0.1").unwrap().1);
        assert!(!validate_manifest(&manifest("1.0.0"), "1.0.1").unwrap().1);
        for version in ["1.0.2-beta.1", "1.0.2+build", "banana"] {
            assert!(validate_manifest(&manifest(version), "1.0.1").is_err());
        }
        let mut raw = manifest("1.0.0");
        raw["platforms"] = serde_json::json!({});
        assert!(validate_manifest(&raw, "1.0.1").is_err());
    }
    #[test]
    fn arbitrary_asset_urls_and_invalid_dates_are_rejected() {
        for url in [
            "http://github.com/S0okJu/aequo/releases/download/v1.0.2/A.app.tar.gz",
            "https://evil.test/A.app.tar.gz",
            "https://github.com/other/repo/releases/download/v1.0.2/A.app.tar.gz",
            "https://github.com/S0okJu/aequo/releases/download/v1.0.1/A.app.tar.gz",
        ] {
            let mut raw = manifest("1.0.2");
            raw["platforms"]["darwin-aarch64"]["url"] = url.into();
            assert!(validate_manifest(&raw, "1.0.1").is_err());
        }
        let mut raw = manifest("1.0.2");
        raw["pub_date"] = "yesterday".into();
        assert!(validate_manifest(&raw, "1.0.1").is_err());
    }
    #[test]
    fn retries_are_bounded_and_honor_longer_server_delay() {
        assert_eq!(
            [0, 1, 2, 3].map(|n| retry_delay(n, None)),
            [60, 300, 1800, 21600]
        );
        assert_eq!(retry_delay(0, Some(600)), 600);
    }
}
