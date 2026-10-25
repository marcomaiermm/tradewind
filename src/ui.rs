//! Native Iced presentation for installation, sync and auction workflows.
mod auction;
#[cfg(test)]
mod captures;
mod components;
mod desktop;
mod identity;
mod realm_picker;
#[cfg(test)]
mod test_support;
mod theme;
mod tooltip;
mod views;

use crate::application::{
    self, Action, Appearance, Backup, Coordinator, DesktopEvent, ExportKind, Faction, Game,
    Outcome, Preferences, Realm, RealmFilters, Settings, Snapshot, SyncRow, SystemAppearance,
    TrayHandle, realm_name_and_faction,
};
use crate::i18n::{Key, Language, LanguagePreference, Text as LocalizedText};
use components::*;
use iced::widget::{button, container, row, rule, space, text};
use iced::{Element, Length, Subscription, Task, Theme, alignment, window};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tooltip::{Position, tooltip};

/// The scrollable that holds every page. Shortcuts snap it back to the top.
const PAGE_CONTENT: &str = "page-content";

/// Command-line switches for the window, parsed once at startup.
#[derive(Clone, Debug, Default)]
pub struct Args {
    /// Save a capture of the first settled frame here, then quit.
    pub screenshot: Option<PathBuf>,
    pub show_settings: bool,
    /// Open a narrow window (layout checks).
    pub compact: bool,
}
impl Args {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screenshot" => parsed.screenshot = args.next().map(PathBuf::from),
                "--show-settings" => parsed.show_settings = true,
                "--compact" => parsed.compact = true,
                _ => (),
            }
        }
        parsed
    }
}

/// Settings switches shown as checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flag {
    AutoSync,
    AutoAddons,
    Notifications,
    Share,
    Autostart,
    CloseToTray,
    MinimizeToTray,
    StartInTray,
}
impl Flag {
    fn field(self, preferences: &mut Preferences) -> &mut bool {
        match self {
            Self::AutoSync => &mut preferences.auto_sync,
            Self::AutoAddons => &mut preferences.auto_addons,
            Self::Notifications => &mut preferences.notifications,
            Self::Share => &mut preferences.share_data,
            Self::Autostart => &mut preferences.autostart,
            Self::CloseToTray => &mut preferences.window.close_to_tray,
            Self::MinimizeToTray => &mut preferences.window.minimize_to_tray,
            Self::StartInTray => &mut preferences.window.start_in_tray,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Realms,
    AuctionHouse,
    Addons,
    Backups,
    Accounting,
    Settings,
    Activity,
    Account,
}
impl Page {
    const TAB_ORDER: [Self; 9] = [
        Self::Overview,
        Self::Realms,
        Self::AuctionHouse,
        Self::Addons,
        Self::Backups,
        Self::Accounting,
        Self::Activity,
        Self::Settings,
        Self::Account,
    ];
    fn title(self, l: Language) -> &'static str {
        match self {
            Self::Overview => l.text(Key::Overview),
            Self::Realms => l.text(Key::Realms),
            Self::AuctionHouse => l.text(Key::AuctionHouse),
            Self::Addons => l.text(Key::Addons),
            Self::Backups => l.text(Key::Backups),
            Self::Accounting => l.text(Key::Accounting),
            Self::Settings => l.text(Key::Settings),
            Self::Activity => l.text(Key::Activity),
            Self::Account => l.text(Key::TsmAccount),
        }
    }
    fn subtitle(self, l: Language) -> &'static str {
        match self {
            Self::Overview => l.text(Key::OverviewSubtitle),
            Self::Realms => l.text(Key::RealmsSubtitle),
            Self::AuctionHouse => l.text(Key::AuctionSubtitle),
            Self::Addons => l.text(Key::AddonsSubtitle),
            Self::Backups => l.text(Key::BackupsSubtitle),
            Self::Accounting => l.text(Key::AccountingSubtitle),
            Self::Settings => l.text(Key::SettingsSubtitle),
            Self::Activity => l.text(Key::ActivitySubtitle),
            Self::Account => l.text(Key::AccountSubtitle),
        }
    }
}
#[derive(Clone)]
enum Message {
    SystemAppearance(SystemAppearance),
    Resized(f32),
    VersionIcons(std::collections::BTreeMap<Game, Vec<u8>>),
    Focus(bool),
    CyclePage(bool),
    SearchShortcut,
    Page(Page),
    Tick,
    TimerFinished(Option<Outcome>),
    Finished(Outcome),
    Directory,
    RootSubmit,
    Sync,
    Backup,
    Install(Game, String),
    RequestRestore(Backup),
    ConfirmRestore,
    CancelConfirm,
    RequestRemove(SyncRow),
    ConfirmRemove,
    RealmSearch(String),
    Auction(application::auction::Browse),
    AuctionIcons(application::icons::Loaded),
    BrowseMarket(Option<application::Market>),
    ToggleAuctionFavorite(application::Favorite),
    RealmGame(Option<Game>),
    RealmRegion(Option<String>),
    RealmFaction(Option<Faction>),
    ResetRealmFilters,
    RealmFiltersSaved(Result<(), LocalizedText>),
    AddRealm(Realm),
    Email(String),
    Password(String),
    Remember(bool),
    LegacyHttp(bool),
    Terms(bool),
    Login,
    Logout,
    Root(String),
    Toggle(Flag, bool),
    Theme(Appearance),
    Language(LanguagePreference),
    TrayLanguageUpdated,
    Channel(String),
    BackupHours(String),
    RetentionDays(String),
    Account(String),
    ExportRealm(String),
    ExportKind(ExportKind),
    Export,
    Link(String),
    LinkFinished(Result<(), LocalizedText>),
    UploadLog,
    DesktopReady(tokio::sync::mpsc::UnboundedSender<DesktopEvent>),
    Desktop(DesktopEvent),
    Show,
    WindowCloseRequested(window::Id),
    WindowClosed(window::Id),
    WindowMinimized(window::Id, u64, Option<bool>),
    Tray(Result<Arc<TrayHandle>, LocalizedText>),
    Quit,
    ShutdownReady,
    Capture(window::Screenshot),
    CaptureSaved(Result<(), LocalizedText>),
    CaptureRequest,
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password(_) => f.write_str("Password(<redacted>)"),
            Self::Finished(_) => f.write_str("Finished"),
            _ => f.write_str("UI action"),
        }
    }
}
struct App {
    coordinator: Option<Coordinator>,
    /// The last saved settings. Only these drive behaviour (window, tray, sync).
    settings: Settings,
    /// Revision of `settings`, so older outcomes cannot overwrite newer saves.
    revision: u64,
    /// The settings page's draft, saved on every change.
    preferences: Preferences,
    /// Sign-in draft, persisted with the next login.
    terms: bool,
    /// A settings change arrived while busy. It is saved once the running action ends.
    pending_save: Option<bool>,
    snapshot: Snapshot,
    page: Page,
    busy: bool,
    quitting: bool,
    status: LocalizedText,
    error: bool,
    email: String,
    password: String,
    remember: bool,
    legacy_http: bool,
    root: String,
    hours: String,
    days: String,
    realm_picker: realm_picker::State,
    adding_realm: Option<Realm>,
    auction: application::auction::Browser,
    auction_images: auction::Images,
    /// Launcher icons per WoW folder. Versions without one keep their colour dot.
    version_icons: std::collections::BTreeMap<Game, iced::widget::svg::Handle>,
    selected_account: Option<String>,
    export_realm: Option<String>,
    export_kind: Option<ExportKind>,
    restore: Option<Backup>,
    remove: Option<SyncRow>,
    tray: Option<Arc<TrayHandle>>,
    tray_starting: bool,
    window: Option<window::Id>,
    window_revision: u64,
    minimize_supported: Option<bool>,
    compact: bool,
    screenshot: Option<PathBuf>,
    capture_pending: bool,
    started: Instant,
    system: SystemAppearance,
    /// Window width. The top bar stacks below `views::STACKED_BAR`.
    width: f32,
}
/// Tray events. The channel is handed to the tray through `DesktopReady`, and the stream ends
/// when the tray drops its sender.
fn desktop_events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(8, async |mut output| {
        use iced::futures::SinkExt;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        if output.send(Message::DesktopReady(sender)).await.is_err() {
            return;
        }
        while let Some(event) = events.recv().await {
            if output.send(Message::Desktop(event)).await.is_err() {
                return;
            }
        }
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice<T> {
    value: T,
    label: String,
}
impl<T> Choice<T> {
    fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}
impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
impl App {
    fn language(&self) -> Language {
        self.settings
            .language
            .resolve(application::system_language())
    }
    fn update_tray_language(&self) -> Task<Message> {
        match self.tray.clone() {
            Some(tray) => {
                let language = self.language();
                Task::perform(async move { tray.set_language(language).await }, |_| {
                    Message::TrayLanguageUpdated
                })
            }
            None => Task::none(),
        }
    }

    fn new(args: Args) -> (Self, Task<Message>) {
        let init = Coordinator::new();
        let (coordinator, settings, error) = match init {
            Ok((c, s)) => (Some(c), s, None),
            Err(e) => (
                None,
                Settings::default(),
                Some(LocalizedText::from_error(&e)),
            ),
        };
        let page = if args.show_settings {
            Page::Settings
        } else {
            Page::Overview
        };
        let settings_root = settings.wow_path.clone();
        let mut app = Self {
            coordinator,
            root: settings.wow_path.to_string_lossy().into_owned(),
            hours: settings.backup_hours.to_string(),
            days: settings.retention_days.to_string(),
            email: settings.email.clone(),
            legacy_http: settings.legacy_http,
            realm_picker: realm_picker::State::new(settings.realm_filters.clone()),
            preferences: settings.preferences(),
            terms: settings.terms_accepted,
            settings,
            revision: 0,
            snapshot: Snapshot::default(),
            page,
            busy: error.is_none(),
            quitting: false,
            pending_save: None,
            status: error.clone().unwrap_or_else(|| msg!(ReadingInstallation)),
            error: error.is_some(),
            password: String::new(),
            remember: false,
            adding_realm: None,
            auction: application::auction::Browser::default(),
            auction_images: auction::Images::default(),
            version_icons: Default::default(),
            selected_account: None,
            export_realm: None,
            export_kind: Some(ExportKind::Sales),
            restore: None,
            remove: None,
            tray: None,
            tray_starting: true,
            window: None,
            window_revision: 0,
            minimize_supported: None,
            compact: args.compact,
            screenshot: args.screenshot,
            capture_pending: false,
            started: Instant::now(),
            system: SystemAppearance::default(),
            width: 1120.0,
        };
        let inspect = app.command(Action::Bootstrap);
        let open = if app.settings.window.start_in_tray
            && !app.error
            && app.screenshot.is_none()
            && page != Page::Settings
        {
            Task::none()
        } else {
            app.show_window()
        };
        (
            app,
            Task::batch([
                inspect,
                open,
                Task::perform(
                    application::version_icons(settings_root),
                    Message::VersionIcons,
                ),
            ]),
        )
    }
    fn command(&self, action: Action) -> Task<Message> {
        if let Some(coordinator) = self.coordinator.clone() {
            Task::perform(
                async move { coordinator.dispatch(action).await },
                Message::Finished,
            )
        } else {
            Task::none()
        }
    }
    fn start(&mut self, action: Action, label: LocalizedText) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        self.status = label;
        self.error = false;
        self.command(action)
    }
    /// Saves the settings draft. Folder and periods are only taken over when valid.
    fn persist(&mut self, with_root: bool) -> Task<Message> {
        if self.busy {
            self.pending_save = Some(with_root || self.pending_save.unwrap_or(false));
            return Task::none();
        }
        if with_root {
            self.preferences.wow_path = self.root.clone().into();
        }
        if let (Ok(hours), Ok(days)) = (self.hours.parse(), self.days.parse()) {
            self.preferences.backup_hours = hours;
            self.preferences.retention_days = days;
        }
        self.start(Action::Save(self.preferences.clone()), msg!(SavingSettings))
    }
    /// Takes over saved settings unless a newer save already arrived. Drafts follow them
    /// only when no save is queued.
    fn accept_settings(&mut self, settings: Settings, revision: u64) {
        if revision < self.revision {
            return;
        }
        self.settings = settings;
        self.revision = revision;
        if self.pending_save.is_none() {
            self.preferences = self.settings.preferences();
        }
    }
    fn persist_periods(&mut self) -> Task<Message> {
        if self.hours.parse::<u64>().is_err() || self.days.parse::<u64>().is_err() {
            self.error = true;
            self.status = msg!(WholePeriods);
            return Task::none();
        }
        self.persist(false)
    }
    fn theme(&self) -> Option<Theme> {
        let dark = match self.preferences.theme {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => self.system.dark,
        };
        Some(if dark { theme::dark() } else { theme::light() })
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            // Iced's own detection follows winit's window theme, which is empty on Wayland
            // and otherwise echoes the theme this app set.
            Subscription::run(application::appearance).map(Message::SystemAppearance),
            Subscription::run(desktop_events),
            window::close_requests().map(Message::WindowCloseRequested),
            window::close_events().map(Message::WindowClosed),
            window::resize_events().map(|(_, size)| Message::Resized(size.width)),
            iced::event::listen_with(|event, status, _| {
                if status != iced::event::Status::Ignored {
                    return None;
                }
                let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) = event
                else {
                    return None;
                };
                match key {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) => {
                        Some(Message::Focus(modifiers.shift()))
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
                        Some(Message::CancelConfirm)
                    }
                    iced::keyboard::Key::Named(
                        named @ (iced::keyboard::key::Named::ArrowDown
                        | iced::keyboard::key::Named::ArrowUp),
                    ) => Some(Message::Auction(application::auction::Browse::Step(
                        named == iced::keyboard::key::Named::ArrowDown,
                    ))),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::F5) => {
                        Some(Message::Sync)
                    }
                    iced::keyboard::Key::Character(c)
                        if modifiers.control() && c.eq_ignore_ascii_case("f") =>
                    {
                        Some(Message::SearchShortcut)
                    }
                    // Ctrl+PageUp/PageDown cycle the top-bar tabs. They produce no text.
                    iced::keyboard::Key::Named(
                        named @ (iced::keyboard::key::Named::PageDown
                        | iced::keyboard::key::Named::PageUp),
                    ) if modifiers.control() => Some(Message::CyclePage(
                        named == iced::keyboard::key::Named::PageDown,
                    )),
                    _ => None,
                }
            }),
        ])
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        // Let the coordinator finish existing writes before accepting any more work.
        if self.quitting && !matches!(message, Message::ShutdownReady) {
            return Task::none();
        }
        match message {
            Message::SystemAppearance(system) => {
                self.system = system;
                tooltip::set_reduced_motion(system.reduced_motion);
            }
            Message::Resized(width) => self.width = width,
            Message::VersionIcons(icons) => {
                self.version_icons = icons
                    .into_iter()
                    .map(|(game, svg)| (game, iced::widget::svg::Handle::from_memory(svg)))
                    .collect();
            }
            Message::Focus(backwards) => {
                return if backwards {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::CyclePage(forward) => {
                let count = Page::TAB_ORDER.len();
                let current = Page::TAB_ORDER
                    .iter()
                    .position(|&page| page == self.page)
                    .unwrap_or(0);
                let next = if forward {
                    (current + 1) % count
                } else {
                    (current + count - 1) % count
                };
                return self.update(Message::Page(Page::TAB_ORDER[next]));
            }
            Message::SearchShortcut => {
                if self.page == Page::AuctionHouse {
                    self.auction.dispatch(application::auction::Browse::Back);
                    return Task::batch([
                        iced::widget::operation::snap_to(
                            PAGE_CONTENT,
                            iced::widget::operation::RelativeOffset::START,
                        ),
                        iced::widget::operation::focus(auction::SEARCH_ID),
                    ]);
                }
                if self.page == Page::Realms {
                    return Task::batch([
                        iced::widget::operation::snap_to(
                            PAGE_CONTENT,
                            iced::widget::operation::RelativeOffset::START,
                        ),
                        iced::widget::operation::focus(realm_picker::SEARCH_ID),
                    ]);
                }
            }
            Message::Page(page) => {
                self.page = page;
                self.restore = None;
                self.remove = None;
                if page == Page::AuctionHouse {
                    // Navigation returns to results without reloading and reopening the item.
                    let back = self.update(Message::Auction(application::auction::Browse::Back));
                    if self.auction.catalog.selected.is_some() || self.busy {
                        return back;
                    }
                    return Task::batch([back, self.update(Message::BrowseMarket(None))]);
                }
            }
            Message::Finished(outcome) => {
                self.busy = false;
                self.adding_realm = None;
                self.error = outcome.result.is_err();
                self.status = outcome.result.unwrap_or_else(|e| e);
                let resumed = !self.snapshot.logged_in && outcome.snapshot.logged_in;
                let selected = outcome.snapshot.selected_directory.clone();
                let selected_directory = selected.is_some();
                self.snapshot = outcome.snapshot;
                let previous_market = self.auction.catalog.selected.clone();
                self.accept_settings(outcome.settings, outcome.revision);
                self.auction.replace(
                    self.snapshot.auction.as_ref(),
                    &self.settings.auction_favorites,
                );
                if let Some(path) = selected {
                    self.root = path.to_string_lossy().into_owned();
                }
                if self.selected_account.is_none() {
                    self.selected_account = self
                        .snapshot
                        .installation
                        .accounts
                        .first()
                        .map(|a| a.protocol_name());
                    self.select_account();
                }
                if resumed {
                    self.password.clear();
                    self.page = Page::Overview;
                }
                let scroll = if previous_market != self.auction.catalog.selected {
                    iced::widget::operation::snap_to(
                        auction::RESULTS_ID,
                        iced::widget::operation::RelativeOffset::START,
                    )
                } else {
                    Task::none()
                };
                let save = if let Some(with_root) = self.pending_save.take() {
                    self.persist(with_root)
                } else if selected_directory {
                    self.persist(true)
                } else {
                    Task::none()
                };
                return Task::batch([
                    self.update_tray_language(),
                    self.load_auction_icons(),
                    scroll,
                    save,
                ]);
            }
            Message::TimerFinished(Some(outcome)) => {
                self.error = outcome.result.is_err();
                self.status = outcome.result.unwrap_or_else(|e| e);
                self.snapshot = outcome.snapshot;
                self.accept_settings(outcome.settings, outcome.revision);
                self.auction.replace(
                    self.snapshot.auction.as_ref(),
                    &self.settings.auction_favorites,
                );
                return self.load_auction_icons();
            }
            Message::TimerFinished(None) => (),
            Message::DesktopReady(sender) => {
                let language = self.language();
                return Task::perform(
                    async move {
                        application::start_desktop(sender, language)
                            .await
                            .map(Arc::new)
                    },
                    Message::Tray,
                );
            }
            Message::Desktop(event) => match event {
                DesktopEvent::Quit => return self.update(Message::Quit),
                DesktopEvent::Show => return self.update(Message::Show),
                DesktopEvent::Sync if !self.busy && self.snapshot.logged_in => {
                    return self.update(Message::Sync);
                }
                DesktopEvent::Sync => (),
            },
            Message::Tick => {
                let mut desktop_tasks = Vec::new();
                // A terminated tray service must not leave a windowless daemon stranded.
                if self.window.is_none() && !self.tray_starting && !self.tray_available() {
                    desktop_tasks.push(self.show_window());
                }
                if self.minimize_supported == Some(true)
                    && self.settings.window.minimize_to_tray
                    && self.tray_available()
                    && let Some(id) = self.window
                {
                    desktop_tasks.push(self.check_minimized(id));
                }
                if self.screenshot.is_some()
                    && !self.capture_pending
                    && !self.busy
                    && self.started.elapsed() > Duration::from_secs(3)
                {
                    self.capture_pending = true;
                    desktop_tasks.push(self.update(Message::CaptureRequest));
                }
                if !self.busy
                    && let Some(coordinator) = self.coordinator.clone()
                {
                    desktop_tasks.push(Task::perform(
                        async move { coordinator.tick().await },
                        Message::TimerFinished,
                    ));
                }
                return Task::batch(desktop_tasks);
            }
            Message::Sync => {
                return self.start(Action::Sync, msg!(Syncing));
            }
            Message::Directory => {
                return self.start(
                    Action::ChooseDirectory(self.root.clone().into()),
                    msg!(ChoosingDirectory),
                );
            }
            // The folder is saved (and the installation re-inspected) on Enter and after
            // Browse, not per keystroke: every save scans the folder.
            Message::RootSubmit => return self.persist(true),
            Message::Login => {
                if self.busy {
                    return Task::none();
                }
                let password = std::mem::take(&mut self.password);
                return self.start(
                    Action::Login {
                        email: self.email.trim().into(),
                        password,
                        remember: self.remember,
                        terms: self.terms,
                        legacy_http: self.legacy_http,
                    },
                    msg!(SigningIn),
                );
            }
            Message::Logout => return self.start(Action::Logout, msg!(SigningOut)),
            Message::Backup => {
                return self.start(Action::Backup, msg!(BackingUp));
            }
            Message::Install(game, name) => {
                return self.start(Action::Install(game, name), msg!(Installing));
            }
            Message::RequestRestore(backup) => self.restore = Some(backup),
            Message::ConfirmRestore => {
                if let Some(backup) = self.restore.take() {
                    return self.start(Action::Restore(backup), msg!(Restoring));
                }
            }
            Message::RequestRemove(row) => self.remove = Some(row),
            Message::ConfirmRemove => {
                if let Some(row) = self.remove.take() {
                    return self.start(Action::RemoveRealm(row), msg!(RemovingRealm));
                }
            }
            Message::CancelConfirm => {
                let had_confirmation = self.restore.take().is_some() | self.remove.take().is_some();
                if !had_confirmation
                    && self.page == Page::AuctionHouse
                    && self.auction.detail().is_some()
                {
                    return self.update(Message::Auction(application::auction::Browse::Back));
                }
            }
            Message::Auction(action) => {
                use application::auction::Browse;
                if matches!(action, Browse::Step(_)) && self.page != Page::AuctionHouse {
                    return Task::none();
                }
                let page = self.auction.page;
                // Picking an item beside the list keeps the list where it is.
                let stepping = matches!(action, Browse::Step(_));
                let picking = stepping || matches!(action, Browse::Select(_));
                self.auction.dispatch(action);
                let keep_list = picking && page == self.auction.page;
                // Proportional snap keeps the stepped row roughly in view. Exact
                // row offsets would need the rendered row heights.
                let position = self
                    .auction
                    .visible()
                    .position(|item| self.auction.selected.as_deref() == Some(item.id.as_str()));
                let rows = self.auction.visible().count();
                let list = if let (true, Some(position)) = (stepping, position) {
                    iced::widget::operation::snap_to(
                        auction::RESULTS_ID,
                        iced::widget::operation::RelativeOffset {
                            x: 0.0,
                            y: position as f32 / rows.saturating_sub(1).max(1) as f32,
                        },
                    )
                } else if keep_list {
                    Task::none()
                } else {
                    iced::widget::operation::snap_to(
                        auction::RESULTS_ID,
                        iced::widget::operation::RelativeOffset::START,
                    )
                };
                return Task::batch([
                    iced::widget::operation::snap_to(
                        PAGE_CONTENT,
                        iced::widget::operation::RelativeOffset::START,
                    ),
                    list,
                    iced::widget::operation::snap_to(
                        auction::DETAIL_ID,
                        iced::widget::operation::RelativeOffset::START,
                    ),
                    self.load_auction_icons(),
                ]);
            }
            Message::AuctionIcons(loaded) => {
                if self.auction_images.state.accept(&loaded) {
                    for (id, icon) in loaded.images {
                        if let Some(icon) = icon {
                            self.auction_images.handles.insert(
                                id,
                                iced::widget::image::Handle::from_rgba(
                                    icon.width,
                                    icon.height,
                                    icon.rgba.to_vec(),
                                ),
                            );
                        }
                    }
                }
                return self.load_auction_icons();
            }
            Message::BrowseMarket(market) => {
                self.page = Page::AuctionHouse;
                return self.start(Action::BrowseMarket(market), msg!(AuctionLoading));
            }
            Message::ToggleAuctionFavorite(favorite) => {
                return self.start(
                    Action::ToggleFavorite(favorite),
                    msg!(AuctionFavoriteSaving),
                );
            }
            Message::RealmSearch(search) => {
                self.realm_picker.query = search;
            }
            Message::RealmGame(game) => {
                if game.is_some_and(|game| !game.faction_realms()) {
                    self.realm_picker.filters.faction = None;
                }
                self.realm_picker.filters.game = game;
                return self.save_realm_filters();
            }
            Message::RealmRegion(region) => {
                self.realm_picker.filters.region = region;
                return self.save_realm_filters();
            }
            Message::RealmFaction(faction) => {
                self.realm_picker.filters.faction = faction;
                return self.save_realm_filters();
            }
            Message::ResetRealmFilters => {
                self.realm_picker = realm_picker::State::new(RealmFilters::default());
                return self.save_realm_filters();
            }
            Message::RealmFiltersSaved(result) => {
                if let Err(error) = result {
                    self.error = true;
                    self.status = msg!(FiltersSaveFailed, error = error);
                }
            }
            Message::AddRealm(realm) => {
                if self.busy || !self.snapshot.logged_in {
                    return Task::none();
                }
                self.adding_realm = Some(realm.clone());
                return self.start(Action::AddRealm(realm), msg!(AddingRealm));
            }
            Message::Email(email) => self.email = email,
            Message::Password(password) => self.password = password,
            Message::Remember(remember) => self.remember = remember,
            Message::LegacyHttp(enabled) => {
                if !self.busy && !self.snapshot.logged_in {
                    self.legacy_http = enabled;
                }
            }
            Message::Terms(terms) => self.terms = terms,
            Message::Root(root) => self.root = root,
            Message::Language(language) => {
                if self.busy {
                    return Task::none();
                }
                self.settings.language = language;
                return self.start(Action::SetLanguage(language), msg!(SavingLanguage));
            }
            Message::TrayLanguageUpdated => (),
            Message::Theme(theme) => {
                self.preferences.theme = theme;
                return self.persist(false);
            }
            Message::Channel(channel) => {
                self.preferences.channel = channel;
                return self.persist(false);
            }
            Message::BackupHours(hours) => {
                self.hours = hours;
                return self.persist_periods();
            }
            Message::RetentionDays(days) => {
                self.days = days;
                return self.persist_periods();
            }
            Message::Toggle(flag, value) => {
                if flag == Flag::MinimizeToTray && self.minimize_supported != Some(true) {
                    return Task::none();
                }
                *flag.field(&mut self.preferences) = value;
                return self.persist(false);
            }
            Message::Account(account) => {
                self.selected_account = Some(account);
                self.select_account();
            }
            Message::ExportRealm(realm) => self.export_realm = Some(realm),
            Message::ExportKind(kind) => self.export_kind = Some(kind),
            Message::Export => {
                if let (Some(account), Some(realm), Some(kind)) = (
                    self.selected_account.clone(),
                    self.export_realm.clone(),
                    self.export_kind,
                ) {
                    return self.start(Action::Export(account, realm, kind), msg!(Exporting));
                }
            }
            Message::Link(url) => {
                return Task::perform(application::open_link(url), Message::LinkFinished);
            }
            Message::LinkFinished(result) => {
                if let Err(error) = result {
                    self.status = error;
                    self.error = true;
                }
            }
            Message::UploadLog => {
                return self.start(Action::UploadLog, msg!(UploadingLog));
            }
            Message::WindowClosed(id) => {
                if self.window == Some(id) {
                    self.window = None;
                }
            }
            Message::Tray(result) => {
                self.tray_starting = false;
                self.tray = result.ok();
                let open = if self.window.is_none() && !self.tray_available() {
                    self.show_window()
                } else {
                    Task::none()
                };
                return Task::batch([open, self.update_tray_language()]);
            }
            Message::WindowCloseRequested(id) => return self.close_requested(id),
            Message::WindowMinimized(id, revision, state) => {
                return self.window_minimized(id, revision, state);
            }
            Message::Show => return self.show_window(),
            Message::Quit => return self.request_quit(),
            Message::ShutdownReady => return iced::exit(),
            Message::CaptureRequest => {
                if let Some(id) = self.window {
                    return window::screenshot(id).map(Message::Capture);
                }
            }
            Message::Capture(screenshot) => {
                if let Some(path) = self.screenshot.take() {
                    return Task::perform(
                        application::save_capture(
                            path,
                            screenshot.size.width,
                            screenshot.size.height,
                            screenshot.rgba.to_vec(),
                        ),
                        Message::CaptureSaved,
                    );
                }
            }
            Message::CaptureSaved(result) => {
                if let Err(e) = result {
                    eprintln!("Screenshot: {e}");
                }
                return iced::exit();
            }
        }
        Task::none()
    }
    fn load_auction_icons(&mut self) -> Task<Message> {
        if self.page != Page::AuctionHouse {
            return Task::none();
        }
        let images = &mut self.auction_images;
        let generation = images.state.generation();
        let request = images.state.next(&self.settings.wow_path, &self.auction);
        if generation != images.state.generation() {
            images.handles.clear();
        }
        images.handles.retain(|id, _| images.state.retains(id));
        match (self.coordinator.clone(), request) {
            (Some(coordinator), Some(request)) => Task::perform(
                async move { coordinator.load_icons(request).await },
                Message::AuctionIcons,
            ),
            _ => Task::none(),
        }
    }
    fn save_realm_filters(&self) -> Task<Message> {
        if let Some(coordinator) = self.coordinator.clone() {
            let filters = self.realm_picker.filters.clone();
            Task::perform(
                async move {
                    coordinator
                        .dispatch(Action::SaveRealmFilters(filters))
                        .await
                        .result
                        .map(|_| ())
                },
                Message::RealmFiltersSaved,
            )
        } else {
            Task::none()
        }
    }
    fn select_account(&mut self) {
        self.export_realm = self
            .selected_account
            .as_ref()
            .and_then(|account| self.snapshot.accounting_realms.get(account))
            .and_then(|realms| realms.first())
            .cloned();
    }
}

pub fn run(args: Args) -> iced::Result {
    // KWin/Plasma need desktop metadata before the first Wayland window is mapped.
    if let Err(error) = application::prepare_desktop_identity() {
        eprintln!(
            "{}",
            msg!(DesktopIconFailed, error = error).render(application::system_language())
        );
    }
    components::load_fonts();
    // The process keeps syncing when its window is deliberately closed into the tray.
    iced::daemon(
        move || App::new(args.clone()),
        App::update,
        App::view_window,
    )
    .title("Tradewind")
    .theme(|app: &App, _| app.theme())
    .subscription(App::subscription)
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(
        settings: Settings,
        revision: u64,
        result: Result<LocalizedText, LocalizedText>,
    ) -> Message {
        Message::Finished(Outcome {
            snapshot: Snapshot::default(),
            settings,
            revision,
            result,
        })
    }

    #[test]
    fn invalid_periods_are_not_persisted_and_busy_changes_wait() {
        let (mut app, _) = App::new(Args::default());
        app.busy = false;
        let _ = app.update(Message::BackupHours(String::new()));
        assert!(app.error && !app.busy);
        assert!(app.hours.is_empty(), "the typed draft is kept");
        app.busy = true;
        let _ = app.update(Message::Toggle(Flag::Share, true));
        assert_eq!(app.pending_save, Some(false));
        assert!(app.preferences.share_data);
    }

    #[test]
    fn queued_drafts_survive_results_and_stale_revisions_are_ignored() {
        let (mut app, _) = App::new(Args::default());
        let saved = app.settings.clone();
        app.busy = true;
        let _ = app.update(Message::Theme(Appearance::Light));
        let _ = app.update(Message::Toggle(Flag::Share, true));
        let _ = app.update(outcome(saved.clone(), 1, Ok("Eingelesen".into())));
        assert_eq!(app.preferences.theme, Appearance::Light);
        assert!(app.preferences.share_data);
        assert!(app.busy, "the queued save started");
        assert_eq!(app.pending_save, None);

        let mut newer = saved.clone();
        newer.share_data = true;
        let _ = app.update(outcome(newer, 2, Ok(msg!(SettingsSaved))));
        assert!(app.settings.share_data && app.preferences.share_data);
        let _ = app.update(Message::TimerFinished(Some(Outcome {
            snapshot: Snapshot::default(),
            settings: saved,
            revision: 1,
            result: Ok(msg!(InstallationRead)),
        })));
        assert!(
            app.settings.share_data,
            "an older outcome cannot undo a save"
        );
    }

    #[test]
    fn language_switch_preserves_the_folder_draft() {
        let (mut app, _) = App::new(Args::default());
        app.busy = false;
        let saved = app.settings.clone();
        let _ = app.update(Message::Root("/example/unsaved".into()));
        let _ = app.update(Message::Language(LanguagePreference::English));
        let _ = app.update(outcome(
            Settings {
                language: LanguagePreference::English,
                ..saved
            },
            1,
            Ok(msg!(LanguageSaved)),
        ));
        assert_eq!(app.root, "/example/unsaved");
        assert_eq!(app.language(), Language::English);
        assert_eq!(app.status.render(app.language()), "Language saved");
    }
}
