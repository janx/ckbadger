pub fn format_build_version(semver: &str, branch_name: &str, commit_hash: &str) -> String {
    if branch_name == "main" {
        format!("{semver}@{commit_hash}")
    } else {
        format!("{semver}+{branch_name}@{commit_hash}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_build_version;

    #[test]
    fn omits_main_branch_label() {
        assert_eq!(
            format_build_version("0.1.0", "main", "abcdef123456"),
            "0.1.0@abcdef123456"
        );
    }

    #[test]
    fn keeps_non_main_branch_label_verbatim() {
        assert_eq!(
            format_build_version("0.1.0", "feature/foo", "abcdef123456"),
            "0.1.0+feature/foo@abcdef123456"
        );
    }
}
