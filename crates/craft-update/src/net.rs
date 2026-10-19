#[cfg(feature = "github")]
use std::io::Read;
#[cfg(feature = "github")]
use std::time::Duration;

use crate::Error;

/// Where release information and downloads come from. [`GitHub`] in the app; tests use a fake.
pub trait Source: Send + Sync {
    /// Fetches `url` whole, refusing answers over `limit` bytes.
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, Error>;

    /// Streams `url` into `sink` chunk by chunk, refusing more than `limit` bytes. A `sink`
    /// error (such as a cancel) stops the download.
    fn download(&self, url: &str, limit: u64, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error>;
}

/// GitHub over HTTPS, trusting the certificate authorities the operating system trusts.
#[cfg(feature = "github")]
pub struct GitHub {
    user_agent: String,
}

#[cfg(feature = "github")]
impl GitHub {
    /// A source that identifies itself as `user_agent` (such as `PdfCraft/1.2.0`).
    pub fn new(user_agent: impl Into<String>) -> GitHub {
        GitHub { user_agent: user_agent.into() }
    }

    fn agent(&self, timeout: Option<Duration>) -> Result<ureq::Agent, Error> {
        let found = rustls_native_certs::load_native_certs();
        let certs: Vec<ureq::tls::Certificate<'static>> =
            found.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
        if certs.is_empty() {
            return Err(Error::Network("no trusted certificates found on this system".into()));
        }
        Ok(ureq::Agent::config_builder()
            .timeout_global(timeout)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::new_with_certs(&certs)).build())
            .build()
            .new_agent())
    }

    fn call(&self, agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>, Error> {
        if !url.starts_with("https://") {
            return Err(Error::Invalid(format!("refusing a non-HTTPS address: {url}")));
        }
        agent
            .get(url)
            .header("Accept", if url.starts_with("https://api.github.com/") { "application/vnd.github+json" } else { "application/octet-stream" })
            .header("User-Agent", &self.user_agent)
            .call()
            .map_err(|e| Error::Network(e.to_string()))
    }
}

#[cfg(feature = "github")]
impl Source for GitHub {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, Error> {
        let agent = self.agent(Some(Duration::from_secs(20)))?;
        let mut response = self.call(&agent, url)?;
        response.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| Error::Network(e.to_string()))
    }

    fn download(&self, url: &str, limit: u64, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        // No overall timeout: a large installer on a slow line takes as long as it takes; a
        // stalled connection still ends through the receive timeout.
        let agent = self.agent(None)?;
        let mut response = self.call(&agent, url)?;
        let mut reader = response.body_mut().with_config().limit(limit).reader();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| Error::Network(e.to_string()))?;
            if n == 0 {
                return Ok(());
            }
            sink(buf.get(..n).unwrap_or_default())?;
        }
    }
}

#[cfg(all(test, feature = "github"))]
mod tests {
    use super::*;

    /// Live: asks GitHub over TLS with the OS's roots (`cargo test -p craft-update -- --ignored`).
    #[test]
    #[ignore = "needs network access"]
    fn github_answers_with_a_release_list() {
        let body = GitHub::new("craft-update-test").get("https://api.github.com/repos/treeforjs/pdfcraft/releases?per_page=5", 4 << 20).unwrap();
        assert!(body.starts_with(b"["), "{}", String::from_utf8_lossy(&body));
        assert!(matches!(GitHub::new("x").get("http://api.github.com/", 10), Err(Error::Invalid(_))));
        // Release downloads redirect to GitHub's object storage; the redirect is followed.
        let mut got = Vec::new();
        GitHub::new("craft-update-test")
            .download("https://github.com/goreleaser/nfpm/releases/download/v2.47.0/checksums.txt", 1 << 20, &mut |c| {
                got.extend_from_slice(c);
                Ok(())
            })
            .unwrap();
        assert!(String::from_utf8_lossy(&got).contains("nfpm_2.47.0"), "{}", String::from_utf8_lossy(&got));
    }
}
