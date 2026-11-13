//! Updates: is a GitHub release newer than this build, where are its files, and is a download intact.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The release asset that is the app, and the checksum file next to it (written by the release workflow).
pub const EXE: &str = "Potato.exe";
const SUMS: &str = "SHA256SUMS.txt";

/// A newer release: its tag and where to download the app and its checksums.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub exe_url: String,
    pub sums_url: String,
}

/// True when `tag` (like "v0.3.0") is a newer version than `current` (like "0.2.0").
pub fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect() };
    matches!((parse(tag), parse(current)), (Some(t), Some(c)) if t > c)
}

/// The release in GitHub's "latest release" answer, if it's newer than `current` and has both files.
pub fn newer_release(json: &Value, current: &str) -> Option<Release> {
    let tag = json["tag_name"].as_str()?;
    if !is_newer(tag, current) {
        return None;
    }
    let url = |name: &str| json["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"].as_str().map(String::from);
    Some(Release { tag: tag.to_string(), exe_url: url(EXE)?, sums_url: url(SUMS)? })
}

/// Whether `bytes` match the SHA-256 the checksum file lists for the app (`<hex>  Potato.exe` lines).
pub fn matches_checksum(bytes: &[u8], sums: &str) -> bool {
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    sums.lines().any(|l| {
        let mut parts = l.split_whitespace();
        parts.next().is_some_and(|h| h.eq_ignore_ascii_case(&hex)) && parts.next().map(|n| n.trim_start_matches('*')) == Some(EXE)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compares_numbers_not_text() {
        assert!(is_newer("v0.10.0", "0.9.1"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.9", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
    }

    #[test]
    fn reads_the_release_files() {
        let j = json!({"tag_name": "v0.3.0", "assets": [
            {"name": "SHA256SUMS.txt", "browser_download_url": "https://x/sums"},
            {"name": "Potato.exe", "browser_download_url": "https://x/exe"}]});
        assert_eq!(newer_release(&j, "0.2.1"), Some(Release { tag: "v0.3.0".into(), exe_url: "https://x/exe".into(), sums_url: "https://x/sums".into() }));
        assert_eq!(newer_release(&j, "0.3.0"), None);   // not newer
        let no_sums = json!({"tag_name": "v0.3.0", "assets": [{"name": "Potato.exe", "browser_download_url": "https://x/exe"}]});
        assert_eq!(newer_release(&no_sums, "0.2.1"), None);   // can't be checked, so not offered
    }

    #[test]
    fn checksum_must_match_the_app_line() {
        let bytes = b"potato";
        let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert!(matches_checksum(bytes, &format!("{hex}  Potato.exe\n")));
        assert!(matches_checksum(bytes, &format!("{}  *Potato.exe", hex.to_uppercase())));
        assert!(!matches_checksum(b"tampered", &format!("{hex}  Potato.exe")));
        assert!(!matches_checksum(bytes, &format!("{hex}  Other.exe")));
        assert!(!matches_checksum(bytes, ""));
    }
}
