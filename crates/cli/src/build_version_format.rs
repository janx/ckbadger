/// Format the build version string.
///
/// - `main` branch or detached HEAD (CI tag checkout): `0.1.0@abcdef123456`
/// - Other branches: `0.1.0+feature/foo@abcdef123456`
pub fn format_build_version(semver: &str, branch_name: Option<&str>, commit_hash: &str) -> String {
    match branch_name {
        Some("main") | None => format!("{semver}@{commit_hash}"),
        Some(branch) => format!("{semver}+{branch}@{commit_hash}"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_build_version;

    #[test]
    fn omits_main_branch_label() {
        assert_eq!(
            format_build_version("0.1.0", Some("main"), "abcdef123456"),
            "0.1.0@abcdef123456"
        );
    }

    #[test]
    fn keeps_non_main_branch_label_verbatim() {
        assert_eq!(
            format_build_version("0.1.0", Some("feature/foo"), "abcdef123456"),
            "0.1.0+feature/foo@abcdef123456"
        );
    }

    #[test]
    fn detached_head_omits_label() {
        assert_eq!(
            format_build_version("0.1.0", None, "abcdef123456"),
            "0.1.0@abcdef123456"
        );
    }
}
