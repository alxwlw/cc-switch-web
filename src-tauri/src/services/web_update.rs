//! Update check of the headless web server (WEB-28). This step only knows which
//! commit the running binary was built from.

/// The commit the binary was built from, set at build time through
/// `CC_SWITCH_WEB_GIT_SHA` (`scripts/install-server.sh`, the `Dockerfile.web`
/// build argument). `None` for a build that cannot be tied to a commit.
pub fn build_sha() -> Option<&'static str> {
    normalize_sha(option_env!("CC_SWITCH_WEB_GIT_SHA"))
}

/// An empty value is "not set": the Docker `ARG` defaults to an empty string and
/// `option_env!` then yields `Some("")`.
fn normalize_sha(raw: Option<&'static str>) -> Option<&'static str> {
    raw.map(str::trim).filter(|sha| !sha.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_value_is_none() {
        assert_eq!(normalize_sha(None), None);
    }

    #[test]
    fn empty_and_blank_values_are_none() {
        assert_eq!(normalize_sha(Some("")), None);
        assert_eq!(normalize_sha(Some("  \n")), None);
    }

    #[test]
    fn a_sha_is_kept_without_surrounding_whitespace() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(normalize_sha(Some(sha)), Some(sha));
        assert_eq!(normalize_sha(Some("abc1234\n")), Some("abc1234"));
    }
}
