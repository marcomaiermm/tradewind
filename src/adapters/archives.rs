//! Archive operations use libarchive's bsdtar, including legacy ZIP-LZMA backups.
use crate::{
    adapters::{
        files::{Paths, Settings, atomic_write, within},
        wow::{self, Account},
    },
    core::{self, BackupId, Game},
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{Local, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

const LIMIT: u64 = 128 * 1024 * 1024;
fn output(command: &mut Command, limit: u64) -> Result<Vec<u8>> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context(msg!(MissingArchiveTools))?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(limit + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > limit {
        let _ = child.kill();
        let _ = child.wait();
        bail!(msg!(ArchiveTooLarge));
    }
    ensure!(child.wait()?.success(), msg!(ArchiveFailed));
    Ok(bytes)
}
pub fn members(archive: &Path) -> Result<Vec<String>> {
    let verbose = output(
        Command::new("bsdtar").arg("-tvf").arg(archive),
        2 * 1024 * 1024,
    )?;
    ensure!(
        String::from_utf8_lossy(&verbose)
            .lines()
            .all(|s| s.starts_with('-') || s.starts_with('d')),
        msg!(ArchiveLinks)
    );
    let listing = String::from_utf8(output(
        Command::new("bsdtar").arg("-tf").arg(archive),
        2 * 1024 * 1024,
    )?)?;
    let names: Vec<_> = listing.lines().map(str::to_owned).collect();
    ensure!(!names.is_empty() && names.len() < 20000, msg!(ArchiveSize));
    let mut seen = std::collections::HashSet::new();
    for name in &names {
        ensure!(
            safe_member(name) && seen.insert(name),
            msg!(ArchiveDuplicatePath)
        );
    }
    Ok(names)
}
pub fn safe_member(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':', '\0', '*', '?', '[', ']'])
        && !name.chars().any(char::is_control)
        && Path::new(name)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
        && !name.starts_with('-')
}
fn extract_file(archive: &Path, name: &str) -> Result<Vec<u8>> {
    ensure!(safe_member(name), msg!(ArchiveInvalidPath));
    output(
        Command::new("bsdtar")
            .arg("-xOf")
            .arg(archive)
            .arg("--")
            .arg(name),
        LIMIT,
    )
}

pub fn install(root: &Path, game: Game, addon: &str, bytes: &[u8]) -> Result<()> {
    ensure!(
        ["TradeSkillMaster", "TradeSkillMaster_AppHelper"].contains(&addon),
        msg!(UnknownAddon)
    );
    ensure!(!wow::running(), msg!(CloseWowUpdate));
    let dest = wow::addon_dir(root, game);
    within(root, &dest)?;
    fs::create_dir_all(&dest)?;
    let temp = tempfile::tempdir_in(&dest)?;
    let zip = temp.path().join("download.zip");
    atomic_write(&zip, bytes)?;
    let files = members(&zip)?;
    let stage = temp.path().join(addon);
    fs::create_dir(&stage)?;
    let mut total = 0;
    for name in files {
        let path = Path::new(&name);
        ensure!(
            path.components()
                .next()
                .is_some_and(|c| c.as_os_str() == addon),
            msg!(AddonForeignFiles)
        );
        if name.ends_with('/') {
            continue;
        }
        let relative = path.strip_prefix(addon)?;
        ensure!(!relative.as_os_str().is_empty(), msg!(InvalidAddonFile));
        let data = extract_file(&zip, &name)?;
        total += data.len();
        ensure!(total < 256 * 1024 * 1024, msg!(AddonTooLarge));
        atomic_write(&stage.join(relative), &data)?;
    }
    ensure!(
        stage.join(format!("{addon}.toc")).is_file(),
        msg!(AddonMissingToc)
    );
    let target = dest.join(addon);
    within(root, &target)?;
    if addon == "TradeSkillMaster_AppHelper" && target.join("AppData.lua").is_file() {
        atomic_write(
            &stage.join("AppData.lua"),
            &fs::read(target.join("AppData.lua"))?,
        )?;
    }
    let old = temp.path().join("previous");
    if target.exists() {
        fs::rename(&target, &old)?;
    }
    if let Err(error) = fs::rename(&stage, &target) {
        if old.exists()
            && let Err(rollback) = fs::rename(&old, &target)
        {
            let recovery = temp.keep().join("previous");
            bail!(msg!(
                AddonRollbackFailed,
                error = rollback.to_string(),
                path = recovery.display().to_string()
            ));
        }
        return Err(error.into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Backup {
    pub account: String,
    pub system_id: String,
    pub timestamp: i64,
    pub local: Option<PathBuf>,
    pub remote: bool,
    pub keep: bool,
}
impl Backup {
    pub fn id(&self) -> BackupId {
        BackupId {
            account: self.account.clone(),
            system_id: self.system_id.clone(),
            timestamp: self.timestamp,
        }
    }
    pub fn remote_name(&self) -> String {
        let id = self.id();
        format!("{}_{}_{}.zip", id.system_id, id.account, id.timestamp)
    }
}
pub fn local_backups(paths: &Paths, settings: &Settings) -> Result<Vec<Backup>> {
    let dir = paths.backups();
    let mut backups = Vec::new();
    for (dir, keep) in [(dir.clone(), false), (dir.join("keep"), true)] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".zip") else {
                continue;
            };
            let Some((account, time)) = stem.rsplit_once('_') else {
                continue;
            };
            if !core::valid_account(account) {
                continue;
            }
            let timestamp = if time.len() == 14 {
                NaiveDateTime::parse_from_str(time, "%Y%m%d%H%M%S")
                    .ok()
                    .and_then(|t| Local.from_local_datetime(&t).single())
                    .map(|t| t.timestamp())
            } else {
                time.parse::<i64>().ok()
            };
            if let Some(timestamp) = timestamp.filter(|t| *t > 0) {
                backups.push(Backup {
                    account: account.into(),
                    system_id: settings.system_id.clone(),
                    timestamp,
                    local: Some(entry.path()),
                    remote: false,
                    keep,
                });
            }
        }
    }
    backups.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    Ok(backups)
}
pub fn create(paths: &Paths, settings: &Settings, account: &Account, keep: bool) -> Result<Backup> {
    ensure!(!wow::running(), msg!(CloseWowConsistentBackup));
    let name = account.protocol_name();
    ensure!(core::valid_account(&name), msg!(InvalidAccount));
    let files: Vec<_> = fs::read_dir(&account.path)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_file())
                && e.file_name()
                    .to_string_lossy()
                    .starts_with("TradeSkillMaster")
                && e.file_name().to_string_lossy().ends_with(".lua")
        })
        .map(|e| e.file_name())
        .collect();
    ensure!(!files.is_empty(), msg!(NoSavedVariables));
    let temp = tempfile::tempdir()?;
    let zip = temp.path().join("backup.zip");
    output(
        Command::new("zip")
            .arg("-q")
            .arg(&zip)
            .arg("--")
            .args(files)
            .current_dir(&account.path),
        1024,
    )?;
    let timestamp = Utc::now().timestamp();
    let parent = if keep {
        paths.backups().join("keep")
    } else {
        paths.backups()
    };
    let local = parent.join(format!("{name}_{timestamp}.zip"));
    ensure!(!local.exists(), msg!(BackupExists));
    atomic_write(&local, &fs::read(zip)?)?;
    Ok(Backup {
        account: name,
        system_id: settings.system_id.clone(),
        timestamp,
        local: Some(local),
        remote: false,
        keep,
    })
}
pub fn bytes(paths: &Paths, backup: &Backup) -> Result<Vec<u8>> {
    backup.id().validate()?;
    let local = backup.local.as_ref().context(msg!(CloudOnlyBackup))?;
    within(&paths.backups(), local)?;
    ensure!(fs::metadata(local)?.len() <= LIMIT, msg!(BackupTooLarge));
    Ok(fs::read(local)?)
}
pub fn restore_bytes(
    paths: &Paths,
    settings: &Settings,
    account: &Account,
    bytes: &[u8],
) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let archive = temp.path().join("backup.zip");
    atomic_write(&archive, bytes)?;
    restore(paths, settings, account, &archive)
}
pub fn restore(
    paths: &Paths,
    settings: &Settings,
    account: &Account,
    archive: &Path,
) -> Result<()> {
    ensure!(!wow::running(), msg!(CloseWowRestore));
    let names = members(archive)?;
    let mut files = Vec::new();
    let mut total = 0;
    for name in names {
        ensure!(
            !name.contains('/') && name.starts_with("TradeSkillMaster") && name.ends_with(".lua"),
            msg!(BackupForeignFiles)
        );
        let bytes = extract_file(archive, &name)?;
        total += bytes.len();
        ensure!(total <= 256 * 1024 * 1024, msg!(BackupTooLarge));
        let globals = crate::adapters::lua::parse(std::str::from_utf8(&bytes)?)?;
        if name == "TradeSkillMaster.lua" {
            ensure!(
                globals
                    .get("TradeSkillMasterDB")
                    .is_some_and(serde_json::Value::is_object),
                msg!(BackupInvalidTable)
            );
        }
        files.push((name, bytes));
    }
    ensure!(
        files.iter().any(|(n, _)| n == "TradeSkillMaster.lua"),
        msg!(BackupNoData)
    );
    for (name, _) in &files {
        within(&settings.wow_path, &account.path.join(name))?;
    }
    // Keep a recoverable pre-restore backup even if a later disk write fails.
    create(paths, settings, account, true)?;
    for (name, bytes) in files {
        atomic_write(&account.path.join(name), &bytes)?;
    }
    Ok(())
}
pub fn prune(paths: &Paths, settings: &Settings) -> Result<()> {
    let backups = local_backups(paths, settings)?;
    let cutoff = Utc::now().timestamp() - settings.retention_days as i64 * 86400;
    for b in &backups {
        if b.keep || b.timestamp >= cutoff {
            continue;
        }
        if backups
            .iter()
            .any(|newer| newer.account == b.account && newer.timestamp > b.timestamp)
            && let Some(path) = &b.local
        {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_paths_are_confined() {
        for bad in [
            "../x",
            "/tmp/x",
            "a/../../x",
            "a\\b",
            "C:/x",
            "a\nx",
            "-file",
        ] {
            assert!(!safe_member(bad), "{bad}");
        }
        assert!(safe_member("TradeSkillMaster/LibTSM/init.lua"));
        assert!(core::valid_account("ACCOUNT#1-Anniversary"));
        assert!(!core::valid_account("../ACCOUNT"));
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }
    #[test]
    fn legacy_lzma_restore_keeps_previous_settings_and_rejects_invalid_archives() {
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join("_retail_/WTF/Account/ACCOUNT/SavedVariables");
        fs::create_dir_all(&path).unwrap();
        let original = b"TradeSkillMasterDB = { original=true }\n";
        fs::write(path.join("TradeSkillMaster.lua"), original).unwrap();
        let account = Account {
            name: "ACCOUNT".into(),
            game: Game::Retail,
            path: path.clone(),
        };
        let settings = Settings {
            wow_path: root.path().into(),
            ..Default::default()
        };
        let paths = Paths::in_dir(root.path());
        for invalid in ["traversal.zip", "symlink.zip", "corrupt-lua.zip"] {
            assert!(
                restore(&paths, &settings, &account, &fixture(invalid)).is_err(),
                "{invalid}"
            );
            assert_eq!(
                fs::read(path.join("TradeSkillMaster.lua")).unwrap(),
                original
            );
        }
        restore(&paths, &settings, &account, &fixture("legacy-lzma.zip")).unwrap();
        assert_eq!(
            fs::read(path.join("TradeSkillMaster.lua")).unwrap(),
            include_bytes!("../../tests/fixtures/savedvariables.lua")
        );
        let backups = local_backups(&paths, &settings).unwrap();
        assert_eq!(backups.len(), 1);
        assert!(backups[0].keep);
        assert_eq!(
            extract_file(backups[0].local.as_ref().unwrap(), "TradeSkillMaster.lua").unwrap(),
            original
        );
    }
    #[test]
    fn helper_update_preserves_appdata_and_foreign_archives_do_not_change_addons() {
        let root = tempfile::tempdir().unwrap();
        let addon = root
            .path()
            .join("_retail_/Interface/AddOns/TradeSkillMaster_AppHelper");
        fs::create_dir_all(&addon).unwrap();
        let data = include_bytes!("../../tests/fixtures/appdata.lua");
        fs::write(addon.join("AppData.lua"), data).unwrap();
        fs::write(
            addon.join("TradeSkillMaster_AppHelper.toc"),
            "## Version: old\n",
        )
        .unwrap();
        install(
            root.path(),
            Game::Retail,
            "TradeSkillMaster_AppHelper",
            include_bytes!("../../tests/fixtures/addon-helper.zip"),
        )
        .unwrap();
        assert_eq!(fs::read(addon.join("AppData.lua")).unwrap(), data);
        let before = fs::read(addon.join("TradeSkillMaster_AppHelper.toc")).unwrap();
        assert!(
            install(
                root.path(),
                Game::Retail,
                "TradeSkillMaster_AppHelper",
                include_bytes!("../../tests/fixtures/traversal.zip")
            )
            .is_err()
        );
        assert_eq!(
            fs::read(addon.join("TradeSkillMaster_AppHelper.toc")).unwrap(),
            before
        );
    }
}
