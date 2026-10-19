use std::cmp::Ordering;
use std::fmt;

/// A release version: `1.2.3`, optionally with a pre-release such as `1.2.3-rc.1`. A leading
/// `v` is accepted and build metadata (`+…`) is ignored. Ordered by semver precedence, so
/// `1.2.3-rc.1 < 1.2.3 < 1.2.4`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    /// Major, minor and patch.
    pub core: (u64, u64, u64),
    /// Dot-separated pre-release identifiers; empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// Reads a version; `None` when it isn't one. Missing minor and patch count as `0`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        if s.is_empty() || s.len() > 128 {
            return None;
        }
        let s = s.split('+').next().unwrap_or(s);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let number = |p: Option<&str>, required: bool| -> Option<u64> {
            match p {
                Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok(),
                Some(_) => None,
                None if required => None,
                None => Some(0),
            }
        };
        let major = number(parts.next(), true)?;
        let minor = number(parts.next(), false)?;
        let patch = number(parts.next(), false)?;
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_owned).collect();
                if ids.iter().any(|i| i.is_empty() || !i.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
                    return None;
                }
                ids
            }
        };
        Some(Version { core: (major, minor, patch), pre })
    }

    /// Whether this is a pre-release.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (a, b, c) = self.core;
        write!(f, "{a}.{b}.{c}")?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_order_by_semver_precedence() {
        let ordered =
            ["0.9.9", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "v1.0.1", "1.10.0"];
        for pair in ordered.windows(2) {
            if let [a, b] = pair {
                assert!(v(a) < v(b), "{a} < {b}");
            }
        }
        assert_eq!(v("v1.2"), v("1.2.0"));
        assert_eq!(v("1.2.3+build.7"), v("1.2.3"));
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
    }

    #[test]
    fn hostile_versions_are_refused() {
        for bad in ["", "v", "1..2", "1.2.3.4", "x.1", "1.2.3-", "1.2.3-a..b", "1.2.-3", "99999999999999999999999.0.0", "1.2.3-é", &"9".repeat(200)]
        {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
    }
}
