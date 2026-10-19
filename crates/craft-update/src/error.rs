use std::fmt;

/// Why an update step failed. The text is written for people: apps show it as is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// GitHub couldn't be reached or answered with an error.
    Network(String),
    /// GitHub's answer or a downloaded file isn't what a release looks like.
    Invalid(String),
    /// A signature or checksum didn't match: the download is not used.
    Verify(String),
    /// This copy can't update itself (how it was installed, or the platform).
    Unsupported(String),
    /// Installing failed.
    Install(String),
    /// The person cancelled.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(m) => write!(f, "couldn't reach the update server ({m})"),
            Error::Invalid(m) => write!(f, "the update server's answer isn't usable ({m})"),
            Error::Verify(m) => write!(f, "the download failed its integrity check ({m})"),
            Error::Unsupported(m) | Error::Install(m) => f.write_str(m),
            Error::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for Error {}
