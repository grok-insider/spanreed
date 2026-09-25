//! Argument helpers shared by the subcommands.

/// The value after `flag` (`--interval 60`).
pub(super) fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

pub(super) fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// The first argument that is not a `--flag`.
pub(super) fn positional(args: &[String]) -> Option<&str> {
    args.iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn flags_values_and_positionals() {
        let a = args(&["--force", "grok", "--interval", "30"]);
        assert!(has(&a, "--force"));
        assert_eq!(value(&a, "--interval"), Some("30"));
        assert_eq!(value(&a, "--missing"), None);
        assert_eq!(positional(&a), Some("grok"));
        assert_eq!(positional(&args(&["--all"])), None);
    }
}
