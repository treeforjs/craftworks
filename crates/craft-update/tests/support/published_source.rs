use std::io::Read;
use std::path::PathBuf;

use craft_update::{Error, GitHub, Source};

/// CI snapshots only the four public API feeds through its authenticated request.
/// Every checksum, signature and payload still comes through the real HTTPS source.
pub struct PublishedSource {
    github: GitHub,
    directory: Option<PathBuf>,
}

impl PublishedSource {
    pub fn new(user_agent: &str) -> Self {
        Self { github: GitHub::new(user_agent), directory: std::env::var_os("CRAFT_RELEASE_FEEDS").map(PathBuf::from) }
    }
}

fn feed_name(url: &str) -> Option<&'static str> {
    match url {
        "https://api.github.com/repos/treeforjs/craftworks/releases?per_page=20" => Some("craftworks.json"),
        "https://api.github.com/repos/treeforjs/pdfcraft/releases?per_page=100" => Some("pdfcraft.json"),
        "https://api.github.com/repos/treeforjs/vectorcraft/releases?per_page=100" => Some("vectorcraft.json"),
        "https://api.github.com/repos/treeforjs/photocraft/releases?per_page=100" => Some("photocraft.json"),
        _ => None,
    }
}

impl Source for PublishedSource {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, Error> {
        if let (Some(directory), Some(name)) = (&self.directory, feed_name(url)) {
            let mut bytes = Vec::new();
            std::fs::File::open(directory.join(name))
                .and_then(|file| file.take(limit.saturating_add(1)).read_to_end(&mut bytes))
                .map_err(|error| Error::Network(format!("can't read CI release feed: {error}")))?;
            if bytes.len() as u64 > limit {
                return Err(Error::Invalid("CI release feed is larger than its allowed size".into()));
            }
            return Ok(bytes);
        }
        self.github.get(url, limit)
    }

    fn download(&self, url: &str, limit: u64, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        self.github.download(url, limit, sink)
    }
}

#[test]
fn only_exact_release_feed_urls_can_use_the_snapshot() {
    assert_eq!(feed_name("https://api.github.com/repos/treeforjs/craftworks/releases?per_page=20"), Some("craftworks.json"));
    assert_eq!(feed_name("https://api.github.com/repos/treeforjs/pdfcraft/releases?per_page=100"), Some("pdfcraft.json"));
    assert_eq!(feed_name("https://api.github.com/repos/other/pdfcraft/releases?per_page=100"), None);
    assert_eq!(feed_name("https://github.com/treeforjs/pdfcraft/releases/download/v1/SHA256SUMS.txt"), None);
    assert_eq!(feed_name("https://api.github.com.evil.invalid/repos/treeforjs/craftworks/releases?per_page=20"), None);
}

#[test]
#[allow(clippy::unwrap_used)]
fn release_feed_snapshots_keep_the_callers_byte_limit() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("pdfcraft.json"), b"[]").unwrap();
    let source = PublishedSource { github: GitHub::new("fixture"), directory: Some(directory.path().to_owned()) };
    let url = "https://api.github.com/repos/treeforjs/pdfcraft/releases?per_page=100";
    assert_eq!(source.get(url, 2).unwrap(), b"[]");
    assert!(matches!(source.get(url, 1), Err(Error::Invalid(_))));
}
