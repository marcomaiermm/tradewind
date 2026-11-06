use crate::i18n::{Language, LanguagePreference, Text};
use crate::{
    adapters::{
        archives::{self as archive, Backup},
        desktop,
        files::{self as storage, Paths, Preferences, Settings},
        lua,
        tsm::{Api, BACKEND, Feed, REALM_API},
        wow::{self, AppData, Installation},
    },
    core::{
        self, ExportKind, Faction, Game, Realm, RealmFilters,
        auction::{Catalog, Favorite, Market},
    },
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub installation: Installation,
    pub backups: Vec<Backup>,
    pub sync: Vec<SyncRow>,
    pub realms: Vec<Realm>,
    pub registered_realms: Vec<Realm>,
    pub channels: Vec<String>,
    pub latest: BTreeMap<String, String>,
    /// Accounting realms per account, keyed by `Account::protocol_name`.
    pub accounting_realms: BTreeMap<String, Vec<String>>,
    pub selected_directory: Option<PathBuf>,
    pub logged_in: bool,
    pub premium: bool,
    pub last_sync: Option<i64>,
    pub news: String,
    pub auction: Option<Arc<Catalog>>,
    pub log: Vec<LogLine>,
}
/// One Activity entry. `error` drives severity styling in the view.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub time: String,
    pub message: Text,
    pub error: bool,
}
impl LogLine {
    /// English line as written to `activity.log` and uploaded to TSM.
    fn render(&self) -> String {
        msg!(LogEntry, time = &self.time, message = &self.message).render(Language::English)
    }
}
impl Snapshot {
    pub fn realm_registered(&self, realm: &Realm) -> bool {
        self.registered_realms.iter().any(|registered| {
            registered.game == realm.game
                && registered.region == realm.region
                && registered.id == realm.id
                && registered.name == realm.name
        })
    }
    pub fn can_restore(&self, backup: &Backup) -> bool {
        self.knows_account(&backup.account) && (backup.local.is_some() || self.logged_in)
    }
    pub fn knows_account(&self, account: &str) -> bool {
        self.installation
            .accounts
            .iter()
            .any(|a| core::same_account(&a.protocol_name(), account))
    }
}
#[derive(Clone, Debug)]
pub struct SyncRow {
    pub game: Game,
    pub name: String,
    pub region: String,
    pub timestamp: i64,
    pub error: Option<Text>,
    pub removable: bool,
}
#[derive(Clone)]
pub enum Action {
    Bootstrap,
    InspectAt(PathBuf),
    ChooseDirectory(PathBuf),
    /// Opens the browser sign-in and waits for it.
    Login {
        remember: bool,
        terms: bool,
    },
    Logout,
    Sync,
    Save(Preferences),
    SaveRealmFilters(RealmFilters),
    SetLanguage(LanguagePreference),
    Install(Game, String),
    Backup,
    ScheduledBackup,
    Restore(Backup),
    AddRealm(Realm),
    RemoveRealm(SyncRow),
    /// Account protocol name, accounting realm and kind. The destination is asked for.
    Export(String, String, ExportKind),
    BrowseMarket(Option<Market>),
    ToggleFavorite(Favorite),
}
impl Action {
    /// Needs the WoW folder lease, which also keeps a second app instance out.
    pub(crate) fn writes_wow_files(&self) -> bool {
        matches!(
            self,
            Self::Sync
                | Self::Install(..)
                | Self::Backup
                | Self::ScheduledBackup
                | Self::Restore(..)
        )
    }
    /// Reports its result as a desktop notification, if enabled.
    pub(crate) fn notifies(&self) -> bool {
        matches!(
            self,
            Self::Sync | Self::Backup | Self::Restore(..) | Self::Install(..)
        )
    }
    /// Followed by a sync once the session is logged in.
    pub(crate) fn chains_sync(&self) -> bool {
        matches!(self, Self::Login { .. } | Self::Bootstrap)
    }
    /// Written to the Activity log. Preference toggles would only flood it.
    fn logged(&self) -> bool {
        !matches!(
            self,
            Self::ToggleFavorite(..) | Self::SetLanguage(..) | Self::SaveRealmFilters(..)
        )
    }
}
pub(crate) struct Engine {
    pub(crate) settings: Settings,
    /// Bumped on every settings write, so the UI can drop outcomes carrying older settings.
    pub(crate) revision: u64,
    pub paths: Paths,
    api: Api,
    /// Subscribed realms with the id of their subscription record, which removal needs.
    user_realms: Vec<(Realm, String)>,
    /// Addon zips of the selected channel from the last socket feed.
    addon_urls: BTreeMap<String, String>,
    pub snapshot: Snapshot,
    last_attempt: Instant,
    last_backup_attempt: Instant,
    /// Shown instead of the bootstrap result once, e.g. after damaged settings were reset.
    pub(crate) startup_warning: Option<Text>,
}
impl Engine {
    pub(crate) fn new(paths: Paths, settings: Settings) -> Result<Self> {
        let api = Api::new()?;
        Ok(Self {
            settings,
            revision: 0,
            paths,
            api,
            user_realms: Vec::new(),
            addon_urls: BTreeMap::new(),
            snapshot: Snapshot::default(),
            last_attempt: Instant::now(),
            last_backup_attempt: Instant::now(),
            startup_warning: None,
        })
    }
    pub fn log(&mut self, message: &Text) {
        self.push_log(message, false);
    }
    pub fn log_error(&mut self, message: &Text) {
        self.push_log(message, true);
    }
    fn push_log(&mut self, message: &Text, error: bool) {
        self.snapshot.log.insert(
            0,
            LogLine {
                time: chrono::Local::now().format("%H:%M:%S").to_string(),
                message: message.clone(),
                error,
            },
        );
        self.snapshot.log.truncate(200);
        let _ = storage::atomic_write(
            &self.paths.data.join("activity.log"),
            self.rendered_log().as_bytes(),
        );
    }
    fn rendered_log(&self) -> String {
        self.snapshot
            .log
            .iter()
            .map(LogLine::render)
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn language(&self) -> Language {
        self.settings.language.resolve(desktop::system_language())
    }
    pub(crate) fn sync_due(&self) -> bool {
        core::sync_due(
            self.settings.auto_sync,
            self.api.logged_in(),
            self.last_attempt.elapsed().as_secs(),
        )
    }
    pub(crate) fn backup_check_due(&self) -> bool {
        core::backup_check_due(
            self.last_backup_attempt.elapsed().as_secs(),
            self.settings.backup_hours,
        )
    }
    pub(crate) fn record_attempt(&mut self, action: &Action) {
        match action {
            Action::Sync => self.last_attempt = Instant::now(),
            Action::ScheduledBackup => self.last_backup_attempt = Instant::now(),
            _ => (),
        }
    }
    /// The only way settings change, so every write bumps the revision.
    fn save(&mut self, settings: Settings) -> Result<()> {
        self.paths.save(&settings)?;
        self.settings = settings;
        self.revision += 1;
        Ok(())
    }
    fn scan(&mut self) -> Result<()> {
        self.snapshot.installation = wow::scan(&self.settings.wow_path)?;
        Ok(())
    }
    fn read_accounting(&mut self) -> Result<()> {
        self.snapshot.accounting_realms = self
            .snapshot
            .installation
            .accounts
            .iter()
            .map(|account| Ok((account.protocol_name(), wow::accounting_realms(account)?)))
            .collect::<Result<_>>()?;
        Ok(())
    }
    /// Re-reads local archives. Cloud entries stay until the next cloud listing.
    fn refresh_backups(&mut self) -> Result<()> {
        let remote: Vec<_> = self
            .snapshot
            .backups
            .iter()
            .filter(|b| b.remote)
            .cloned()
            .collect();
        self.snapshot.backups = archive::local_backups(&self.paths, &self.settings)?;
        merge_backups(&mut self.snapshot.backups, remote);
        Ok(())
    }
    fn inspect(&mut self) -> Result<()> {
        self.scan()?;
        self.read_accounting()?;
        self.refresh_backups()
    }
    pub(crate) async fn run(&mut self, action: Action) -> Result<Text> {
        let logged = action.logged();
        let message = self.perform(action).await?;
        self.refresh_account_state();
        if logged {
            self.log(&message);
        }
        Ok(message)
    }
    async fn perform(&mut self, action: Action) -> Result<Text> {
        Ok(match action {
            Action::InspectAt(root) => {
                self.settings.wow_path = wow::normalize_root(&root);
                self.snapshot.auction = None;
                self.inspect()?;
                msg!(InstallationRead)
            }
            Action::Bootstrap => {
                self.inspect()?;
                if let Some(warning) = self.startup_warning.take() {
                    bail!(warning);
                }
                if self.settings.remember_login {
                    self.api.resume().await?;
                }
                msg!(InstallationRead)
            }
            Action::BrowseMarket(market) => {
                self.load_auction(market.as_ref())?;
                msg!(AuctionLoaded)
            }
            Action::ToggleFavorite(favorite) => {
                let catalog = self
                    .snapshot
                    .auction
                    .as_ref()
                    .context(msg!(AuctionMarketMissing))?;
                ensure!(
                    catalog
                        .selected
                        .as_ref()
                        .is_some_and(|m| m.game == favorite.game)
                        && catalog.items.iter().any(|i| i.id == favorite.item),
                    msg!(AuctionMarketMissing)
                );
                let mut settings = self.settings.clone();
                if settings.auction_favorites.contains(&favorite) {
                    settings.auction_favorites.retain(|f| f != &favorite);
                } else {
                    settings.auction_favorites.push(favorite);
                }
                self.save(settings)?;
                msg!(AuctionFavoriteSaved)
            }
            Action::ChooseDirectory(initial) => {
                self.snapshot.selected_directory =
                    Some(desktop::choose_directory(&initial, self.language()).await?);
                msg!(DirectorySelected)
            }
            Action::Login { remember, terms } => {
                ensure!(terms, msg!(TermsRequired));
                let warning = self.api.login(remember).await?;
                let mut settings = self.settings.clone();
                settings.terms_accepted = terms;
                settings.remember_login = remember;
                self.save(settings)?;
                warning.unwrap_or_else(|| msg!(LoggedIn))
            }
            Action::Logout => {
                let result = self.api.logout().await;
                let mut settings = self.settings.clone();
                settings.remember_login = false;
                self.save(settings)?;
                self.snapshot.realms.clear();
                self.snapshot.registered_realms.clear();
                self.user_realms.clear();
                self.snapshot.sync.clear();
                self.snapshot.backups.retain(|b| !b.remote);
                result?;
                msg!(LoggedOut)
            }
            Action::Save(mut preferences) => {
                preferences.wow_path = wow::normalize_root(&preferences.wow_path);
                // Settings save on every edit. Only a new folder is scanned and drops the catalog.
                let installation = if preferences.wow_path != self.settings.wow_path {
                    Some(wow::scan(&preferences.wow_path)?)
                } else {
                    None
                };
                let autostart_changed = preferences.autostart != self.settings.autostart;
                let mut settings = self.settings.clone();
                settings.set_preferences(preferences);
                self.save(settings)?;
                if let Some(installation) = installation {
                    self.snapshot.installation = installation;
                    self.snapshot.auction = None;
                    self.read_accounting()?;
                }
                if autostart_changed {
                    desktop::set_autostart(self.settings.autostart)?;
                }
                msg!(SettingsSaved)
            }
            Action::SetLanguage(language) => {
                let mut settings = self.settings.clone();
                settings.language = language;
                self.save(settings)?;
                msg!(LanguageSaved)
            }
            Action::SaveRealmFilters(filters) => {
                let mut settings = self.settings.clone();
                settings.realm_filters = filters;
                self.save(settings)?;
                msg!(FiltersSaved)
            }
            Action::Sync => self.sync().await?,
            Action::Install(game, addon) => {
                self.api.ensure_session().await?;
                let feed = self.api.feed().await?;
                self.apply_feed_info(&feed);
                self.scan()?;
                // Addon files are only read on login or `/reload`, so WoW may keep running.
                self.archive(true, true)?;
                self.install(game, &addon).await?;
                self.scan()?;
                self.refresh_backups()?;
                if wow::running() {
                    msg!(AddonInstalledReload, addon = addon, game = game.label())
                } else {
                    msg!(AddonInstalled, addon = addon, game = game.label())
                }
            }
            Action::Backup => {
                self.scan()?;
                let count = self.backup_due(true, false)?;
                self.refresh_backups()?;
                if self.api.logged_in() && self.api.premium() {
                    self.api.ensure_session().await?;
                    self.sync_backups().await?;
                }
                msg!(BackupCount, count = count)
            }
            Action::ScheduledBackup => {
                self.scan()?;
                let count = self.backup_due(false, false)?;
                self.refresh_backups()?;
                msg!(AutomaticBackupCount, count = count)
            }
            Action::Restore(backup) => {
                backup.id().validate()?;
                self.scan()?;
                let account = self
                    .snapshot
                    .installation
                    .accounts
                    .iter()
                    .find(|a| core::same_account(&a.protocol_name(), &backup.account))
                    .context(msg!(AccountMissingInstallation))?
                    .clone();
                if let Some(path) = &backup.local {
                    archive::restore(&self.paths, &self.settings, &account, path)?;
                } else {
                    self.api.ensure_session().await?;
                    let list = self.api.get(&format!("{BACKEND}/backups")).await?;
                    let url = remote_backups(&list)?
                        .into_iter()
                        .find(|(remote, _)| remote.remote_name() == backup.remote_name())
                        .context(msg!(CloudOnlyBackup))?
                        .1;
                    let bytes = self.api.fetch(&url).await?;
                    archive::restore_bytes(&self.paths, &self.settings, &account, &bytes)?;
                }
                self.inspect()?;
                msg!(BackupRestored)
            }
            Action::AddRealm(realm) => {
                ensure!(self.snapshot.realms.contains(&realm), msg!(StaleRealm));
                if self.snapshot.realm_registered(&realm) {
                    return Ok(msg!(RealmAlreadyAdded, name = realm.name));
                }
                self.api.ensure_session().await?;
                let body = json!({
                    "realmId": realm.id.parse::<i64>().context(msg!(MissingRealmId))?,
                    "userId": self.api.user_id()?,
                    "auctionHouseType": auction_house(core::realm_name_and_faction(realm.game, &realm.name).1),
                });
                self.api
                    .post(&format!("{BACKEND}/user-realms"), body)
                    .await?;
                self.refresh_user_realms().await?;
                msg!(RealmAdded, name = realm.name)
            }
            Action::RemoveRealm(row) => {
                ensure!(row.removable, msg!(AutomaticRealmRemoval));
                let record = self
                    .user_realms
                    .iter()
                    .find(|(realm, _)| {
                        realm.game == row.game
                            && realm.region == row.region
                            && realm.name == row.name
                    })
                    .context(msg!(StaleRealm))?
                    .1
                    .clone();
                ensure!(
                    record
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                    msg!(InvalidResponse)
                );
                self.api.ensure_session().await?;
                self.api
                    .delete(&format!("{BACKEND}/user-realms/{record}"))
                    .await?;
                self.refresh_user_realms().await?;
                msg!(RealmRemoved, name = row.name)
            }
            Action::Export(account_key, realm, kind) => {
                self.scan()?;
                let account = self
                    .snapshot
                    .installation
                    .accounts
                    .iter()
                    .find(|a| a.protocol_name() == account_key)
                    .context(msg!(AccountNotFound))?;
                let destination = desktop::choose_export(
                    &format!("Accounting_{account_key}_{kind}.csv"),
                    self.language(),
                )
                .await?;
                ensure!(
                    destination
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case("csv")),
                    msg!(CsvExtension)
                );
                let count = wow::export(account, &realm, kind, &destination)?;
                msg!(
                    ExportCount,
                    count = count,
                    path = destination.display().to_string()
                )
            }
        })
    }
    pub fn refresh_account_state(&mut self) {
        self.snapshot.logged_in = self.api.logged_in();
        self.snapshot.premium = self.api.premium();
    }
    fn load_auction(&mut self, requested: Option<&Market>) -> Result<()> {
        self.scan()?;
        let hints: Vec<_> = self
            .snapshot
            .sync
            .iter()
            .map(|s| Market {
                game: s.game,
                scope: s.name.clone(),
                region: Some(s.region.clone()),
                region_only: s.name == s.region,
            })
            .collect();
        let catalog = crate::adapters::auction::load(
            &self.settings.wow_path,
            &self.snapshot.installation,
            requested,
            &hints,
            self.language(),
        )?;
        self.snapshot.auction = Some(Arc::new(catalog));
        Ok(())
    }
    /// Archives every account changed since its last backup. `force` archives all of them.
    /// Callers refresh `snapshot.backups` afterwards.
    fn backup_due(&mut self, force: bool, keep: bool) -> Result<usize> {
        self.last_backup_attempt = Instant::now();
        if wow::running() {
            if force {
                bail!(msg!(CloseWowBackup));
            }
            return Ok(0);
        }
        self.archive(force, keep)
    }
    /// Like `backup_due`, also while WoW runs: SavedVariables on disk only change on logout
    /// or `/reload`, so they are the last saved state.
    fn archive(&mut self, force: bool, keep: bool) -> Result<usize> {
        let previous = archive::local_backups(&self.paths, &self.settings)?;
        let mut count = 0;
        for account in &self.snapshot.installation.accounts {
            let last = previous
                .iter()
                .filter(|b| b.account == account.protocol_name())
                .map(|b| b.timestamp)
                .max()
                .unwrap_or(0);
            let modified = wow::modified(account)?;
            if core::backup_due(
                Utc::now().timestamp(),
                last,
                modified,
                self.settings.backup_hours as i64 * 3600,
                force,
            ) {
                archive::create(&self.paths, &self.settings, account, keep)?;
                count += 1;
            }
        }
        archive::prune(&self.paths, &self.settings)?;
        Ok(count)
    }
    async fn install(&self, game: Game, addon: &str) -> Result<()> {
        let url = self.addon_urls.get(addon).context(msg!(MissingAddonUrl))?;
        let bytes = self.api.fetch(url).await?;
        archive::install(&self.settings.wow_path, game, addon, &bytes)
    }
    /// Uploads local backups the cloud does not have yet and lists the cloud ones.
    async fn sync_backups(&mut self) -> Result<()> {
        if !self.api.premium() {
            return Ok(());
        }
        let list = self.api.get(&format!("{BACKEND}/backups")).await?;
        let mut remote: Vec<_> = remote_backups(&list)?.into_iter().map(|(b, _)| b).collect();
        let local = archive::local_backups(&self.paths, &self.settings)?;
        let user = self.api.user_id()?.to_owned();
        for backup in &local {
            let game = self
                .snapshot
                .installation
                .accounts
                .iter()
                .find(|a| core::same_account(&a.protocol_name(), &backup.account))
                .map(|a| a.game);
            let Some(game) = game else { continue };
            if remote
                .iter()
                .any(|r| r.remote_name() == backup.remote_name())
            {
                continue;
            }
            let record = backup_record(&user, backup, game);
            let slot = self
                .api
                .post(
                    &format!("{BACKEND}/backups/uploads"),
                    json!({
                        "userId": record["userId"],
                        "backupId": record["backupId"],
                        "type": record["type"],
                        "timestamp": record["timestamp"],
                    }),
                )
                .await?;
            let url = slot["url"].as_str().context(msg!(InvalidResponse))?;
            let bytes = archive::bytes(&self.paths, backup)?;
            self.api.upload(url, bytes).await?;
            self.api.post(&format!("{BACKEND}/backups"), record).await?;
            let mut r = backup.clone();
            r.remote = true;
            remote.push(r);
        }
        self.snapshot.backups = local;
        merge_backups(&mut self.snapshot.backups, remote);
        Ok(())
    }
    async fn sync(&mut self) -> Result<Text> {
        self.api.ensure_session().await?;
        self.inspect()?;
        let (feed, regions) = self.fetch_status().await?;
        let mut warnings = Vec::new();
        self.backup_due(false, false)?;
        if self.settings.auto_addons && !wow::running() {
            warnings.extend(self.update_addons().await?);
        }
        self.refresh_backups()?;
        let targets = sync_targets(
            &feed,
            &regions,
            &self.user_realms,
            &self.snapshot.installation.games,
        )?;
        ensure!(!targets.is_empty(), msg!(NoSyncRealms));
        let mut data = self.app_data()?;
        let changed = self.apply_prices(targets, &mut data).await?;
        self.write_app_info(&feed.app_info, data)?;
        if let Err(e) = self.sync_backups().await {
            warnings.push(msg!(CloudWarning, error = Text::from_error(&e)));
        }
        // ponytail: accounting upload (the share_data setting) is not ported yet: spec §6, sv-snapshots.
        if let Some(catalog) = &self.snapshot.auction {
            let selected = catalog.selected.clone();
            if let Err(error) = self.load_auction(selected.as_ref()) {
                warnings.push(msg!(AuctionRefreshFailed, error = Text::from_error(&error)));
            }
        }
        Ok(self.finish_sync(changed, warnings, Utc::now().timestamp()))
    }
    /// Realm catalog, subscriptions and the socket feed with prices, addons and news.
    async fn fetch_status(&mut self) -> Result<(Feed, Value)> {
        let realms = self.api.get(&format!("{REALM_API}/realms")).await?;
        let regions = self.api.get(&format!("{REALM_API}/regions")).await?;
        self.snapshot.realms = realm_options(&realms)?;
        self.refresh_user_realms().await?;
        let feed = self.api.feed().await?;
        self.apply_feed_info(&feed);
        Ok((feed, regions))
    }
    async fn refresh_user_realms(&mut self) -> Result<()> {
        let list = self
            .api
            .get(&format!(
                "{BACKEND}/user-realms?userIds={}",
                self.api.user_id()?
            ))
            .await?;
        self.user_realms = user_realms(&self.snapshot.realms, &list);
        self.snapshot.registered_realms = self.user_realms.iter().map(|(r, _)| r.clone()).collect();
        Ok(())
    }
    /// Channels from the account, the selected channel's addon versions and the news.
    fn apply_feed_info(&mut self, feed: &Feed) {
        self.snapshot.channels = self.api.channels();
        self.snapshot.latest.clear();
        self.addon_urls.clear();
        for addon in &feed.addons {
            if let (Some(name), Some(version), Some(url), true) = (
                addon["addon"].as_str(),
                addon["version"].as_str(),
                addon["url"].as_str(),
                addon["channel"].as_str() == Some(self.settings.channel.as_str()),
            ) {
                self.snapshot.latest.insert(name.into(), version.into());
                self.addon_urls.insert(name.into(), url.into());
            }
        }
        self.snapshot.news = news(&feed.app_info);
    }
    /// Backs up first, then installs every outdated addon. Failures become warnings.
    async fn update_addons(&mut self) -> Result<Vec<Text>> {
        let outdated: Vec<_> = self
            .snapshot
            .installation
            .addons
            .iter()
            .filter(|a| {
                self.snapshot
                    .latest
                    .get(&a.name)
                    .is_some_and(|latest| a.update_available(latest))
            })
            .cloned()
            .collect();
        if outdated.is_empty() {
            return Ok(Vec::new());
        }
        self.backup_due(true, true)?;
        let mut warnings = Vec::new();
        for addon in outdated {
            if let Err(e) = self.install(addon.game, &addon.name).await {
                warnings.push(msg!(
                    AddonWarning,
                    addon = addon.name,
                    error = Text::from_error(&e)
                ));
            }
        }
        self.scan()?;
        Ok(warnings)
    }
    fn app_data(&self) -> Result<BTreeMap<Game, (PathBuf, AppData)>> {
        let mut data = BTreeMap::new();
        for &game in &self.snapshot.installation.games {
            let path = wow::addon_dir(&self.settings.wow_path, game)
                .join("TradeSkillMaster_AppHelper/AppData.lua");
            if path.parent().is_some_and(|p| p.is_dir()) {
                let appdata = AppData::load(&path)?;
                data.insert(game, (path, appdata));
            }
        }
        ensure!(!data.is_empty(), msg!(AppHelperMissing));
        Ok(data)
    }
    /// Downloads newer price strings into `data` and rebuilds the sync rows.
    /// Returns how many strings changed. Download failures land on their row.
    async fn apply_prices(
        &mut self,
        targets: Vec<Target>,
        data: &mut BTreeMap<Game, (PathBuf, AppData)>,
    ) -> Result<usize> {
        let mut downloads = BTreeMap::new();
        let mut changed = 0;
        self.snapshot.sync.clear();
        for target in targets {
            let Some((_, appdata)) = data.get_mut(&target.game) else {
                continue;
            };
            let mut error = None;
            for (kind, (timestamp, url)) in &target.strings {
                let timestamp = *timestamp;
                if !core::price_update_due(timestamp, appdata.last(kind, &target.name)) {
                    continue;
                }
                if !downloads.contains_key(url) {
                    match self.api.download(url).await {
                        Ok(text) => {
                            downloads.insert(url.to_owned(), text);
                        }
                        Err(e) => {
                            error = Some(Text::from_error(&e));
                            continue;
                        }
                    }
                }
                if let Err(e) =
                    appdata.update(kind, &target.name, &downloads[url], timestamp, false)
                {
                    error = Some(Text::from_error(&e));
                } else {
                    changed += 1;
                }
            }
            let timestamp = appdata.last(
                if target.region_only {
                    "AUCTIONDB_REGION_STAT"
                } else {
                    "AUCTIONDB_NON_COMMODITY_DATA"
                },
                &target.name,
            );
            self.snapshot.sync.push(SyncRow {
                game: target.game,
                name: target.name,
                region: target.region,
                timestamp,
                error,
                removable: target.removable,
            });
        }
        Ok(changed)
    }
    /// Stamps the sync time, addon message and news, then writes every AppData.lua.
    fn write_app_info(
        &self,
        app_info: &Value,
        data: BTreeMap<Game, (PathBuf, AppData)>,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let info = app_info_lua(app_info, now);
        for (path, mut appdata) in data.into_values() {
            appdata.update("APP_INFO", "Global", &info, now, false)?;
            appdata.save(&self.settings.wow_path, &path)?;
        }
        Ok(())
    }
    fn finish_sync(&mut self, changed: usize, warnings: Vec<Text>, now: i64) -> Text {
        let failures: Vec<_> = self
            .snapshot
            .sync
            .iter()
            .filter_map(|row| {
                row.error.as_ref().map(|error| {
                    msg!(
                        PriceWarning,
                        region = &row.region,
                        name = &row.name,
                        error = error
                    )
                })
            })
            .collect();
        let problems = failures.len() + warnings.len();
        for problem in failures.into_iter().chain(warnings) {
            self.log_error(&problem);
        }
        if problems > 0 {
            msg!(
                PartialSync,
                updates = msg!(UpdateCount, count = changed),
                problems = msg!(ProblemCount, count = problems)
            )
        } else {
            self.snapshot.last_sync = Some(now);
            msg!(SyncComplete, updates = msg!(UpdateCount, count = changed))
        }
    }
}
/// The `APP_INFO` table the addon loads: `message` and an array of `news` entries.
/// `lua::quote` escapes quotes, backslashes and control characters, and `AppData::update`
/// picks a long-bracket level that the text cannot terminate.
fn app_info_lua(app_info: &Value, now: i64) -> String {
    let news: Vec<_> = app_info["news"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            format!(
                "{{title={},content={},timestamp={},link={}}}",
                lua::quote(item["title"].as_str().unwrap_or("")),
                lua::quote(item["content"].as_str().unwrap_or("")),
                item["timestamp"].as_i64().unwrap_or(0),
                lua::quote(item["link"].as_str().unwrap_or(""))
            )
        })
        .collect();
    format!(
        "{{version={},lastSync={now},message={{id={},msg={}}},news={{{}}}}}",
        lua::quote(env!("CARGO_PKG_VERSION")),
        app_info["message"]["id"].as_i64().unwrap_or(0),
        lua::quote(app_info["message"]["msg"].as_str().unwrap_or("")),
        news.join(",")
    )
}
fn news(app_info: &Value) -> String {
    app_info["news"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            format!(
                "{}\n{}",
                item["title"].as_str().unwrap_or(""),
                strip_html(item["content"].as_str().unwrap_or(""))
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
pub fn strip_html(s: &str) -> String {
    let mut in_tag = false;
    s.chars()
        .filter(|c| {
            if *c == '<' {
                in_tag = true;
                false
            } else if *c == '>' {
                in_tag = false;
                false
            } else {
                !in_tag
            }
        })
        .collect()
}
/// The API sends ids as numbers.
fn id_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|x| x.to_string()))
}
/// Auction house type of the realm API (`All`) and of the socket and user realms (`ALL`).
fn faction(auction_house: &str) -> Option<Faction> {
    match auction_house.to_ascii_lowercase().as_str() {
        "alliance" => Some(Faction::Alliance),
        "horde" => Some(Faction::Horde),
        _ => None,
    }
}
fn auction_house(faction: Option<Faction>) -> &'static str {
    match faction {
        Some(Faction::Alliance) => "ALLIANCE",
        Some(Faction::Horde) => "HORDE",
        None => "ALL",
    }
}
/// Game and AppData region key (`Fresh-EU`) of a realm API region.
fn region_key(region: &Value) -> Option<(Game, String)> {
    let (game, prefix) = Game::from_version(region["gameVersion"].as_str()?)?;
    Some((
        game,
        format!(
            "{prefix}{}",
            region["regionPrefix"].as_str()?.to_uppercase()
        ),
    ))
}
/// One entry per realm and auction house. Factions of the classic versions are separate
/// entries named `Realm-Alliance`/`Realm-Horde`, like their AppData keys.
fn realm_options(list: &Value) -> Result<Vec<Realm>> {
    let mut realms = Vec::new();
    for realm in list["items"].as_array().context(msg!(InvalidRealmList))? {
        let Some((game, region)) = region_key(&realm["region"]) else {
            continue;
        };
        let id = id_string(&realm["realmId"]).context(msg!(MissingRealmId))?;
        let name = realm["localizedName"]
            .as_str()
            .context(msg!(MissingRealmName))?;
        for house in realm["auctionHouses"].as_array().into_iter().flatten() {
            let name = match faction(house["type"].as_str().unwrap_or("")) {
                Some(faction) => format!("{name}-{}", faction_name(faction)),
                None => name.to_owned(),
            };
            realms.push(Realm {
                game,
                id: id.clone(),
                region: region.clone(),
                name,
            });
        }
    }
    realms.sort_by_key(ToString::to_string);
    Ok(realms)
}
fn faction_name(faction: Faction) -> &'static str {
    match faction {
        Faction::Alliance => "Alliance",
        Faction::Horde => "Horde",
    }
}
/// The catalog realm of a realm id and auction house type.
fn find_realm<'a>(realms: &'a [Realm], id: &Value, auction_house: &Value) -> Option<&'a Realm> {
    let id = id_string(id)?;
    let faction = faction(auction_house.as_str()?);
    realms.iter().find(|realm| {
        realm.id == id && core::realm_name_and_faction(realm.game, &realm.name).1 == faction
    })
}
/// Subscribed catalog realms with their subscription record ids.
fn user_realms(realms: &[Realm], list: &Value) -> Vec<(Realm, String)> {
    list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let realm = find_realm(realms, &item["realmId"], &item["auctionHouseType"])?;
            Some((realm.clone(), item["id"].as_str()?.to_owned()))
        })
        .collect()
}
#[derive(Debug)]
struct Target {
    game: Game,
    name: String,
    region: String,
    region_only: bool,
    removable: bool,
    /// String type to (`lastModified`, URL).
    strings: BTreeMap<String, (i64, String)>,
}
/// The price strings of one socket event kind, for the events accepted by `matches`.
fn price_strings(
    events: &[Value],
    matches: impl Fn(&Value) -> bool,
) -> Result<BTreeMap<String, (i64, String)>> {
    events
        .iter()
        .filter(|event| matches(event))
        .map(|event| {
            // `lastModified` is a number or a string.
            let time = event["lastModified"]
                .as_i64()
                .or_else(|| event["lastModified"].as_str()?.parse().ok())
                .context(msg!(MissingPriceTime))?;
            Ok((
                event["stringType"]
                    .as_str()
                    .context(msg!(MissingPriceUrl))?
                    .to_owned(),
                (
                    time,
                    event["url"].as_str().context(msg!(MissingPriceUrl))?.into(),
                ),
            ))
        })
        .collect()
}
/// A row per subscribed realm, then one per region the socket sent data for.
fn sync_targets(
    feed: &Feed,
    regions: &Value,
    subscribed: &[(Realm, String)],
    games: &[Game],
) -> Result<Vec<Target>> {
    let mut result = Vec::new();
    let catalog: Vec<_> = subscribed.iter().map(|(realm, _)| realm.clone()).collect();
    for (realm, _) in subscribed.iter().filter(|(r, _)| games.contains(&r.game)) {
        let strings = price_strings(&feed.realms, |event| {
            find_realm(&catalog, &event["realmId"], &event["auctionHouseType"]) == Some(realm)
        })?;
        result.push(Target {
            game: realm.game,
            name: realm.name.clone(),
            region: realm.region.clone(),
            region_only: false,
            removable: true,
            strings,
        });
    }
    for region in regions["items"].as_array().into_iter().flatten() {
        let Some((game, key)) = region_key(region) else {
            continue;
        };
        let id = id_string(&region["regionId"]);
        let strings = price_strings(&feed.regions, |event| id_string(&event["regionId"]) == id)?;
        if games.contains(&game) && !strings.is_empty() {
            result.push(Target {
                game,
                name: key.clone(),
                region: key,
                region_only: true,
                removable: false,
                strings,
            });
        }
    }
    Ok(result)
}
/// Backups this app uploaded carry this description. The official app's cloud backups are
/// JSON patches of the parsed SavedVariables, which are not zips and cannot be restored here.
const BACKUP_MARKER: &str = "Tradewind backup";
/// The cloud record of a full (`original`) backup zip. `gameVersion` is the WoW folder.
// ponytail: field names follow the official app's upload and restore code; the server is unverified.
fn backup_record(user: &str, backup: &Backup, game: Game) -> Value {
    let time = Utc
        .timestamp_opt(backup.timestamp, 0)
        .single()
        .unwrap_or_default();
    json!({
        "userId": user,
        "type": "original",
        "backupId": backup.remote_name().trim_end_matches(".zip"),
        "previousId": null,
        "gameVersion": game.folder(),
        "account": backup.account,
        "systemId": backup.system_id,
        "description": BACKUP_MARKER,
        "timestamp": time.to_rfc3339_opts(SecondsFormat::Millis, true),
    })
}
/// Cloud backups made by this app with their download URL.
// ponytail: only the first page of `items` is read; `metadata` paging is unverified.
fn remote_backups(list: &Value) -> Result<Vec<(Backup, String)>> {
    let mut result = Vec::new();
    for item in list["items"].as_array().context(msg!(MissingCloudData))? {
        if !item["description"]
            .as_str()
            .is_some_and(|d| d.starts_with(BACKUP_MARKER))
        {
            continue;
        }
        let (Some(account), Some(system), Some(url)) = (
            item["account"].as_str(),
            item["systemId"].as_str(),
            item["url"].as_str(),
        ) else {
            continue;
        };
        let timestamp = item["timestamp"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .context(msg!(MissingCloudTime))?
            .timestamp();
        let backup = Backup {
            account: account.into(),
            system_id: system.into(),
            timestamp,
            local: None,
            remote: true,
            keep: false,
        };
        backup.id().validate()?;
        result.push((backup, url.to_owned()));
    }
    Ok(result)
}
fn merge_backups(local: &mut Vec<Backup>, remote: Vec<Backup>) {
    for r in remote {
        if let Some(l) = local
            .iter_mut()
            .find(|l| l.remote_name() == r.remote_name())
        {
            l.remote = true;
            l.keep |= r.keep;
        } else {
            local.push(r);
        }
    }
    local.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{fs, time::Duration};

    fn engine(root: &std::path::Path, settings: Settings) -> Engine {
        Engine::new(Paths::in_dir(root), settings).unwrap()
    }
    fn realms() -> Vec<Realm> {
        serde_json::from_str(include_str!("../../tests/fixtures/realm-selection.json")).unwrap()
    }

    #[test]
    fn partial_sync_logs_realm_errors_and_preserves_last_complete_sync() {
        let root = tempfile::tempdir().unwrap();
        let mut engine = engine(root.path(), Settings::default());
        engine.snapshot.last_sync = Some(100);
        engine.snapshot.sync = vec![SyncRow {
            game: Game::Anniversary,
            name: "Test Realm-Horde".into(),
            region: "Fresh-EU".into(),
            timestamp: 90,
            error: Some(msg!(DownloadHostRejected, host = "fixture.example")),
            removable: false,
        }];
        let status = engine.finish_sync(
            0,
            vec![msg!(CloudWarning, error = msg!(TransportTimeout))],
            200,
        );
        assert!(status.render(Language::German).contains("2 Probleme"));
        assert_eq!(engine.snapshot.last_sync, Some(100));
        assert!(engine.snapshot.log.iter().all(|line| line.error));
        let log = fs::read_to_string(engine.paths.data.join("activity.log")).unwrap();
        assert!(log.contains(
            "Price data Fresh-EU · Test Realm-Horde: Download host is not allowed: fixture.example"
        ));
        assert!(log.contains("Cloud backups: Connection or transfer timed out."));
        engine.snapshot.sync[0].error = None;
        assert_eq!(
            engine.finish_sync(3, vec![], 300).render(Language::German),
            "Synchronisiert · 3 Datensätze aktualisiert"
        );
        assert_eq!(engine.snapshot.last_sync, Some(300));
    }

    fn feed() -> Feed {
        let mut feed = Feed::default();
        for packet in
            crate::adapters::tsm::packets(include_str!("../../tests/fixtures/socket-poll.txt"))
        {
            if let crate::adapters::tsm::Packet::Event(name, payload) = packet {
                feed.absorb(&name, payload);
            }
        }
        feed
    }
    fn catalog() -> Vec<Realm> {
        realm_options(
            &serde_json::from_str(include_str!("../../tests/fixtures/realms-v2.json")).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn realm_catalog_names_regions_and_factions_follow_the_appdata_keys() {
        let realms = catalog();
        let key = |game, name: &str| {
            realms
                .iter()
                .find(|r| r.game == game && r.name == name)
                .map(|r| (r.region.as_str(), r.id.as_str()))
        };
        assert_eq!(key(Game::Retail, "Blackrock"), Some(("EU", "275")));
        assert_eq!(
            key(Game::ClassicEra, "Transcendence-Alliance"),
            Some(("Classic-EU", "871"))
        );
        assert_eq!(
            key(Game::ClassicEra, "Transcendence-Horde"),
            Some(("Classic-EU", "871"))
        );
        assert_eq!(
            key(Game::ClassicEra, "Doomhowl-Horde"),
            Some(("HC-US", "1808"))
        );
        assert_eq!(
            key(Game::ClassicEra, "Penance (AU)-Alliance"),
            Some(("SoD-US", "1059"))
        );
        assert_eq!(
            key(Game::Anniversary, "Thunderstrike-Horde"),
            Some(("Fresh-EU", "1809"))
        );
        assert_eq!(
            key(Game::Progression, "Atiesh-Alliance"),
            Some(("BCC-US", "665"))
        );
        assert_eq!(
            key(Game::Forever, "Normal-Horde"),
            Some(("Forever-EU", "1101"))
        );
        assert!(key(Game::Retail, "Blackrock-Horde").is_none());
        assert_eq!(realms.len(), 2 + 2 * 6);
    }

    #[test]
    fn socket_feed_becomes_realm_and_region_targets_for_installed_games_only() {
        let realms = catalog();
        let list: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/user-realms.json")).unwrap();
        let subscribed = user_realms(&realms, &list);
        assert_eq!(subscribed.len(), 1);
        assert_eq!(subscribed[0].0.name, "Blackrock");
        assert_eq!(subscribed[0].1, "usr_rlm_00000000000000000000000000");
        let regions: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/regions-v2.json")).unwrap();
        let targets = sync_targets(
            &feed(),
            &regions,
            &subscribed,
            &[Game::Retail, Game::Forever],
        )
        .unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(
            (targets[0].name.as_str(), targets[0].region.as_str()),
            ("Blackrock", "EU")
        );
        assert!(targets[0].removable && !targets[0].region_only);
        assert_eq!(targets[0].strings.len(), 2);
        // `lastModified` arrives as a number and as a string.
        assert_eq!(
            targets[0].strings["AUCTIONDB_NON_COMMODITY_DATA"].0,
            1793985712
        );
        assert_eq!(
            targets[0].strings["AUCTIONDB_NON_COMMODITY_HISTORICAL"].0,
            1793929249
        );
        assert!(
            !targets[0].strings["AUCTIONDB_NON_COMMODITY_DATA"]
                .1
                .contains('?')
        );
        assert_eq!(
            (
                targets[1].name.as_str(),
                targets[1].region_only,
                targets[1].removable
            ),
            ("EU", true, false)
        );
        assert_eq!(targets[1].strings.len(), 1);
        assert!(
            sync_targets(&feed(), &regions, &subscribed, &[Game::Forever])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn feed_info_gives_versions_urls_and_news_of_the_selected_channel() {
        let root = tempfile::tempdir().unwrap();
        let mut engine = engine(root.path(), Settings::default());
        engine.apply_feed_info(&feed());
        assert_eq!(engine.snapshot.latest["TradeSkillMaster"], "v4.15.3");
        assert!(engine.addon_urls["TradeSkillMaster_AppHelper"].ends_with("AppHelper-v4.15.zip"));
        assert!(engine.snapshot.news.starts_with("TSM 4.15 brings"));
        engine.settings.channel = "beta".into();
        engine.apply_feed_info(&feed());
        assert!(engine.snapshot.latest.is_empty());
    }

    #[test]
    fn app_info_is_a_string_table_with_escaped_text_and_a_news_array() {
        let info = json!({"message":{"id":3,"msg":"say \"hi\"\n]]"},
            "news":[{"title":"A\\B","content":"line]]\nbreak","timestamp":7,"link":"https://tradeskillmaster.com/x"},
                    {"title":"T","content":"C","timestamp":8,"link":"L"}]});
        let line = app_info_lua(&info, 99);
        assert!(line.starts_with(&format!(
            "{{version=\"{}\",lastSync=99,message={{id=3,msg=\"say \\\"hi\\\"\\010]]\"}}",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(line.contains("news={{title=\"A\\\\B\",content=\"line]]\\010break\",timestamp=7,link=\"https://tradeskillmaster.com/x\"},{title=\"T\""));
        let mut data = AppData::default();
        data.update("APP_INFO", "Global", &line, 99, false).unwrap();
        let (_, _, _, body) = data.records().next().unwrap();
        // `]]` inside the text forces a level-1 long bracket.
        assert!(body.contains("[=[return {version="), "{body}");
        assert!(body.ends_with("}]=])"));
        assert_eq!(app_info_lua(&json!({}), 1).matches("news={}").count(), 1);
    }

    #[test]
    fn cloud_backups_round_trip_through_the_records_this_app_uploads() {
        let backup = Backup {
            account: "ACCOUNT-Anniversary".into(),
            system_id: "abc".into(),
            timestamp: 1700000000,
            local: None,
            remote: false,
            keep: false,
        };
        let record = backup_record("user", &backup, Game::Anniversary);
        assert_eq!(record["backupId"], "abc_ACCOUNT-Anniversary_1700000000");
        assert_eq!(record["gameVersion"], "_anniversary_");
        assert_eq!(record["type"], "original");
        assert_eq!(record["previousId"], Value::Null);
        assert_eq!(record["timestamp"], "2023-11-14T22:13:20.000Z");
        let mut listed: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/cloud.json")).unwrap();
        let remote = remote_backups(&listed).unwrap();
        // The official app's patch records and url-less entries are not ours.
        assert_eq!(remote.len(), 1);
        assert_eq!(
            remote[0].0.remote_name(),
            "abc_ACCOUNT-Anniversary_1700000000.zip"
        );
        assert!(
            remote[0]
                .1
                .starts_with("https://tradeskillmaster.nyc3.digitaloceanspaces.com/")
        );
        listed["items"][0]["timestamp"] = json!("yesterday");
        assert!(remote_backups(&listed).is_err());
        listed["items"][0]["timestamp"] = json!("2023-11-14T22:13:20.000Z");
        listed["items"][0]["account"] = json!("../ACCOUNT");
        assert!(remote_backups(&listed).is_err());
        assert!(remote_backups(&json!({"metadata":{}})).is_err());
    }

    #[tokio::test]
    async fn local_backups_run_without_login_when_network_sync_is_disabled() {
        let root = tempfile::tempdir().unwrap();
        let account = root
            .path()
            .join("_retail_/WTF/Account/ACCOUNT/SavedVariables");
        fs::create_dir_all(&account).unwrap();
        fs::write(
            account.join("TradeSkillMaster.lua"),
            include_str!("../../tests/fixtures/savedvariables.lua"),
        )
        .unwrap();
        let settings = Settings {
            wow_path: root.path().into(),
            auto_sync: false,
            notifications: false,
            ..Default::default()
        };
        let mut engine = engine(root.path(), settings);
        engine.last_backup_attempt = Instant::now() - Duration::from_secs(3601);
        assert!(!engine.sync_due());
        let coordinator = crate::application::Coordinator::with_engine(engine);
        let outcome = coordinator.tick().await.unwrap();
        assert!(outcome.result.is_ok());
        assert!(!outcome.snapshot.logged_in);
        assert_eq!(outcome.snapshot.backups.len(), 1);
        assert!(coordinator.tick().await.is_none());
    }

    #[test]
    fn subscriptions_are_scoped_by_game_region_and_faction() {
        let catalog = realms();
        let snapshot = Snapshot {
            registered_realms: vec![catalog[4].clone(), catalog[0].clone()],
            ..Default::default()
        };
        assert!(snapshot.realm_registered(&catalog[0]));
        assert!(!snapshot.realm_registered(&catalog[1]));
        assert!(!snapshot.realm_registered(&catalog[2]));
        assert!(!snapshot.realm_registered(&catalog[3]));
        assert!(snapshot.realm_registered(&catalog[4]));
    }

    #[tokio::test]
    async fn duplicate_and_stale_realms_are_rejected_before_authentication_or_network() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = engine(dir.path(), Settings::default());
        engine.snapshot.realms = realms();
        let realm = engine.snapshot.realms[0].clone();
        engine.snapshot.registered_realms.push(realm.clone());
        assert_eq!(
            engine.run(Action::AddRealm(realm.clone())).await.unwrap(),
            msg!(RealmAlreadyAdded, name = &realm.name)
        );
        let mut stale = realm;
        stale.id = "not-in-catalog".into();
        let error = engine.run(Action::AddRealm(stale)).await.unwrap_err();
        assert_eq!(Text::from_error(&error), msg!(StaleRealm));
        assert_eq!(engine.snapshot.registered_realms.len(), 1);
    }

    #[tokio::test]
    async fn page_saves_keep_language_filters_and_consent_and_bump_the_revision() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("_retail_")).unwrap();
        let settings = Settings {
            wow_path: dir.path().into(),
            ..Default::default()
        };
        let mut engine = engine(dir.path(), settings.clone());
        let filters = RealmFilters {
            game: Some(Game::Progression),
            region: Some("EU".into()),
            faction: Some(core::Faction::Horde),
        };
        engine
            .run(Action::SaveRealmFilters(filters.clone()))
            .await
            .unwrap();
        engine
            .run(Action::SetLanguage(LanguagePreference::German))
            .await
            .unwrap();
        let mut preferences = settings.preferences();
        preferences.retention_days = 7;
        engine.run(Action::Save(preferences)).await.unwrap();
        assert_eq!(engine.revision, 3);
        let reloaded = engine.paths.load().unwrap();
        assert_eq!(reloaded.realm_filters, filters);
        assert_eq!(reloaded.language, LanguagePreference::German);
        assert_eq!(reloaded.retention_days, 7);
        let disk = fs::read_to_string(engine.paths.config.join("settings.json")).unwrap();
        assert!(disk.contains("\"horde\""));
        assert!(disk.contains("\"german\""));
    }

    #[tokio::test]
    async fn offline_browsing_and_watchlist_survive_drafts_without_wow_writes() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir
            .path()
            .join("_retail_/Interface/AddOns/TradeSkillMaster_AppHelper");
        fs::create_dir_all(&helper).unwrap();
        let path = helper.join("AppData.lua");
        let original = include_bytes!("../../tests/fixtures/auction-appdata.lua");
        fs::write(&path, original).unwrap();
        let settings = Settings {
            wow_path: dir.path().into(),
            ..Settings::default()
        };
        let mut engine = engine(dir.path(), settings.clone());
        let browse = Action::BrowseMarket(None);
        assert!(!browse.writes_wow_files());
        engine.run(browse).await.unwrap();
        assert!(!engine.snapshot.logged_in);
        let catalog = engine.snapshot.auction.as_ref().unwrap();
        assert_eq!(catalog.items.len(), 1);
        let favorite = Favorite {
            game: Game::Retail,
            item: "i:100".into(),
        };
        engine
            .run(Action::ToggleFavorite(favorite.clone()))
            .await
            .unwrap();
        engine
            .run(Action::Save(settings.preferences()))
            .await
            .unwrap();
        assert_eq!(
            engine.paths.load().unwrap().auction_favorites,
            std::slice::from_ref(&favorite)
        );
        engine.run(Action::BrowseMarket(None)).await.unwrap();
        let mut invalid = favorite.clone();
        invalid.game = Game::ClassicEra;
        assert!(engine.run(Action::ToggleFavorite(invalid)).await.is_err());
        assert_eq!(
            engine.settings.auction_favorites,
            std::slice::from_ref(&favorite)
        );
        engine.run(Action::ToggleFavorite(favorite)).await.unwrap();
        assert!(engine.paths.load().unwrap().auction_favorites.is_empty());
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
