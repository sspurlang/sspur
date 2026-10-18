//! Deprecated builtins (docs/reference/stability.md). A use keeps working through 1.x and warns
//! `W_DEPRECATED` with the replacement; the builtin can only go in 2.0, after at least one minor
//! release of warnings.

/// One deprecated builtin: `recv` is the receiver type of a method, or "" for a function.
pub struct Deprecated {
    pub recv: &'static str,
    pub name: &'static str,
    pub since: &'static str,
    pub instead: &'static str,
}

/// Nothing is deprecated in 1.0.
pub const DEPRECATED: &[Deprecated] = &[];

/// Whether there is anything to look up, so a build without deprecations does no lookups.
pub(crate) const ANY: bool = cfg!(test) || !DEPRECATED.is_empty();

/// Builtins that exist only in this crate's unit tests, so the mechanism has something to warn on.
#[cfg(test)]
pub(crate) static TEST_ONLY: &[(Deprecated, &str)] = &[
    (Deprecated { recv: "", name: "old_double", since: "1.0", instead: "double(n)" }, "old_double(n: Int) -> Int"),
    (Deprecated { recv: "Str", name: "old_size", since: "1.0", instead: "s.len" }, "old_size(s: Str) -> Int"),
];

#[inline]
pub(crate) fn find(recv: &str, name: &str) -> Option<&'static Deprecated> {
    let hit = |d: &&Deprecated| d.name == name && (d.recv == recv || d.recv == "*");
    #[cfg(test)]
    if let Some((d, _)) = TEST_ONLY.iter().find(|(d, _)| hit(&d)) {
        return Some(d);
    }
    DEPRECATED.iter().find(hit)
}

#[cfg(test)]
mod tests {
    fn diags(src: &str) -> Vec<crate::Diag> {
        crate::check(&sspur_syntax::parse(src).unwrap()).diags
    }

    #[test]
    fn a_deprecated_builtin_warns_with_its_replacement() {
        let d = diags("fn f(n: Int, s: Str) -> Int\n= old_double(n) + s.old_size\n\nfn g() -> List[Int]\n= [1].map(old_double)");
        let w: Vec<_> = d.iter().filter(|d| d.code == "W_DEPRECATED").collect();
        assert_eq!(w.len(), 3, "{d:?}");
        assert!(d.iter().all(|d| !d.is_error()), "{d:?}");
        assert_eq!(w[0].msg, "'old_double' is deprecated since 1.0 and goes in 2.0");
        assert_eq!(w[0].hint.as_deref(), Some("write double(n)"));
        assert_eq!(w[1].msg, "'.old_size' is deprecated since 1.0 and goes in 2.0");
        assert_eq!(w[1].hint.as_deref(), Some("write s.len"));
        assert_eq!(w[2].def.as_deref(), Some("g"));
    }

    #[test]
    fn a_definition_of_the_same_name_does_not_warn() {
        let d = diags("fn old_double(n: Int) -> Int\n= n * 2\n\nfn f(n: Int) -> Int\n= old_double(n)");
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn nothing_is_deprecated_in_1_0() {
        assert!(super::DEPRECATED.is_empty());
    }
}
