//! Is a GitHub release newer than this build?

/// True when `tag` (like "v0.3.0") is a newer version than `current` (like "0.2.0").
pub fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect() };
    matches!((parse(tag), parse(current)), (Some(t), Some(c)) if t > c)
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_numbers_not_text() {
        assert!(is_newer("v0.10.0", "0.9.1"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.9", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
    }
}
