use serde::{Deserialize, Serialize};

/// When to look for updates in the background. Apps keep this in their settings.
///
/// Starting the app never waits on it: the app asks [`AutoCheck::due`] once it is up and, if a
/// check is due, runs it on a background thread after [`AutoCheck::START_DELAY_SECS`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoCheck {
    /// Look for updates automatically (Settings ▸ Updates). Off until the user opts in.
    pub enabled: bool,
    /// When the last check finished, in seconds since 1970.
    pub last_checked: Option<u64>,
    /// A version the person chose to skip: it isn't offered again (a newer one is).
    pub skipped: Option<String>,
}

impl AutoCheck {
    /// How often to look: once a day.
    pub const INTERVAL_SECS: u64 = 24 * 60 * 60;
    /// How long after start to look, so the network never competes with opening the app.
    pub const START_DELAY_SECS: u64 = 10;

    /// Whether a background check is due at `now` (seconds since 1970). A clock that went
    /// backwards counts as due, so a wrong date can't stop checks for good.
    pub fn due(&self, now: u64) -> bool {
        self.enabled
            && match self.last_checked {
                None => true,
                Some(t) => t > now || now.saturating_sub(t) >= Self::INTERVAL_SECS,
            }
    }

    /// Whether the person skipped `version`.
    pub fn is_skipped(&self, version: &crate::Version) -> bool {
        self.skipped.as_deref().and_then(crate::Version::parse).is_some_and(|s| &s == version)
    }

    /// Now, in seconds since 1970 (0 if the clock is before 1970).
    pub fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_once_a_day() {
        let mut a = AutoCheck { enabled: true, ..AutoCheck::default() };
        assert!(a.due(1_000));
        a.last_checked = Some(1_000);
        assert!(!a.due(1_000 + AutoCheck::INTERVAL_SECS - 1));
        assert!(a.due(1_000 + AutoCheck::INTERVAL_SECS));
        assert!(a.due(10), "a clock that went backwards");
        a.enabled = false;
        assert!(!a.due(u64::MAX));
    }

    #[test]
    fn background_checks_require_an_explicit_choice() {
        for json in ["{}", r#"{"last_checked":1}"#, r#"{"enabled":false}"#] {
            let saved: AutoCheck = serde_json::from_str(json).unwrap();
            assert!(!saved.enabled, "{json}");
            assert!(!saved.due(u64::MAX), "{json}");
        }
        assert!(!AutoCheck::default().due(u64::MAX));
        let enabled: AutoCheck = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(enabled.due(1_000));
        assert_eq!(serde_json::from_str::<AutoCheck>(&serde_json::to_string(&enabled).unwrap()).unwrap(), enabled);
    }

    #[test]
    fn skipped_versions_and_old_settings_are_read() {
        let a: AutoCheck = serde_json::from_str(r#"{"skipped":"v1.2.0"}"#).unwrap();
        assert!(!a.enabled);
        assert!(!a.due(u64::MAX));
        assert!(a.is_skipped(&crate::Version::parse("1.2.0").unwrap()));
        assert!(!a.is_skipped(&crate::Version::parse("1.2.1").unwrap()));
    }
}
