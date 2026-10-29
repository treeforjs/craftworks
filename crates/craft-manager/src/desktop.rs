//! Per-user freedesktop menu entries for managed Linux applications.
use std::{fs, io::Write, path::Path, process::Command};

use craft_update::Error;

use crate::{APPS, Manager};

impl Manager {
    /// Register active Linux apps and remove stale entries owned by this library.
    /// Call after installation/removal and at startup to repair older installations.
    pub fn sync_desktop_entries(&self) -> Result<(), Error> {
        if !cfg!(target_os = "linux") {
            return Ok(());
        }
        let root = Self::default_root()?;
        let data = root.parent().ok_or_else(|| Error::Install("Missing user data directory.".into()))?;
        self.sync_desktop_entries_in(data)?;
        // Desktop environments watch this directory; the MIME cache tool is optional.
        let _ = Command::new("update-desktop-database").arg(data.join("applications")).status();
        Ok(())
    }

    pub(crate) fn sync_desktop_entries_in(&self, data: &Path) -> Result<(), Error> {
        let applications = data.join("applications");
        fs::create_dir_all(&applications).map_err(failed)?;
        let owner = format!("X-Craftworks-Library={}\n", value(&self.root().to_string_lossy()));
        for app in APPS {
            // A separate ID preserves entries installed independently of Craftworks.
            let path = applications.join(format!("io.github.treeforjs.craftworks.{}.desktop", app.id));
            let existing = match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Some(fs::read_to_string(&path).map_err(failed)?),
                Ok(_) => return Err(Error::Install(format!("Menu entry {} is not a regular file.", path.display()))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(failed(error)),
            };
            if existing.as_ref().is_some_and(|text| !text.lines().any(|line| line == owner.trim_end())) {
                return Err(Error::Install(format!("Menu entry {} belongs to another installation.", path.display())));
            }
            let Some(installed) = self.installed(app)? else {
                if existing.is_some() {
                    fs::remove_file(&path).map_err(failed)?;
                }
                continue;
            };
            let executable = installed.executable.to_str().ok_or_else(|| Error::Install("The application path is not UTF-8.".into()))?;
            let command = if app.id == "pdfcraft" { format!("{} %F", exec(executable)) } else { exec(executable) };
            let mime = if app.id == "pdfcraft" { "MimeType=application/pdf;\n" } else { "" };
            let text = format!(
                "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec={}\nIcon=applications-graphics\nTerminal=false\nCategories=Graphics;\nStartupNotify=true\n{}{}",
                app.name, app.summary, command, mime, owner
            );
            if existing.as_deref() != Some(text.as_str()) {
                let mut pending = tempfile::NamedTempFile::new_in(&applications).map_err(failed)?;
                pending.write_all(text.as_bytes()).map_err(failed)?;
                pending.persist(&path).map_err(|error| failed(error.error))?;
            }
        }
        Ok(())
    }
}

fn value(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

fn exec(path: &str) -> String {
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '%' => quoted.push_str("%%"),
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    value(&quoted)
}

fn failed(error: std::io::Error) -> Error {
    Error::Install(format!("Couldn't update application menu entries: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quotes_field_codes_and_shell_characters() {
        assert_eq!(exec("/apps/Pdf Craft/100%.AppImage"), "\"/apps/Pdf Craft/100%%.AppImage\"");
        assert_eq!(exec("/apps/$`\"\\.AppImage"), "\"/apps/\\\\$\\\\`\\\\\"\\\\\\\\.AppImage\"");
    }
}
