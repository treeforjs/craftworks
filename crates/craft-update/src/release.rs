use serde::Deserialize;

use crate::{AppInfo, Error, Version};

/// The largest asset the updater will download (installers are tens of MB).
pub const MAX_ASSET_BYTES: u64 = 2 << 30;

/// A published release of the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version, from the tag.
    pub version: Version,
    /// The tag as written, such as `v1.2.0`.
    pub tag: String,
    /// Its page under the app's releases page.
    pub page: String,
    /// The release notes (Markdown), capped.
    pub notes: String,
    /// Its downloads. Only files whose link points into the app's own repository are kept.
    pub assets: Vec<Asset>,
}

/// One download of a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// The file name, such as `pdfcraft-1.2.0-windows-x64.msi`. Never contains a path separator.
    pub name: String,
    /// Where to download it.
    pub url: String,
    /// Its size in bytes, as GitHub reports it.
    pub size: u64,
}

impl Release {
    /// The asset with this exact name.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Deserialize)]
struct RawRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Whether `name` is a plain file name (no directories, no tricks).
pub(crate) fn plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

fn read(raw: RawRelease, app: &AppInfo) -> Option<Release> {
    if raw.draft || raw.prerelease && !app.prereleases {
        return None;
    }
    let version = Version::parse(&raw.tag_name)?;
    if version.is_prerelease() && !app.prereleases {
        return None;
    }
    let releases = app.releases_page();
    let page = raw
        .html_url
        .strip_prefix(&releases)
        .filter(|rest| rest.starts_with('/') && !rest.contains(['?', '#', '\\']))
        .map_or(releases.clone(), |_| raw.html_url.clone());
    let downloads = format!("https://github.com/{}/releases/download/{}/", app.repo, raw.tag_name);
    let assets = raw
        .assets
        .into_iter()
        .filter(|a| plain_file_name(&a.name) && a.size <= MAX_ASSET_BYTES)
        .filter(|a| a.browser_download_url.strip_prefix(&downloads) == Some(a.name.as_str()))
        .map(|a| Asset { name: a.name, url: a.browser_download_url, size: a.size })
        .collect();
    let mut notes = raw.body.unwrap_or_default();
    if notes.len() > 16 * 1024 {
        let mut cut = 16 * 1024;
        while !notes.is_char_boundary(cut) {
            cut = cut.saturating_sub(1);
        }
        notes.truncate(cut);
        notes.push('…');
    }
    Some(Release { version, tag: raw.tag_name, page, notes, assets })
}

/// The newest release in GitHub's answer to `GET /repos/{repo}/releases`, skipping drafts and,
/// unless the app asks for them, pre-releases. `Ok(None)` when there is none.
pub fn newest(json: &str, app: &AppInfo) -> Result<Option<Release>, Error> {
    let raw: Vec<RawRelease> = serde_json::from_str(json).map_err(|e| Error::Invalid(format!("not a release list: {e}")))?;
    Ok(raw.into_iter().filter_map(|r| read(r, app)).max_by(|a, b| a.version.cmp(&b.version)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn app() -> AppInfo {
        AppInfo {
            id: "pdfcraft".into(),
            display_name: "PdfCraft".into(),
            repo: "treeforjs/pdfcraft".into(),
            current: "0.5.0".into(),
            public_key: None,
            prereleases: false,
        }
    }

    fn rel(tag: &str, pre: bool, draft: bool, assets: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","prerelease":{pre},"draft":{draft},"html_url":"https://github.com/treeforjs/pdfcraft/releases/tag/{tag}","body":"notes","assets":[{assets}]}}"#
        )
    }

    fn asset(tag: &str, name: &str, url_name: &str) -> String {
        format!(r#"{{"name":"{name}","size":10,"browser_download_url":"https://github.com/treeforjs/pdfcraft/releases/download/{tag}/{url_name}"}}"#)
    }

    #[test]
    fn the_newest_published_release_wins() {
        let json = format!(
            "[{},{},{},{}]",
            rel("v0.6.0", false, false, &asset("v0.6.0", "a.msi", "a.msi")),
            rel("v0.8.0", false, true, ""),
            rel("v0.7.0-rc.1", true, false, ""),
            rel("v0.5.1", false, false, ""),
        );
        let r = newest(&json, &app()).unwrap().unwrap();
        assert_eq!(r.tag, "v0.6.0");
        assert_eq!(r.assets.len(), 1);
        let mut beta = app();
        beta.prereleases = true;
        assert_eq!(newest(&json, &beta).unwrap().unwrap().tag, "v0.7.0-rc.1");
        assert_eq!(newest("[]", &app()).unwrap(), None);
        assert!(newest("{\"message\":\"Not Found\"}", &app()).is_err());
        assert!(newest("<html>", &app()).is_err());
    }

    #[test]
    fn only_assets_inside_the_apps_own_release_are_kept() {
        let foreign = r#"{"name":"evil.msi","size":1,"browser_download_url":"https://example.com/evil.msi"}"#;
        let other_tag = asset("v0.1.0", "old.msi", "old.msi");
        let renamed = asset("v0.6.0", "good.msi", "other.msi");
        let dotted = asset("v0.6.0", "..", "..");
        let huge = r#"{"name":"big.msi","size":99999999999,"browser_download_url":"https://github.com/treeforjs/pdfcraft/releases/download/v0.6.0/big.msi"}"#;
        let ok = asset("v0.6.0", "pdfcraft-0.6.0-windows-x64.msi", "pdfcraft-0.6.0-windows-x64.msi");
        let json = format!("[{}]", rel("v0.6.0", false, false, &[foreign, &other_tag, &renamed, &dotted, huge, &ok].join(",")));
        let r = newest(&json, &app()).unwrap().unwrap();
        let names: Vec<_> = r.assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["pdfcraft-0.6.0-windows-x64.msi"]);
        let page = r#"[{"tag_name":"v1.0.0","html_url":"https://evil.example/x"}]"#;
        assert_eq!(newest(page, &app()).unwrap().unwrap().page, "https://github.com/treeforjs/pdfcraft/releases");
    }
}
