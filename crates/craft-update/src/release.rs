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
    // A shared repository can publish several crates alongside its app. Only the app's
    // own namespace is a release candidate; keep the original tag for URLs and downloads.
    let prefix = format!("{}-", app.id);
    let version_tag = raw.tag_name.strip_prefix(&prefix).filter(|tag| tag.starts_with('v')).unwrap_or(&raw.tag_name);
    let version = Version::parse(version_tag)?;
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
/// Accepts plain version tags and `<app.id>-v<version>`; other crate namespaces are skipped.
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

    fn launcher() -> AppInfo {
        AppInfo {
            id: "craftworks".into(),
            display_name: "Craftworks".into(),
            repo: "treeforjs/craftworks".into(),
            current: "0.1.0".into(),
            public_key: None,
            prereleases: false,
        }
    }

    fn launcher_release(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "html_url": format!("https://github.com/treeforjs/craftworks/releases/tag/{tag}"),
            "assets": [],
        })
    }

    #[test]
    fn namespaced_app_releases_skip_shared_crates_and_foreign_apps() {
        let mut draft = launcher_release("craftworks-v0.1.3");
        draft["draft"] = true.into();
        let json = serde_json::json!([
            launcher_release("craft-ui-v0.2.0"),
            launcher_release("craft-update-v0.3.0"),
            launcher_release("pdfcraft-v9.0.0"),
            launcher_release("release-craftworks-v9.0.0"),
            launcher_release("craftworks-vv9.0.0"),
            launcher_release("craftworks-v0.1.2-rc.1"),
            draft,
            launcher_release("craftworks-v0.1.1"),
            launcher_release("v0.1.0"),
        ]);
        let release = newest(&json.to_string(), &launcher()).unwrap().unwrap();
        assert_eq!(release.version, Version::parse("0.1.1").unwrap());
        assert_eq!(release.tag, "craftworks-v0.1.1");
        assert_eq!(release.page, "https://github.com/treeforjs/craftworks/releases/tag/craftworks-v0.1.1");
    }

    #[test]
    fn namespaced_releases_keep_the_original_tag_for_asset_validation() {
        let mut release = launcher_release("craftworks-v0.1.1");
        release["assets"] = serde_json::json!([
            {
                "name": "craftworks-0.1.1-windows-x64-setup.exe",
                "size": 10,
                "browser_download_url": "https://github.com/treeforjs/craftworks/releases/download/craftworks-v0.1.1/craftworks-0.1.1-windows-x64-setup.exe",
            },
            {
                "name": "wrong-tag.exe",
                "size": 10,
                "browser_download_url": "https://github.com/treeforjs/craftworks/releases/download/v0.1.1/wrong-tag.exe",
            },
            {
                "name": "foreign-repo.exe",
                "size": 10,
                "browser_download_url": "https://github.com/treeforjs/pdfcraft/releases/download/craftworks-v0.1.1/foreign-repo.exe",
            },
        ]);
        let release = newest(&serde_json::json!([release]).to_string(), &launcher()).unwrap().unwrap();
        assert_eq!(release.assets.len(), 1);
        let asset = release.assets.first().unwrap();
        assert_eq!(asset.name, "craftworks-0.1.1-windows-x64-setup.exe");
        assert!(asset.url.contains("/download/craftworks-v0.1.1/"));
    }

    #[test]
    fn plain_and_leading_v_app_tags_still_work() {
        for tag in ["0.1.1", "v0.1.1", "craftworks-v0.1.1"] {
            let json = serde_json::json!([launcher_release(tag)]);
            let release = newest(&json.to_string(), &launcher()).unwrap().unwrap();
            assert_eq!(release.version, Version::parse("0.1.1").unwrap());
        }
        for tag in ["craft-ui-v0.2.0", "pdfcraft-v0.2.0", "release-v0.2.0"] {
            let json = serde_json::json!([launcher_release(tag)]);
            assert!(newest(&json.to_string(), &launcher()).unwrap().is_none(), "{tag}");
        }
    }
}
