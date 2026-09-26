use anyhow::{Context, Result, ensure};
use std::{fs, io::Write, path::Path};

/// Import snapshots and safe preferences only. Never carry executable hooks or credentials.
pub(crate) fn run(source: &Path, destination: &Path, preview: bool) -> Result<()> {
    ensure!(source.is_dir(), "Import source must be a directory");
    let mut files = Vec::new();
    let config = source.join("config.toml");
    if config.exists() {
        ensure!(
            config.symlink_metadata()?.file_type().is_file(),
            "Config must be a regular file"
        );
        ensure!(
            config.metadata()?.len() <= 1024 * 1024,
            "Config exceeds 1 MiB"
        );
        let value: toml::Value = fs::read_to_string(&config)?
            .parse()
            .context("Invalid source config")?;
        let mut safe = toml::map::Map::new();
        for section in ["display", "provider", "compaction"] {
            if let Some(value) = value.get(section) {
                safe.insert(section.into(), value.clone());
            }
        }
        // Newline bindings and quiet defaults belong to Anastasia, not the imported UI.
        if let Some(display) = safe.get_mut("display").and_then(toml::Value::as_table_mut) {
            display.insert("keybinding_hints".into(), false.into());
            display.insert("idle_animation".into(), true.into());
        }
        let parsed: crate::config::Config =
            toml::from_str(&toml::to_string(&safe)?).context("Unsupported imported preferences")?;
        // Serialize known types to discard unknown fields, including stray secrets.
        safe.insert("display".into(), toml::Value::try_from(&parsed.display)?);
        safe.insert("provider".into(), toml::Value::try_from(&parsed.provider)?);
        safe.insert(
            "compaction".into(),
            toml::Value::try_from(&parsed.compaction)?,
        );
        let text = toml::to_string_pretty(&safe)?;
        files.push((destination.join("config.toml"), text.into_bytes()));
    }
    let sessions = source.join("sessions");
    if sessions.exists() {
        ensure!(
            sessions.symlink_metadata()?.file_type().is_dir(),
            "Sessions must be a real directory"
        );
        for entry in fs::read_dir(sessions)? {
            let entry = entry?;
            if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            ensure!(
                entry.file_type()?.is_file(),
                "Session must be a regular file"
            );
            ensure!(
                entry.metadata()?.len() <= 64 * 1024 * 1024,
                "Session exceeds 64 MiB"
            );
            // Replay journals in scratch space: the loader may back up corrupt journals.
            let scratch = tempfile::tempdir()?;
            let snapshot = scratch.path().join(entry.file_name());
            fs::copy(entry.path(), &snapshot)?;
            let journal = crate::session::session_journal_path_from_snapshot(&entry.path());
            if journal.exists() {
                ensure!(
                    journal.symlink_metadata()?.file_type().is_file(),
                    "Journal must be a regular file"
                );
                ensure!(
                    journal.metadata()?.len() <= 64 * 1024 * 1024,
                    "Journal exceeds 64 MiB"
                );
                let text = fs::read_to_string(&journal)?;
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    let _: serde_json::Value =
                        serde_json::from_str(line).context("Invalid session journal")?;
                }
                fs::write(
                    crate::session::session_journal_path_from_snapshot(&snapshot),
                    text,
                )?;
            }
            let session = crate::session::Session::load_from_path(&snapshot)
                .with_context(|| format!("Invalid session {}", entry.path().display()))?;
            ensure!(
                entry.path().file_stem().and_then(|s| s.to_str()) == Some(session.id.as_str()),
                "Session ID must match its filename"
            );
            let bytes = serde_json::to_vec(&session)?;
            files.push((destination.join("sessions").join(entry.file_name()), bytes));
            // ponytail: validate in memory up to 512 MiB; use a staging manifest for larger imports.
            ensure!(
                files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>() <= 512 * 1024 * 1024,
                "Import exceeds 512 MiB; split it into smaller batches"
            );
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if files.is_empty() {
        println!("Nothing to import from {}", source.display());
    }
    // Validate every source before writing anything. create_new also protects against races.
    for (path, bytes) in files {
        if path.symlink_metadata().is_ok() {
            println!("Skip existing {}", path.display());
            continue;
        }
        if preview {
            println!("Would import {}", path.display());
            continue;
        }
        for directory in [destination, path.parent().unwrap()] {
            if directory.symlink_metadata().is_ok() {
                ensure!(
                    directory.symlink_metadata()?.file_type().is_dir(),
                    "Destination must be a real directory"
                );
            }
            fs::create_dir_all(directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
            }
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        println!("Imported {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_import_and_conflict_preserve_sources() -> Result<()> {
        let source = tempfile::tempdir()?;
        let target = tempfile::tempdir()?;
        let config = "[provider]\ndefault_model = 'example'\n[hooks]\n";
        fs::write(source.path().join("config.toml"), config)?;
        run(source.path(), target.path(), true)?;
        assert!(!target.path().join("config.toml").exists());
        run(source.path(), target.path(), false)?;
        let imported = fs::read_to_string(target.path().join("config.toml"))?;
        assert!(imported.contains("example") && !imported.contains("hooks"));
        run(source.path(), target.path(), false)?;
        assert_eq!(
            fs::read_to_string(target.path().join("config.toml"))?,
            imported
        );
        assert_eq!(
            fs::read_to_string(source.path().join("config.toml"))?,
            config
        );
        Ok(())
    }
}
