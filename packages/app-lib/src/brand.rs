pub const PRODUCT_NAME: &str = "Axolotl Launcher";
pub const SHORT_PRODUCT_NAME: &str = "Axolotl";
pub const WEBSITE: &str = "https://www.ghs.red";
pub const BUNDLE_IDENTIFIER: &str = "red.ghs.axolotl";
pub const DEEP_LINK_SCHEME: &str = "axolotl";

/// Longest accepted data directory suffix; enough to name a branch or a build.
const MAX_DATA_DIR_SUFFIX_LEN: usize = 32;

pub fn user_agent(version: &str, os: &str) -> String {
    format!("garbage-human-studio/axolotl/{version} ({os})")
}

/// Name of the directory the launcher keeps its settings, databases and logs in.
///
/// A build can opt into its own directory by setting `AXOLOTL_DATA_DIR_SUFFIX`
/// at build time (see `build.rs`). Local and test builds use this to stay away
/// from the database of an installed launcher: the two otherwise share one
/// directory, and a database touched by a newer build can no longer be opened
/// by an older one. Releases leave the variable unset, so they resolve to the
/// plain identifier.
pub fn app_data_dir_identifier(app_identifier: &str) -> String {
    data_dir_identifier(app_identifier, option_env!("AXOLOTL_DATA_DIR_SUFFIX"))
}

/// Appends a sanitized suffix to the identifier.
///
/// The result becomes a directory name, so anything that could escape the data
/// directory (path separators, traversal) is dropped rather than escaped. A
/// suffix that leaves nothing usable behind is ignored entirely.
fn data_dir_identifier(base: &str, suffix: Option<&str>) -> String {
    let Some(suffix) = suffix else {
        return base.to_string();
    };

    let sanitized: String = suffix
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.')
        })
        .take(MAX_DATA_DIR_SUFFIX_LEN)
        .collect();
    let sanitized = sanitized.trim_matches(['.', '-']);

    if sanitized.is_empty() {
        base.to_string()
    } else {
        format!("{base}-{sanitized}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_is_unique_and_contains_no_contact_information() {
        let user_agent = user_agent("1.2.3", "windows");

        assert_eq!(user_agent, "garbage-human-studio/axolotl/1.2.3 (windows)");
        assert!(!user_agent.contains("ghs.red"));
        assert!(!user_agent.contains("http"));
        assert!(!user_agent.contains('@'));
    }

    #[test]
    fn data_dir_identifier_without_suffix_keeps_the_identifier() {
        assert_eq!(
            data_dir_identifier("red.ghs.axolotl", None),
            "red.ghs.axolotl"
        );
    }

    #[test]
    fn data_dir_identifier_appends_a_readable_suffix() {
        for (suffix, expected) in [
            ("pr538", "red.ghs.axolotl-pr538"),
            ("fix_issue_538", "red.ghs.axolotl-fix_issue_538"),
            ("v1.2.3", "red.ghs.axolotl-v1.2.3"),
        ] {
            assert_eq!(
                data_dir_identifier("red.ghs.axolotl", Some(suffix)),
                expected,
                "unexpected identifier for suffix {suffix:?}"
            );
        }
    }

    #[test]
    fn data_dir_identifier_never_escapes_the_data_directory() {
        for suffix in ["../evil", "..\\evil", "a/b", "a\\b", "..", "/", "\\"] {
            let identifier =
                data_dir_identifier("red.ghs.axolotl", Some(suffix));

            assert!(
                !identifier.contains('/') && !identifier.contains('\\'),
                "suffix {suffix:?} produced a path separator in {identifier:?}"
            );
            assert!(
                !identifier.contains(".."),
                "suffix {suffix:?} produced a parent reference in {identifier:?}"
            );
            assert!(
                identifier.starts_with("red.ghs.axolotl"),
                "suffix {suffix:?} lost the identifier prefix"
            );
        }
    }

    #[test]
    fn data_dir_identifier_ignores_unusable_suffixes() {
        for suffix in ["", "   ", "..", "--", "///", "..-.-"] {
            assert_eq!(
                data_dir_identifier("red.ghs.axolotl", Some(suffix)),
                "red.ghs.axolotl",
                "suffix {suffix:?} should have been ignored"
            );
        }
    }

    #[test]
    fn data_dir_identifier_bounds_the_suffix_length() {
        let identifier =
            data_dir_identifier("red.ghs.axolotl", Some(&"a".repeat(200)));

        assert_eq!(
            identifier,
            format!("red.ghs.axolotl-{}", "a".repeat(MAX_DATA_DIR_SUFFIX_LEN))
        );
    }
}
