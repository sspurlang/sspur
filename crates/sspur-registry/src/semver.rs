//! Versions `MAJOR.MINOR.PATCH` and requirements on them, as Cargo reads them: `^1.2` (the
//! default, also written `1.2`), `~1.2`, `=1.2.3`, `>=1.2, <2`, `*`. In a `0.x` version the
//! first non-zero component is the one that breaks compatibility.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn num(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 9 || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

impl Version {
    /// Exactly three numbers without leading zeros; pre-release and build suffixes are refused.
    pub fn parse(s: &str) -> Option<Version> {
        let mut it = s.trim().split('.');
        let v = Version { major: num(it.next()?)?, minor: num(it.next()?)?, patch: num(it.next()?)? };
        it.next().is_none().then_some(v)
    }

    /// The versions that may replace this one without breaking callers start with this key.
    pub fn compat(&self) -> (u64, u64, u64) {
        match (self.major, self.minor) {
            (0, 0) => (0, 0, self.patch),
            (0, m) => (0, m, 0),
            (m, _) => (m, 0, 0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Caret,
    Tilde,
    Exact,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pred {
    op: Op,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
}

/// A version requirement: a comma-separated list of predicates that must all hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Req {
    preds: Vec<Pred>,
    text: String,
}

impl fmt::Display for Req {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Req {
    pub fn any() -> Req {
        Req { preds: vec![], text: "*".into() }
    }

    pub fn parse(s: &str) -> Result<Req, String> {
        let text = s.trim().to_string();
        if text == "*" || text.is_empty() {
            return Ok(Req::any());
        }
        let mut preds = Vec::new();
        for part in text.split(',') {
            let p = part.trim();
            let (op, rest) = [(">=", Op::Ge), ("<=", Op::Le), (">", Op::Gt), ("<", Op::Lt), ("=", Op::Exact), ("^", Op::Caret), ("~", Op::Tilde)]
                .iter()
                .find_map(|(t, o)| p.strip_prefix(t).map(|r| (*o, r.trim())))
                .unwrap_or((Op::Caret, p));
            let bad = || format!("'{s}' is not a version requirement; write ^1.2, ~1.2.3, =1.2.3, >=1.2, <2 or *");
            let nums: Vec<&str> = rest.split('.').collect();
            if nums.is_empty() || nums.len() > 3 || nums.iter().any(|x| *x == "*" || *x == "x") && nums.len() == 1 {
                return Err(bad());
            }
            let n = |i: usize| -> Result<Option<u64>, String> {
                match nums.get(i) {
                    None => Ok(None),
                    Some(&("*" | "x")) => Ok(None),
                    Some(x) => num(x).map(Some).ok_or_else(bad),
                }
            };
            let major = n(0)?.ok_or_else(bad)?;
            let (minor, patch) = (n(1)?, n(2)?);
            if minor.is_none() && patch.is_some() {
                return Err(bad());
            }
            preds.push(Pred { op, major, minor, patch });
        }
        Ok(Req { preds, text })
    }

    pub fn matches(&self, v: &Version) -> bool {
        self.preds.iter().all(|p| p.matches(v))
    }

    /// The default requirement `sspur add` writes for a version: `^1.2.3`.
    pub fn caret(v: &Version) -> Req {
        Req::parse(&format!("^{v}")).unwrap()
    }
}

impl Pred {
    fn lower(&self) -> Version {
        Version { major: self.major, minor: self.minor.unwrap_or(0), patch: self.patch.unwrap_or(0) }
    }

    fn matches(&self, v: &Version) -> bool {
        let lo = self.lower();
        let cmp_prefix = || -> Ordering {
            let a = (v.major, self.minor.map(|_| v.minor), self.patch.map(|_| v.patch));
            let b = (self.major, self.minor, self.patch);
            a.cmp(&b)
        };
        match self.op {
            Op::Exact => cmp_prefix() == Ordering::Equal,
            Op::Gt => cmp_prefix() == Ordering::Greater,
            Op::Ge => cmp_prefix() != Ordering::Less,
            Op::Lt => cmp_prefix() == Ordering::Less,
            Op::Le => cmp_prefix() != Ordering::Greater,
            Op::Tilde => {
                v >= &lo
                    && match self.minor {
                        Some(m) => v.major == self.major && v.minor == m,
                        None => v.major == self.major,
                    }
            }
            Op::Caret => {
                if v < &lo {
                    return false;
                }
                match (self.major, self.minor, self.patch) {
                    (0, None, _) => v.major == 0,
                    (0, Some(0), None) => v.major == 0 && v.minor == 0,
                    (0, Some(0), Some(p)) => v.major == 0 && v.minor == 0 && v.patch == p,
                    (0, Some(m), _) => v.major == 0 && v.minor == m,
                    (m, _, _) => v.major == m,
                }
            }
        }
    }
}

/// The highest version in `vs` that every requirement accepts.
pub fn best<'a>(vs: impl IntoIterator<Item = &'a Version>, reqs: &[Req]) -> Option<Version> {
    vs.into_iter().filter(|v| reqs.iter().all(|r| r.matches(v))).max().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn m(r: &str, s: &str) -> bool {
        Req::parse(r).unwrap().matches(&v(s))
    }

    #[test]
    fn versions() {
        assert!(v("1.2.3") < v("1.10.0"));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("01.2.3"), None);
        assert_eq!(Version::parse("1.2.3-beta"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(v("0.3.1").compat(), (0, 3, 0));
        assert_eq!(v("0.0.4").compat(), (0, 0, 4));
        assert_eq!(v("2.9.1").compat(), (2, 0, 0));
    }

    #[test]
    fn requirements() {
        assert!(m("^1.2", "1.2.0") && m("^1.2", "1.9.9") && !m("^1.2", "2.0.0") && !m("^1.2", "1.1.9"));
        assert!(m("1.2", "1.4.0"));
        assert!(m("^0.2.3", "0.2.9") && !m("^0.2.3", "0.3.0") && !m("^0.2.3", "0.2.2"));
        assert!(m("^0.0.3", "0.0.3") && !m("^0.0.3", "0.0.4"));
        assert!(m("^0", "0.9.0") && !m("^0", "1.0.0"));
        assert!(m("~1.2.3", "1.2.9") && !m("~1.2.3", "1.3.0"));
        assert!(m("~1", "1.9.0") && !m("~1", "2.0.0"));
        assert!(m("=1.2.3", "1.2.3") && !m("=1.2.3", "1.2.4"));
        assert!(m("=1.2", "1.2.7") && !m("=1.2", "1.3.0"));
        assert!(m(">=1.2, <2", "1.5.0") && !m(">=1.2, <2", "2.0.0") && !m(">=1.2, <2", "1.1.0"));
        assert!(m(">1.2", "1.3.0") && !m(">1.2", "1.2.5"));
        assert!(m("<=1.2", "1.2.9") && !m("<=1.2", "1.3.0"));
        assert!(m("*", "0.0.1") && m("1.*", "1.5.0") && !m("1.*", "2.0.0"));
        for bad in ["^", "a.b", "1.2.3.4", "^1..2", "~>1.2", "1.*.3"] {
            assert!(Req::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn best_picks_highest_match() {
        let vs = [v("1.0.0"), v("1.4.2"), v("2.0.0"), v("1.3.9")];
        assert_eq!(best(&vs, &[Req::parse("^1.2").unwrap()]), Some(v("1.4.2")));
        assert_eq!(best(&vs, &[Req::parse("^1.2").unwrap(), Req::parse("<1.4").unwrap()]), Some(v("1.3.9")));
        assert_eq!(best(&vs, &[Req::parse("^3").unwrap()]), None);
    }
}
