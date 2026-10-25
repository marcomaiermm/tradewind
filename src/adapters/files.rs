use crate::i18n::Text;
use anyhow::{Context, Result, ensure};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    #[serde(alias = "System")]
    System,
    #[serde(alias = "Dunkel")]
    Dark,
    #[serde(alias = "Hell")]
    Light,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub wow_path: PathBuf,
    pub email: String,
    pub channel: String,
    pub auto_sync: bool,
    pub auto_addons: bool,
    pub backup_hours: u64,
    pub retention_days: u64,
    pub notifications: bool,
    pub share_data: bool,
    pub autostart: bool,
    #[serde(flatten)]
    pub window: crate::core::desktop::WindowPreferences,
    pub theme: Appearance,
    pub language: crate::i18n::LanguagePreference,
    pub system_id: String,
    pub terms_accepted: bool,
    pub remember_login: bool,
    pub legacy_http: bool,
    pub realm_filters: crate::core::RealmFilters,
    pub auction_favorites: Vec<crate::core::auction::Favorite>,
}
impl Default for Settings {
    fn default() -> Self {
        // Stable machine identifier, compatible with the original's 12-digit hex format.
        use sha2::{Digest, Sha256};
        let machine = fs::read_to_string("/etc/machine-id").unwrap_or_default();
        let hash = format!("{:x}", Sha256::digest(machine.as_bytes()));
        Self {
            wow_path: PathBuf::new(),
            email: String::new(),
            channel: "release".into(),
            auto_sync: true,
            auto_addons: true,
            backup_hours: 1,
            retention_days: 30,
            notifications: true,
            share_data: false,
            autostart: false,
            window: Default::default(),
            theme: Appearance::System,
            language: Default::default(),
            system_id: hash[..12].into(),
            terms_accepted: false,
            remember_login: false,
            legacy_http: false,
            realm_filters: crate::core::RealmFilters::default(),
            auction_favorites: Vec::new(),
        }
    }
}
/// What the settings page edits. Everything else in `Settings` belongs to its own action
/// (login, language, realm filters, favorites), so a page save cannot overwrite it.
#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub wow_path: PathBuf,
    pub channel: String,
    pub auto_sync: bool,
    pub auto_addons: bool,
    pub backup_hours: u64,
    pub retention_days: u64,
    pub notifications: bool,
    pub share_data: bool,
    pub autostart: bool,
    pub window: crate::core::desktop::WindowPreferences,
    pub theme: Appearance,
}
impl Settings {
    pub fn preferences(&self) -> Preferences {
        Preferences {
            wow_path: self.wow_path.clone(),
            channel: self.channel.clone(),
            auto_sync: self.auto_sync,
            auto_addons: self.auto_addons,
            backup_hours: self.backup_hours,
            retention_days: self.retention_days,
            notifications: self.notifications,
            share_data: self.share_data,
            autostart: self.autostart,
            window: self.window,
            theme: self.theme,
        }
    }
    pub fn set_preferences(&mut self, preferences: Preferences) {
        let Preferences {
            wow_path,
            channel,
            auto_sync,
            auto_addons,
            backup_hours,
            retention_days,
            notifications,
            share_data,
            autostart,
            window,
            theme,
        } = preferences;
        self.wow_path = wow_path;
        self.channel = channel;
        self.auto_sync = auto_sync;
        self.auto_addons = auto_addons;
        self.backup_hours = backup_hours;
        self.retention_days = retention_days;
        self.notifications = notifications;
        self.share_data = share_data;
        self.autostart = autostart;
        self.window = window;
        self.theme = theme;
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=168).contains(&self.backup_hours) && (1..=3650).contains(&self.retention_days),
            msg!(InvalidBackupPeriod)
        );
        crate::core::BackupId {
            account: "ACCOUNT".into(),
            system_id: self.system_id.clone(),
            timestamp: 1,
        }
        .validate()
    }
}
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}
impl Paths {
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("", "", "tradewind").context(msg!(MissingXdgDirs))?;
        Ok(Self {
            config: dirs.config_dir().to_owned(),
            data: dirs.data_dir().to_owned(),
        })
    }
    pub fn load(&self) -> Result<Settings> {
        let path = self.config.join("settings.json");
        if !path.exists() {
            return Ok(Settings::default());
        }
        let settings: Settings = serde_json::from_slice(&fs::read(path)?)?;
        settings.validate()?;
        Ok(settings)
    }
    /// Like `load`, but sets an unreadable file aside so a damaged config never blocks startup.
    pub fn load_or_recover(&self) -> Result<(Settings, Option<Text>)> {
        let error = match self.load() {
            Ok(settings) => return Ok((settings, None)),
            Err(error) => error,
        };
        let path = self.config.join("settings.json");
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let broken = path.with_file_name(format!("settings.json.broken-{stamp}"));
        fs::rename(&path, &broken).with_context(|| {
            msg!(
                SettingsUnreadable,
                path = path.display().to_string(),
                error = Text::from_error(&error)
            )
        })?;
        let warning = msg!(
            SettingsReset,
            path = broken.display().to_string(),
            error = Text::from_error(&error)
        );
        Ok((Settings::default(), Some(warning)))
    }
    pub fn save(&self, settings: &Settings) -> Result<()> {
        settings.validate()?;
        atomic_write(
            &self.config.join("settings.json"),
            &serde_json::to_vec_pretty(settings)?,
        )
    }
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }
    #[cfg(test)]
    pub fn in_dir(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
        }
    }
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context(msg!(PathNoDirectory))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| msg!(FileSaveFailed, path = path.display().to_string()))?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn within(root: &Path, path: &Path) -> Result<()> {
    ensure!(
        !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        msg!(ParentPathForbidden)
    );
    let root = root.canonicalize()?;
    let mut existing = path;
    while fs::symlink_metadata(existing).is_err() {
        existing = existing.parent().context(msg!(InvalidDestination))?;
    }
    ensure!(
        existing.canonicalize()?.starts_with(&root),
        msg!(OutsideWow)
    );
    ensure!(
        !fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()),
        msg!(SymlinkDestination)
    );
    Ok(())
}

/// Cross-process root lease. The lock file is never removed: unlinking would allow two lock inodes.
pub(crate) struct MutationLease {
    _file: fs::File,
}
impl MutationLease {
    pub fn acquire(root: &Path) -> Result<Self> {
        let root = crate::adapters::wow::normalize_root(root)
            .canonicalize()
            .context(msg!(MissingWow))?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            // A planted lock symlink must never redirect a write.
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join(".tradewind.lock"))?;
        file.try_lock().context(msg!(OtherInstance))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confinement_and_process_lease() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("absent"), root.path().join("dangling"))
            .unwrap();
        assert!(within(root.path(), &root.path().join("safe/file.lua")).is_ok());
        assert!(within(root.path(), &root.path().join("new/../../escape")).is_err());
        assert!(within(root.path(), &root.path().join("escape/file.lua")).is_err());
        assert!(within(root.path(), &root.path().join("dangling")).is_err());
        let lease = MutationLease::acquire(root.path()).unwrap();
        assert!(MutationLease::acquire(root.path()).is_err());
        drop(lease);
        assert!(MutationLease::acquire(root.path()).is_ok());
        fs::remove_file(root.path().join(".tradewind.lock")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("lock"),
            root.path().join(".tradewind.lock"),
        )
        .unwrap();
        assert!(MutationLease::acquire(root.path()).is_err());
        assert!(!outside.path().join("lock").exists());
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[test]
    fn window_preferences_default_migrate_and_roundtrip() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.window.close_to_tray);
        assert!(!settings.window.minimize_to_tray);
        assert!(!settings.window.start_in_tray);
        assert!(!settings.autostart);
        for close in [false, true] {
            let mut settings: Settings = serde_json::from_value(serde_json::json!({
                "close_to_tray": close, "auto_sync": false, "language": "german"
            }))
            .unwrap();
            assert_eq!(settings.window.close_to_tray, close);
            assert!(!settings.auto_sync);
            assert_eq!(settings.language, crate::i18n::LanguagePreference::German);
            settings.window.minimize_to_tray = true;
            settings.window.start_in_tray = true;
            let root = tempfile::tempdir().unwrap();
            let paths = Paths {
                config: root.path().into(),
                data: root.path().join("data"),
            };
            paths.save(&settings).unwrap();
            assert_eq!(paths.load().unwrap().window, settings.window);
            let value = serde_json::to_value(&settings).unwrap();
            assert_eq!(value["close_to_tray"], close);
            assert!(
                value.get("window").is_none(),
                "preferences keep their flat storage keys"
            );
        }
    }

    #[test]
    fn corrupt_periods_are_rejected_when_loading_existing_configuration() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths {
            config: root.path().into(),
            data: root.path().join("data"),
        };
        fs::write(
            root.path().join("settings.json"),
            br#"{"backup_hours":18446744073709551615}"#,
        )
        .unwrap();
        assert!(paths.load().is_err());
        fs::write(root.path().join("settings.json"), b"{}").unwrap();
        let loaded = paths.load().unwrap();
        assert!(!loaded.remember_login);
        assert!(!loaded.legacy_http);
        assert_eq!(loaded.backup_hours, 1);
    }

    #[test]
    fn unreadable_settings_are_set_aside_and_replaced_by_defaults() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::in_dir(root.path());
        let file = paths.config.join("settings.json");
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(&file, b"{ not json").unwrap();
        let (settings, warning) = paths.load_or_recover().unwrap();
        assert!(warning.is_some());
        assert_eq!(settings.backup_hours, Settings::default().backup_hours);
        assert!(!file.exists());
        let kept: Vec<_> = fs::read_dir(&paths.config).unwrap().collect();
        assert_eq!(kept.len(), 1, "the damaged file is kept for inspection");
        assert_eq!(
            fs::read(kept[0].as_ref().unwrap().path()).unwrap(),
            b"{ not json"
        );
        assert!(paths.load_or_recover().unwrap().1.is_none());
    }
}

#[cfg(test)]
mod language_migration_tests {
    use super::*;
    #[test]
    fn existing_german_theme_values_migrate_without_losing_preferences() {
        for (old, expected) in [
            ("System", Appearance::System),
            ("Dunkel", Appearance::Dark),
            ("Hell", Appearance::Light),
        ] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"theme":old,"auto_sync":false})).unwrap();
            assert_eq!(settings.theme, expected);
            assert_eq!(settings.language, crate::i18n::LanguagePreference::System);
            assert!(!settings.auto_sync);
            let serialized = serde_json::to_value(&settings).unwrap();
            assert!(["system", "dark", "light"].contains(&serialized["theme"].as_str().unwrap()));
            assert_eq!(serialized["language"], "system");
            assert!(serialized.get("password").is_none());
            assert!(serialized.get("refresh_token").is_none());
        }
    }
}
