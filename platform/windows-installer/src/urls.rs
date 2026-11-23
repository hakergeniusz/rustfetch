//! Pure URL/tag construction for the release assets.
//!
//! Kept target-independent so it can be unit-tested on Linux; the Windows FFI
//! layer only needs `HOST` plus the path portion for `WinHttpOpenRequest`.

pub const HOST: &str = "github.com";
pub const REPO_OWNER: &str = "hakergeniusz";
pub const REPO_NAME: &str = "rustfetch";
pub const ASSET_NAME: &str = "rustfetch-windows-x86_64.exe";

/// Normalise a user-supplied version to a canonical release tag (`vX.Y.Z`).
///
/// Accepts `v0.2.5`, `0.2.5`, `V0.2.5` (and surrounding whitespace), all of
/// which become `v0.2.5`. `None`/blank stays `None` (meaning "latest").
pub fn normalize_tag(version: Option<&str>) -> Option<String> {
    version.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let without_v = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        if without_v.is_empty() {
            return None;
        }
        Some(format!("v{}", without_v))
    })
}

/// Path portion of the GitHub releases download URL.
pub fn release_base_path(version: Option<&str>) -> String {
    match normalize_tag(version) {
        Some(tag) => format!("/{}/{}/releases/download/{}", REPO_OWNER, REPO_NAME, tag),
        None => format!("/{}/{}/releases/latest/download", REPO_OWNER, REPO_NAME),
    }
}

/// Full HTTPS URL for an asset, used for human-readable step messages.
pub fn asset_url(version: Option<&str>, asset: &str) -> String {
    format!("https://{}{}/{}", HOST, release_base_path(version), asset)
}

#[cfg(test)]
mod tests {
    use super::{asset_url, normalize_tag, release_base_path, ASSET_NAME};

    #[test]
    fn normalizes_tags() {
        assert_eq!(normalize_tag(None), None);
        assert_eq!(normalize_tag(Some("v0.2.5")).as_deref(), Some("v0.2.5"));
        assert_eq!(normalize_tag(Some("0.2.5")).as_deref(), Some("v0.2.5"));
        assert_eq!(normalize_tag(Some("V0.2.5")).as_deref(), Some("v0.2.5"));
        assert_eq!(normalize_tag(Some("  v1.2.3  ")).as_deref(), Some("v1.2.3"));
        assert_eq!(normalize_tag(Some("   ")), None);
    }

    #[test]
    fn builds_release_paths() {
        assert_eq!(
            release_base_path(None),
            "/hakergeniusz/rustfetch/releases/latest/download"
        );
        assert_eq!(
            release_base_path(Some("0.2.8")),
            "/hakergeniusz/rustfetch/releases/download/v0.2.8"
        );
    }

    #[test]
    fn builds_asset_and_sidecar_urls() {
        assert_eq!(
            asset_url(None, ASSET_NAME),
            "https://github.com/hakergeniusz/rustfetch/releases/latest/download/rustfetch-windows-x86_64.exe"
        );
        assert_eq!(
            asset_url(Some("v1.0.0"), &format!("{}.sha256", ASSET_NAME)),
            "https://github.com/hakergeniusz/rustfetch/releases/download/v1.0.0/rustfetch-windows-x86_64.exe.sha256"
        );
    }
}
