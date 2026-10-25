use crate::i18n::{Language, LanguagePreference, Text};
use crate::{
    adapters::{
        archives::{self as archive, Backup},
        desktop,
        files::{self as storage, Paths, Preferences, Settings},
        lua,
        tsm::{Api, Body, PROTOCOL_VERSION, encoded},
        uploads,
        wow::{self, AppData, Installation},
    },
    core::{
        self, ExportKind, Game, Realm, RealmFilters,
        auction::{Catalog, Favorite, Market},
    },
};
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde_json::Value;
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
    Login {
        email: String,
        password: String,
        remember: bool,
        terms: bool,
        legacy_http: bool,
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
    UploadLog,
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
    pub snapshot: Snapshot,
    last_attempt: Instant,
    last_backup_attempt: Instant,
    /// Shown instead of the bootstrap result once, e.g. after damaged settings were reset.
    pub(crate) startup_warning: Option<Text>,
}
impl Engine {
    pub(crate) fn new(paths: Paths, settings: Settings) -> Result<Self> {
        let api = Api::new(settings.legacy_http)?;
        Ok(Self {
            settings,
            revision: 0,
            paths,
            api,
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
                    self.api.resume(self.settings.email.clone()).await?;
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
            Action::Login {
                email,
                password,
                remember,
                terms,
                legacy_http,
            } => {
                ensure!(terms, msg!(TermsRequired));
                self.api = Api::new(legacy_http)?;
                let warning = self.api.login(email.clone(), password, remember).await?;
                let mut settings = self.settings.clone();
                settings.terms_accepted = terms;
                settings.remember_login = remember;
                settings.legacy_http = legacy_http;
                settings.email = email;
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
                self.scan()?;
                self.backup_due(true, true)?;
                self.install(game, &addon).await?;
                self.scan()?;
                self.refresh_backups()?;
                msg!(AddonInstalled, addon = addon, game = game.label())
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
                    let bytes = self
                        .api
                        .binary(&["backup", &encoded(&backup.remote_name())], None)
                        .await?;
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
                self.api
                    .get(&["realms2", "add", realm.game.protocol(), &realm.id])
                    .await?;
                self.snapshot.registered_realms.push(realm.clone());
                msg!(RealmAdded, name = realm.name)
            }
            Action::RemoveRealm(row) => {
                ensure!(row.removable, msg!(AutomaticRealmRemoval));
                self.api.ensure_session().await?;
                self.api
                    .get(&[
                        "realms2",
                        "remove",
                        row.game.protocol(),
                        &row.region,
                        &row.name,
                    ])
                    .await?;
                self.snapshot.registered_realms.retain(|realm| {
                    realm.game != row.game || realm.region != row.region || realm.name != row.name
                });
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
            Action::UploadLog => {
                self.api.ensure_session().await?;
                self.api
                    .post(&["log", "user"], Body::Text(self.rendered_log()))
                    .await?;
                msg!(LogUploaded)
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
        let name = format!("{addon}{}", game.account_suffix());
        let bytes = self
            .api
            .binary(&["addon", &name], Some(&self.settings.channel))
            .await?;
        archive::install(&self.settings.wow_path, game, addon, &bytes)
    }
    async fn sync_backups(&mut self) -> Result<()> {
        if !self.api.premium() {
            return Ok(());
        }
        let info = self.api.get(&["backup"]).await?;
        let mut remote = remote_backups(&info)?;
        let missing = info["missing"]
            .as_array()
            .context(msg!(MissingBackupList))?;
        let local = archive::local_backups(&self.paths, &self.settings)?;
        for backup in &local {
            let name = backup.remote_name();
            let stem = name.trim_end_matches(".zip");
            let known = remote.iter().any(|r| r.remote_name() == name);
            if missing.iter().any(|v| v.as_str() == Some(stem)) || !known {
                let bytes = archive::bytes(&self.paths, backup)?;
                self.api
                    .post(&["backup", &encoded(&name)], Body::Binary(bytes))
                    .await?;
                let mut r = backup.clone();
                r.remote = true;
                remote.push(r);
            }
        }
        self.snapshot.backups = local;
        merge_backups(&mut self.snapshot.backups, remote);
        Ok(())
    }
    async fn sync(&mut self) -> Result<Text> {
        self.api.ensure_session().await?;
        self.inspect()?;
        let status = self.fetch_status().await?;
        let mut warnings = Vec::new();
        self.backup_due(false, false)?;
        if self.settings.auto_addons && !wow::running() {
            warnings.extend(self.update_addons().await?);
        }
        self.refresh_backups()?;
        let targets = sync_targets(&status, &self.snapshot.installation)?;
        ensure!(!targets.is_empty(), msg!(NoSyncRealms));
        let mut data = self.app_data()?;
        let changed = self.apply_prices(targets, &mut data).await?;
        self.write_app_info(&status["appInfo"], data)?;
        if let Err(e) = self.sync_backups().await {
            warnings.push(msg!(CloudWarning, error = Text::from_error(&e)));
        }
        if self.settings.share_data {
            warnings.extend(self.upload_accounting().await);
        }
        if let Some(catalog) = &self.snapshot.auction {
            let selected = catalog.selected.clone();
            if let Err(error) = self.load_auction(selected.as_ref()) {
                warnings.push(msg!(AuctionRefreshFailed, error = Text::from_error(&error)));
            }
        }
        Ok(self.finish_sync(changed, warnings, Utc::now().timestamp()))
    }
    /// Realm catalog, subscriptions, channels, latest addon versions and news.
    async fn fetch_status(&mut self) -> Result<Value> {
        let realm_list = self.api.get(&["realms2", "list"]).await?;
        self.snapshot.realms = realm_options(&realm_list)?;
        let version = self
            .snapshot
            .installation
            .addons
            .iter()
            .find(|a| a.name == "TradeSkillMaster" && a.game == Game::Retail)
            .and_then(|a| a.version.as_deref());
        let status = self.api.status(&self.settings.channel, version).await?;
        self.snapshot.registered_realms = registered_realms(&self.snapshot.realms, &status)?;
        self.snapshot.channels = channels(&status)?;
        self.snapshot.latest = latest_versions(&status)?;
        self.snapshot.news = strip_html(status["appInfo"]["news"].as_str().unwrap_or(""));
        Ok(status)
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
            for (kind, info) in &target.strings {
                let timestamp = info["lastModified"]
                    .as_i64()
                    .context(msg!(MissingPriceTime))?;
                if !core::price_update_due(timestamp, appdata.last(kind, &target.name)) {
                    continue;
                }
                let url = info["url"].as_str().context(msg!(MissingPriceUrl))?;
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
        let info = format!(
            "{{version={PROTOCOL_VERSION},lastSync={now},message={},news={}}}",
            lua_message(&app_info["addonMessage"]),
            lua_message(&app_info["addonNews"])
        );
        for (path, mut appdata) in data.into_values() {
            appdata.update("APP_INFO", "Global", &info, now, true)?;
            appdata.save(&self.settings.wow_path, &path)?;
        }
        Ok(())
    }
    async fn upload_accounting(&self) -> Vec<Text> {
        let mut warnings = Vec::new();
        for account in &self.snapshot.installation.accounts {
            if let Err(e) = uploads::upload(&self.api, account, &self.settings.system_id).await {
                warnings.push(msg!(
                    UploadWarning,
                    game = account.game.label(),
                    error = Text::from_error(&e)
                ));
            }
        }
        warnings
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
fn lua_message(value: &Value) -> String {
    format!(
        "{{id={},msg={}}}",
        value["id"].as_i64().unwrap_or(0),
        lua::quote(value["msg"].as_str().unwrap_or(""))
    )
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
fn channels(status: &Value) -> Result<Vec<String>> {
    Ok(status["channels"]
        .as_object()
        .context(msg!(MissingChannels))?
        .keys()
        .cloned()
        .collect())
}
fn latest_versions(status: &Value) -> Result<BTreeMap<String, String>> {
    status["addons"]
        .as_array()
        .context(msg!(MissingAddonsResponse))?
        .iter()
        .map(|addon| {
            Ok((
                addon["name"]
                    .as_str()
                    .context(msg!(MissingAddonName))?
                    .into(),
                addon["version_str"]
                    .as_str()
                    .context(msg!(MissingAddonVersion))?
                    .into(),
            ))
        })
        .collect()
}
/// The versions whose realms are subscribed through the account, with their status key.
fn subscribable() -> impl Iterator<Item = (Game, &'static str)> {
    Game::ALL
        .into_iter()
        .filter(|game| game.subscribable())
        .filter_map(|game| Some((game, game.status_keys()?.0)))
}
fn registered_realms(realms: &[Realm], status: &Value) -> Result<Vec<Realm>> {
    let mut registered = Vec::new();
    for (game, key) in subscribable() {
        let entries = status[key].as_array().context(msg!(MissingSubscriptions))?;
        registered.extend(
            realms
                .iter()
                .filter(|realm| {
                    realm.game == game
                        && entries.iter().any(|entry| {
                            entry["name"].as_str() == Some(realm.name.as_str())
                                && entry["region"].as_str() == Some(realm.region.as_str())
                        })
                })
                .cloned(),
        );
    }
    Ok(registered)
}
fn realm_options(value: &Value) -> Result<Vec<Realm>> {
    let mut realms = Vec::new();
    for (game, _) in subscribable() {
        for realm in value[game.protocol()]
            .as_array()
            .context(msg!(InvalidRealmList))?
        {
            let id = realm["id"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| realm["id"].as_i64().map(|x| x.to_string()))
                .context(msg!(MissingRealmId))?;
            realms.push(Realm {
                game,
                id,
                region: realm["region"]
                    .as_str()
                    .context(msg!(MissingRealmRegion))?
                    .into(),
                name: realm["name"]
                    .as_str()
                    .context(msg!(MissingRealmName))?
                    .into(),
            });
        }
    }
    realms.sort_by_key(ToString::to_string);
    Ok(realms)
}
#[derive(Debug)]
struct Target {
    game: Game,
    name: String,
    region: String,
    region_only: bool,
    removable: bool,
    strings: BTreeMap<String, Value>,
}
fn sync_targets(status: &Value, installation: &Installation) -> Result<Vec<Target>> {
    let mut result = Vec::new();
    for &game in &installation.games {
        let Some((realms_key, regions_key)) = game.status_keys() else {
            continue;
        };
        // Unsubscribable versions sync the realms the local SavedVariables mention.
        let active = if game.subscribable() {
            Vec::new()
        } else {
            wow::active_realms(&installation.accounts, game)?
        };
        for (key, region_only) in [(realms_key, false), (regions_key, true)] {
            for entry in status[key].as_array().context(msg!(MissingStatusRealms))? {
                let name = entry["name"].as_str().context(msg!(MissingSyncName))?;
                let region = entry["region"].as_str().unwrap_or(name);
                if !game.subscribable() {
                    let played = if region_only {
                        active
                            .iter()
                            .any(|(r, _)| name.split('-').nth(1) == Some(r.as_str()))
                    } else {
                        active.iter().any(|(r, n)| region.ends_with(r) && n == name)
                    };
                    if !played {
                        continue;
                    }
                }
                let strings = entry["appDataStrings"]
                    .as_object()
                    .context(msg!(MissingAppDataStrings))?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                result.push(Target {
                    game,
                    name: name.into(),
                    region: region.into(),
                    region_only,
                    removable: core::can_remove_realm(game, region, region_only),
                    strings,
                });
            }
        }
    }
    Ok(result)
}
fn remote_backups(info: &Value) -> Result<Vec<Backup>> {
    let mut result = Vec::new();
    for (key, list) in info["data"].as_object().context(msg!(MissingCloudData))? {
        let (system, account) = key.split_once('_').context(msg!(InvalidCloudKey))?;
        core::BackupId {
            account: account.into(),
            system_id: system.into(),
            timestamp: 1,
        }
        .validate()?;
        for item in list.as_array().context(msg!(InvalidCloudList))? {
            let timestamp = item["timestamp"].as_i64().context(msg!(MissingCloudTime))?;
            core::BackupId {
                account: account.into(),
                system_id: system.into(),
                timestamp,
            }
            .validate()?;
            result.push(Backup {
                account: account.into(),
                system_id: system.into(),
                timestamp,
                local: None,
                remote: true,
                keep: item["keep"].as_bool().unwrap_or(false),
            });
        }
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

    #[test]
    fn status_cloud_and_realm_fixtures_cover_all_game_variants() {
        let root = tempfile::tempdir().unwrap();
        for game in Game::ALL {
            let path = root
                .path()
                .join(game.folder())
                .join("WTF/Account/ACCOUNT/SavedVariables");
            fs::create_dir_all(&path).unwrap();
            fs::write(
                path.join("TradeSkillMaster.lua"),
                include_str!("../../tests/fixtures/savedvariables.lua"),
            )
            .unwrap();
            fs::write(
                path.join("TradeSkillMaster_AppHelper.lua"),
                include_str!("../../tests/fixtures/helper.lua"),
            )
            .unwrap();
        }
        let status: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/status.json")).unwrap();
        assert_eq!(channels(&status).unwrap(), vec!["beta", "release"]);
        assert!(
            latest_versions(&status)
                .unwrap()
                .contains_key("TradeSkillMaster")
        );
        let targets = sync_targets(&status, &wow::scan(root.path()).unwrap()).unwrap();
        assert_eq!(targets.len(), 7);
        assert_eq!(targets.iter().filter(|t| t.removable).count(), 2);
        assert!(
            targets
                .iter()
                .any(|t| t.game == Game::Anniversary && t.name == "Test Realm-Alliance")
        );
        let list: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/realms.json")).unwrap();
        assert_eq!(realm_options(&list).unwrap().len(), 2);
        let retail =
            realm_options(&json!({"retail":[{"id":42,"name":"Realm","region":"EU"}],"bcc":[]}))
                .unwrap();
        assert_eq!(
            (retail[0].game, retail[0].id.as_str()),
            (Game::Retail, "42")
        );
        let info: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/cloud.json")).unwrap();
        let remote = remote_backups(&info).unwrap();
        assert_eq!(
            remote[0].remote_name(),
            "abc_ACCOUNT-Anniversary_1700000000.zip"
        );
        let mut invalid = info.clone();
        invalid["data"]["abc_ACCOUNT-Anniversary"][0]["timestamp"] = json!(-1);
        assert!(remote_backups(&invalid).is_err());
        assert!(remote_backups(&json!({"data":{"abc_../ACCOUNT":[]}})).is_err());
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
    fn subscriptions_are_scoped_by_game_region_and_faction_before_any_file_sync() {
        let catalog = realms();
        let status = json!({"realms":[{"name":"Pyrewood Village","region":"US"}],
            "realms-Progression":[{"name":"Pyrewood Village-Alliance","region":"EU"}]});
        let registered = registered_realms(&catalog, &status).unwrap();
        assert_eq!(registered, vec![catalog[4].clone(), catalog[0].clone()]);
        let snapshot = Snapshot {
            registered_realms: registered,
            ..Default::default()
        };
        assert!(snapshot.realm_registered(&catalog[0]));
        assert!(!snapshot.realm_registered(&catalog[1]));
        assert!(!snapshot.realm_registered(&catalog[2]));
        assert!(!snapshot.realm_registered(&catalog[3]));
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
            legacy_http: true,
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
        assert!(reloaded.legacy_http);
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
