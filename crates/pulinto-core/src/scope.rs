//! Application scope shared by user-created rules.
//!
//! An empty scope means "all applications". Otherwise entries contain an
//! exact, case-insensitive `WM_CLASS`, or a prefix ending in `*` (for example
//! `jetbrains-*`). Keeping this tiny matcher in `pulinto-core` makes exact
//! replacements and never/always rules obey precisely the same semantics.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppScope {
    patterns: Vec<String>,
}

impl AppScope {
    pub fn global() -> Self {
        Self::default()
    }

    /// Parse a comma-separated list as accepted by the settings UI and D-Bus.
    pub fn parse(value: &str) -> Self {
        let mut patterns: Vec<String> = value
            .split(',')
            .map(|item| item.trim().to_lowercase())
            .filter(|item| !item.is_empty())
            .collect();
        patterns.sort();
        patterns.dedup();
        Self { patterns }
    }

    pub fn is_global(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn matches(&self, app_class: Option<&str>) -> bool {
        if self.is_global() {
            return true;
        }
        let Some(app_class) = app_class else {
            // A scoped rule must never unexpectedly become global when the
            // display server cannot tell us which window owns focus.
            return false;
        };
        let app_class = app_class.trim().to_lowercase();
        self.patterns.iter().any(|pattern| {
            pattern
                .strip_suffix('*')
                .map_or(pattern == &app_class, |prefix| app_class.starts_with(prefix))
        })
    }

    pub fn as_text(&self) -> String {
        self.patterns.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scope_is_global() {
        let scope = AppScope::parse("  ");
        assert!(scope.matches(None));
        assert!(scope.matches(Some("Navigator")));
    }

    #[test]
    fn exact_and_prefix_rules_are_case_insensitive() {
        let scope = AppScope::parse(" Navigator, jetbrains-*, navigator ");
        assert_eq!(scope.as_text(), "jetbrains-*,navigator");
        assert!(scope.matches(Some("NAVIGATOR")));
        assert!(scope.matches(Some("jetbrains-idea")));
        assert!(!scope.matches(Some("code")));
        assert!(!scope.matches(None));
    }
}
