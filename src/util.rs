//! Small shared helpers.

/// Parse an operator-supplied boolean the forgiving way.
///
/// An operator who writes `GRAPH_COMPRESSION=false` means it, and silently treating that as
/// "on" because the code only checked for `"0"` is a confusing way to disagree with them.
pub fn truthy(raw: &str) -> bool {
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no" | ""
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_accepts_the_spellings_an_operator_would_actually_type() {
        for on in ["1", "true", "TRUE", "yes", "on", "anything-else"] {
            assert!(truthy(on), "{on:?} should be truthy");
        }
        for off in ["0", "false", "FALSE", "off", "no", "", "  "] {
            assert!(!truthy(off), "{off:?} should be falsey");
        }
    }
}
