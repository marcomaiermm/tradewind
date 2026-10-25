use super::*;
use iced::widget::column;

/// Below this window width the top bar puts its two tab groups on separate lines.
pub(super) const STACKED_BAR: f32 = 1040.0;
impl App {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let l = self.language();
        let tabs = |pages: &[Page]| {
            pages.iter().fold(row![].spacing(22), |tabs, &page| {
                tabs.push(tab(page.title(l), self.page == page, Message::Page(page)))
            })
        };
        let brand = text("Tradewind").size(16).font(BOLD);
        // Ctrl+PageUp/PageDown cycle the same order: six main tabs, then the three on the right.
        let (main, more) = Page::TAB_ORDER.split_at(6);
        let main = tabs(main);
        let more = tabs(more);
        // Narrow windows cannot hold both tab groups on one line.
        let bar: Element<'_, Message> = if self.width < STACKED_BAR {
            column![
                row![brand, space().width(Length::Fill), more].align_y(alignment::Vertical::Center),
                main
            ]
            .spacing(4)
            .into()
        } else {
            row![brand, main, space().width(Length::Fill), more]
                .spacing(28)
                .align_y(alignment::Vertical::Center)
                .into()
        };
        let bar = container(bar)
            .padding([10, 24])
            .width(Length::Fill)
            .style(chrome);
        let connection = if self.snapshot.logged_in {
            if self.snapshot.premium {
                l.text(Key::ConnectedPremium)
            } else {
                l.text(Key::Connected)
            }
        } else {
            l.text(Key::SignedOut)
        };
        // Launcher grammar: the one blue action, its state in a quiet line beneath.
        let state = text(format!("{connection} · {}", self.status.render(l)))
            .size(12)
            .width(Length::Fill)
            .align_x(alignment::Horizontal::Right)
            .wrapping(text::Wrapping::WordOrGlyph)
            .style(move |theme: &Theme| text::Style {
                color: Some(if self.busy || self.error {
                    theme::tokens(theme).muted_foreground
                } else {
                    theme::tokens(theme).success
                }),
            });
        let sync = column![
            action(
                if self.busy {
                    l.text(Key::Working)
                } else {
                    l.text(Key::SyncNow)
                },
                (!self.busy && self.snapshot.logged_in).then_some(Message::Sync)
            )
            .padding([11, 22])
            // Pages with their own task (export, save, sign-in) keep the blue for it.
            .style(if matches!(self.page, Page::Accounting | Page::Account) {
                outline
            } else {
                primary
            },),
            state
        ]
        .spacing(6)
        .width(320)
        .align_x(alignment::Horizontal::Right);
        let top = row![
            column![
                text(self.page.title(l)).size(26).font(BOLD),
                caption(self.page.subtitle(l))
            ]
            .spacing(4)
            .width(Length::Fill),
            sync
        ]
        .spacing(16)
        .align_y(alignment::Vertical::Center);
        let content = match self.page {
            Page::Overview => self.overview(),
            Page::Realms => self.realms(),
            Page::AuctionHouse => auction::view(
                &self.auction,
                self.language(),
                self.busy,
                &self.auction_images,
                self.auction
                    .catalog
                    .selected
                    .as_ref()
                    .and_then(|market| self.version_icon(market.game, 36.0)),
            ),
            Page::Addons => self.addons(),
            Page::Backups => self.backups(),
            Page::Accounting => self.accounting(),
            Page::Settings => self.settings_view(),
            Page::Activity => self.activity(),
            Page::Account => self.account(),
        };
        // Auction lists and details each own their scroll body beneath fixed navigation.
        let content = if self.page == Page::AuctionHouse && self.auction.catalog.selected.is_some()
        {
            content
        } else {
            // Bottom room so the last control never sits clipped against the window edge.
            scroll(container(content).padding(iced::Padding {
                bottom: 16.0,
                ..iced::Padding::ZERO
            }))
            .id(PAGE_CONTENT)
            .height(Length::Fill)
            .into()
        };
        let mut main = column![top].spacing(16);
        // A notice card only when something needs attention. Routine state lives under Sync.
        if self.error {
            main = main.push(
                container(
                    row![
                        text(l.text(Key::Notice)).size(13).font(BOLD),
                        text(self.status.render(l)).size(14)
                    ]
                    .spacing(16),
                )
                .padding([12, 16])
                .width(Length::Fill)
                .style(destructive_alert),
            );
        }
        let main = main
            .push(content)
            .padding(iced::Padding::from([20, 24]).bottom(16))
            .width(Length::Fill)
            .height(Length::Fill);
        column![bar, rule::horizontal(1), main]
            .height(Length::Fill)
            .into()
    }
    fn overview(&self) -> Element<'_, Message> {
        let l = self.language();
        let mut content = column![].spacing(24);
        if !self.snapshot.logged_in {
            content = content.push(card(
                column![
                    heading(l.text(Key::ConnectTsm)),
                    text(l.text(Key::ConnectDescription)).size(15),
                    action(
                        l.text(Key::ConnectAccount),
                        Some(Message::Page(Page::Account))
                    )
                ]
                .spacing(12),
            ));
        }
        if self.snapshot.installation.games.is_empty() {
            content = content.push(card(
                column![
                    heading(l.text(Key::ChooseWow)),
                    text(l.text(Key::ChooseWowDescription)).size(15),
                    action(
                        l.text(Key::SetupInstallation),
                        Some(Message::Page(Page::Settings))
                    )
                ]
                .spacing(12),
            ));
        } else {
            content = content
                .push(heading(l.text(Key::YourInstallation)))
                .push(caption(&self.root));
            for &game in &self.snapshot.installation.games {
                let addons: Vec<_> = self
                    .snapshot
                    .installation
                    .addons
                    .iter()
                    .filter(|a| a.game == game)
                    .collect();
                let text_status = if !game.tsm_supported() {
                    l.text(Key::TsmUnsupportedGame)
                } else if addons.iter().any(|a| a.version.is_none()) {
                    l.text(Key::MissingAddons)
                } else {
                    l.text(Key::InstalledAddons)
                };
                let accounts = self
                    .snapshot
                    .installation
                    .accounts
                    .iter()
                    .filter(|a| a.game == game)
                    .count();
                content = content
                    .push(
                        row![
                            self.version_summary(
                                game,
                                caption(
                                    msg!(
                                        InstallationSummary,
                                        status = text_status,
                                        accounts = msg!(AccountCount, count = accounts)
                                    )
                                    .render(l)
                                ),
                            ),
                            space().width(Length::Fill),
                            action(l.text(Key::ManageAddons), Some(Message::Page(Page::Addons)))
                                .style(secondary)
                        ]
                        .align_y(alignment::Vertical::Center),
                    )
                    .push(rule::horizontal(1));
            }
        }
        if let Some(last) = self.snapshot.last_sync {
            content = content.push(row![
                text(l.text(Key::LastFullSync)).size(15),
                space().width(Length::Fill),
                text(l.timestamp(last, false)).size(15)
            ]);
        }
        if !self.snapshot.sync.is_empty() {
            content = content.push(
                action(l.text(Key::ViewPrices), Some(Message::Page(Page::Realms))).style(secondary),
            );
        }
        if !self.snapshot.news.is_empty() {
            content = content
                .push(heading(l.text(Key::TsmNews)))
                .push(text(&self.snapshot.news).size(15));
        }
        content.push(caption(l.text(Key::RestartWowHint))).into()
    }
    fn realms(&self) -> Element<'_, Message> {
        let l = self.language();
        let mut content = column![
            heading(l.text(Key::AddRealm)),
            realm_picker::view(
                &self.realm_picker,
                &self.snapshot,
                self.busy,
                self.adding_realm.as_ref(),
                l
            ),
            space().height(8),
            heading(l.text(Key::YourPrices)),
        ]
        .spacing(16);
        if self.snapshot.sync.is_empty() {
            content = content.push(card(
                column![
                    heading(l.text(Key::NoPrices)),
                    text(l.text(Key::NoPricesHint)).size(15)
                ]
                .spacing(10),
            ));
        }
        for sync in &self.snapshot.sync {
            let status = sync
                .error
                .as_ref()
                .map(|error| error.render(l))
                .unwrap_or_else(|| l.text(Key::Saved).into());
            let mut line = row![
                column![
                    row![
                        text(realm_name_and_faction(sync.game, &sync.name).0).size(16),
                        identity::version_badge(sync.game),
                    ]
                    .spacing(8)
                    .align_y(alignment::Vertical::Center),
                    row![
                        caption(sync.region.clone()),
                        match realm_name_and_faction(sync.game, &sync.name).1 {
                            Some(faction) => identity::badge(
                                identity::faction_label(faction, l),
                                identity::faction_color(faction)
                            ),
                            None => space().into(),
                        },
                        caption(if sync.game.subscribable() {
                            l.text(Key::TsmAccount)
                        } else {
                            l.text(Key::AutomaticRealm)
                        })
                    ]
                    .spacing(8)
                    .align_y(alignment::Vertical::Center),
                    caption(format!("{status} · {}", l.timestamp(sync.timestamp, false)))
                ]
                .spacing(4),
                space().width(Length::Fill)
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center);
            line = line.push(
                action(
                    l.text(Key::AuctionOpen),
                    (!self.busy && sync.timestamp > 0).then(|| {
                        Message::BrowseMarket(Some(application::Market {
                            game: sync.game,
                            scope: sync.name.clone(),
                            region: Some(sync.region.clone()),
                            region_only: sync.name == sync.region,
                        }))
                    }),
                )
                .style(secondary),
            );
            if sync.removable {
                line = line.push(
                    action(
                        l.text(Key::Remove),
                        (!self.busy).then_some(Message::RequestRemove(sync.clone())),
                    )
                    .style(ghost),
                );
            }
            content = content.push(line).push(rule::horizontal(1));
        }
        if let Some(remove) = &self.remove {
            content = content.push(card(
                column![
                    text(msg!(RemoveRealmQuestion, name = &remove.name).render(l)),
                    row![
                        action(
                            l.text(Key::RemoveRealm),
                            (!self.busy).then_some(Message::ConfirmRemove)
                        )
                        .style(destructive),
                        action(l.text(Key::Cancel), Some(Message::CancelConfirm)).style(secondary)
                    ]
                    .spacing(12)
                ]
                .spacing(12),
            ));
        }
        content = content.push(caption(l.text(Key::ManagedRealmsHint)));
        content.into()
    }
    fn addons(&self) -> Element<'_, Message> {
        let l = self.language();
        let mut content = column![].spacing(16);
        if self.snapshot.installation.games.is_empty() {
            return card(text(l.text(Key::ChooseWowFirst))).into();
        }
        for &game in &self.snapshot.installation.games {
            content = content.push(
                row![self.version_mark(game, 32.0), heading(game.label())]
                    .spacing(10)
                    .align_y(alignment::Vertical::Center),
            );
            if !game.tsm_supported() {
                content = content.push(caption(l.text(Key::TsmUnsupportedGame)));
            }
            for addon in self
                .snapshot
                .installation
                .addons
                .iter()
                .filter(|a| a.game == game)
            {
                let latest = self
                    .snapshot
                    .latest
                    .get(&addon.name)
                    .map_or(l.text(Key::AvailableAfterLogin), String::as_str);
                let label = if addon.version.is_none() {
                    l.text(Key::Install)
                } else {
                    l.text(Key::Reinstall)
                };
                let info = if addon.development {
                    l.text(Key::DevelopmentAddon).into()
                } else {
                    let version = addon
                        .version
                        .as_deref()
                        .map(|v| {
                            if v.is_empty() {
                                l.text(Key::Unknown)
                            } else {
                                v
                            }
                        })
                        .unwrap_or(l.text(Key::NotInstalled));
                    msg!(AddonVersions, installed = version, available = latest).render(l)
                };
                content = content
                    .push(
                        row![
                            column![text(&addon.name).size(16), caption(info)].spacing(5),
                            space().width(Length::Fill),
                            action(
                                label,
                                (!self.busy && self.snapshot.logged_in)
                                    .then_some(Message::Install(game, addon.name.clone()))
                            )
                            .style(secondary)
                        ]
                        .spacing(12)
                        .align_y(alignment::Vertical::Center),
                    )
                    .push(rule::horizontal(1));
            }
        }
        content.push(caption(l.text(Key::AddonBackupHint))).into()
    }
    fn backups(&self) -> Element<'_, Message> {
        let l = self.language();
        let mut content = column![
            row![
                heading(l.text(Key::SavedAddonSettings)),
                space().width(Length::Fill),
                action(
                    l.text(Key::CreateBackup),
                    (!self.busy && !self.snapshot.installation.accounts.is_empty())
                        .then_some(Message::Backup)
                )
                .style(outline)
            ]
            .align_y(alignment::Vertical::Center),
            caption(if self.snapshot.premium {
                l.text(Key::CloudSyncHint)
            } else {
                l.text(Key::LocalBackupHint)
            })
        ]
        .spacing(16);
        if let Some(backup) = &self.restore {
            content = content.push(card(
                column![
                    heading(l.text(Key::RestoreBackup)),
                    text(format!(
                        "{} · {}",
                        backup.account,
                        l.timestamp(backup.timestamp, true)
                    )),
                    text(l.text(Key::RestoreWarning)).size(14),
                    row![
                        action(
                            l.text(Key::RestoreNow),
                            (!self.busy).then_some(Message::ConfirmRestore)
                        )
                        .style(destructive),
                        action(l.text(Key::Cancel), Some(Message::CancelConfirm)).style(secondary)
                    ]
                    .spacing(12)
                ]
                .spacing(12),
            ));
        }
        if self.snapshot.backups.is_empty() {
            content = content.push(card(
                column![
                    heading(l.text(Key::NoBackups)),
                    text(l.text(Key::NoBackupsHint))
                ]
                .spacing(10),
            ));
        }
        for backup in &self.snapshot.backups {
            let place = match (backup.local.is_some(), backup.remote) {
                (true, true) => l.text(Key::LocalAndCloud),
                (true, false) => l.text(Key::Local),
                _ => l.text(Key::Cloud),
            };
            let available = self.snapshot.can_restore(backup);
            // A disabled Restore says why. `can_restore` fails on the account or on cloud sign-in.
            let reason = (!available).then(|| {
                caption(l.text(if self.snapshot.knows_account(&backup.account) {
                    Key::RestoreNeedsSignIn
                } else {
                    Key::RestoreUnknownAccount
                }))
            });
            content = content
                .push(
                    row![
                        column![
                            text(&backup.account).size(16),
                            caption(format!(
                                "{} · {place}{}",
                                l.timestamp(backup.timestamp, true),
                                if backup.keep {
                                    format!(" · {}", l.text(Key::Keep))
                                } else {
                                    String::new()
                                }
                            )),
                        ]
                        .push(reason)
                        .spacing(5),
                        space().width(Length::Fill),
                        action(
                            l.text(Key::Restore),
                            (!self.busy && available)
                                .then_some(Message::RequestRestore(backup.clone()))
                        )
                        .style(secondary)
                    ]
                    .spacing(12)
                    .align_y(alignment::Vertical::Center),
                )
                .push(rule::horizontal(1));
        }
        content.into()
    }
    fn accounting(&self) -> Element<'_, Message> {
        let l = self.language();
        let accounts: Vec<_> = self
            .snapshot
            .installation
            .accounts
            .iter()
            .map(|a| a.protocol_name())
            .collect();
        let realms = self
            .selected_account
            .as_ref()
            .and_then(|account| self.snapshot.accounting_realms.get(account))
            .cloned()
            .unwrap_or_default();
        let kinds: Vec<_> = ExportKind::ALL
            .into_iter()
            .map(|kind| {
                Choice::new(
                    kind,
                    l.text(match kind {
                        ExportKind::Sales => Key::Sales,
                        ExportKind::Purchases => Key::Purchases,
                        ExportKind::Income => Key::Income,
                        ExportKind::Expenses => Key::Expenses,
                        ExportKind::Expired => Key::Expired,
                        ExportKind::Canceled => Key::Canceled,
                    }),
                )
            })
            .collect();
        let selected_kind = kinds
            .iter()
            .find(|choice| Some(choice.value) == self.export_kind)
            .cloned();
        let content = column![
            heading(l.text(Key::ExportTransactions)),
            text(l.text(Key::AccountingLocalHint)).size(15),
            space().height(8),
            text(l.text(Key::WowAccount)).size(14),
            select(accounts, self.selected_account.clone(), Message::Account)
                .placeholder(l.text(Key::SelectAccount))
                .width(Length::Fill),
            text(l.text(Key::Realm)).size(14),
            select(realms, self.export_realm.clone(), Message::ExportRealm)
                .placeholder(l.text(Key::SelectRealm))
                .width(Length::Fill),
            text(l.text(Key::TransactionType)).size(14),
            select(kinds, selected_kind, |choice| Message::ExportKind(
                choice.value
            ))
            .width(Length::Fill),
            space().height(8),
            action(
                l.text(Key::ExportCsv),
                (!self.busy && self.export_realm.is_some()).then_some(Message::Export)
            ),
            caption(l.text(Key::ExportHint))
        ]
        .spacing(12);
        card(content).into()
    }
    fn settings_view(&self) -> Element<'_, Message> {
        let l = self.language();
        let language_choices = segmented(
            row([
                LanguagePreference::System,
                LanguagePreference::English,
                LanguagePreference::German,
            ]
            .into_iter()
            .map(|value| {
                button(text(value.label(l)).size(14))
                    .padding([6, 12])
                    .on_press_maybe((!self.busy).then_some(Message::Language(value)))
                    .style(segment(self.settings.language == value))
                    .into()
            }))
            .spacing(2),
        );
        let appearances: Vec<_> = [
            (Appearance::System, Key::System),
            (Appearance::Dark, Key::Dark),
            (Appearance::Light, Key::Light),
        ]
        .into_iter()
        .map(|(value, key)| Choice::new(value, l.text(key)))
        .collect();
        let selected_appearance = appearances
            .iter()
            .find(|choice| choice.value == self.preferences.theme)
            .unwrap()
            .clone();
        let toggle = |label: &'static str, flag: Flag, value: bool| {
            check(value)
                .label(label)
                .on_toggle(move |v| Message::Toggle(flag, v))
        };
        let channels = if self.snapshot.channels.is_empty() {
            vec!["release".into()]
        } else {
            self.snapshot.channels.clone()
        };
        column![
            heading(l.text(Key::Language)),
            language_choices,
            caption(l.text(Key::LanguageHint)),
            space().height(8),
            heading(l.text(Key::WowInstallation)),
            text(l.text(Key::InstallationFolder)).size(14),
            row![
                input(l.text(Key::WowFolder), &self.root)
                    .on_input(Message::Root)
                    .on_submit(Message::RootSubmit),
                action(
                    l.text(Key::Browse),
                    (!self.busy).then_some(Message::Directory)
                )
                .style(secondary)
            ]
            .spacing(12),
            caption(l.text(Key::WowRootHint)),
            space().height(8),
            heading(l.text(Key::Automation)),
            toggle(
                l.text(Key::AutoSync),
                Flag::AutoSync,
                self.preferences.auto_sync
            ),
            toggle(
                l.text(Key::AutoAddons),
                Flag::AutoAddons,
                self.preferences.auto_addons
            ),
            row![
                column![
                    text(l.text(Key::BackupInterval)).size(14),
                    input("1–168", &self.hours).on_input(Message::BackupHours)
                ]
                .spacing(6),
                column![
                    text(l.text(Key::BackupRetention)).size(14),
                    input("1–3650", &self.days).on_input(Message::RetentionDays)
                ]
                .spacing(6)
            ]
            .spacing(20),
            caption(l.text(Key::BackupPolicy)),
            space().height(8),
            heading(l.text(Key::Desktop)),
            toggle(
                l.text(Key::Notifications),
                Flag::Notifications,
                self.preferences.notifications
            ),
            row![
                column![
                    text(l.text(Key::Appearance)).size(14),
                    select(appearances, Some(selected_appearance), |choice| {
                        Message::Theme(choice.value)
                    })
                ]
                .spacing(6),
                column![
                    text(l.text(Key::AddonChannel)).size(14),
                    select(
                        channels,
                        Some(self.preferences.channel.clone()),
                        Message::Channel
                    )
                ]
                .spacing(6)
            ]
            .spacing(24),
            space().height(8),
            self.window_settings(),
            space().height(8),
            heading(l.text(Key::DataSharing)),
            toggle(
                l.text(Key::ShareAddonData),
                Flag::Share,
                self.preferences.share_data
            ),
            caption(l.text(Key::ShareDataHint)),
            space().height(8),
            caption(msg!(AppVersion, version = crate::APP_VERSION).render(l)),
            space().height(8),
            heading(l.text(Key::Shortcuts)),
            caption(l.text(Key::ShortcutsList)),
        ]
        .spacing(14)
        .into()
    }

    fn window_settings(&self) -> Element<'_, Message> {
        let l = self.language();
        let toggle = |flag, label, value| {
            check(value)
                .label(l.text(label))
                .on_toggle_maybe((!self.busy).then_some(move |value| Message::Toggle(flag, value)))
        };
        let mut content = column![
            heading(l.text(Key::WindowAndTray)),
            toggle(
                Flag::CloseToTray,
                Key::CloseToTray,
                self.preferences.window.close_to_tray
            ),
            check(self.preferences.window.minimize_to_tray)
                .label(l.text(Key::MinimizeToTraySetting))
                .on_toggle_maybe(
                    (!self.busy && self.minimize_supported == Some(true))
                        .then_some(|value| Message::Toggle(Flag::MinimizeToTray, value))
                ),
        ]
        .spacing(12);
        if self.minimize_supported != Some(true) {
            content = content.push(caption(l.text(if self.minimize_supported.is_none() {
                Key::MinimizeTrayChecking
            } else {
                Key::MinimizeTrayUnavailable
            })));
        }
        content
            .push(toggle(
                Flag::StartInTray,
                Key::StartInTray,
                self.preferences.window.start_in_tray,
            ))
            .push(caption(l.text(Key::StartInTrayHint)))
            .push(toggle(
                Flag::Autostart,
                Key::Autostart,
                self.preferences.autostart,
            ))
            .push(caption(l.text(Key::TrayHint)))
            .into()
    }
    fn account(&self) -> Element<'_, Message> {
        let l = self.language();
        if self.snapshot.logged_in {
            return card(
                column![
                    heading(l.text(Key::AccountConnected)),
                    text(&self.settings.email),
                    text(if self.snapshot.premium {
                        l.text(Key::PremiumDescription)
                    } else {
                        l.text(Key::FreeDescription)
                    }),
                    caption(if self.legacy_http {
                        l.text(Key::LegacyActive)
                    } else {
                        l.text(Key::HttpsConnection)
                    }),
                    action(
                        l.text(Key::SignOut),
                        (!self.busy).then_some(Message::Logout)
                    )
                    .style(secondary)
                ]
                .spacing(16),
            )
            .into();
        }
        let missing = if self.email.is_empty() || self.password.is_empty() {
            Some(Key::SignInNeedsCredentials)
        } else if !self.terms {
            Some(Key::SignInNeedsTerms)
        } else {
            None
        };
        let login = column![
            heading(l.text(Key::SignInTsm)),
            text(l.text(Key::CredentialsHint)).size(15),
            space().height(6),
            text(l.text(Key::Email)).size(14),
            input("name@example.com", &self.email).on_input(Message::Email),
            text(l.text(Key::Password)).size(14),
            input(l.text(Key::Password), &self.password)
                .secure(true)
                .on_input(Message::Password)
                .on_submit(Message::Login),
            check(self.legacy_http)
                .label(l.text(Key::LegacyOption))
                .on_toggle_maybe((!self.busy).then_some(Message::LegacyHttp)),
            caption(l.text(Key::LegacyWarning)),
            check(self.remember)
                .label(l.text(Key::RememberLogin))
                .on_toggle(Message::Remember),
            caption(l.text(Key::PasswordNotStored)),
            check(self.terms)
                .label(l.text(Key::AcceptTerms))
                .on_toggle(Message::Terms),
            action(
                l.text(Key::OpenTerms),
                Some(Message::Link("https://tradeskillmaster.com/terms".into())),
            )
            .style(outline),
            action(
                if self.busy {
                    l.text(Key::SigningIn)
                } else if self.legacy_http {
                    l.text(Key::SignInLegacy)
                } else {
                    l.text(Key::SignIn)
                },
                (!self.busy && missing.is_none()).then_some(Message::Login),
            ),
            // A disabled button alone does not say what is missing.
            missing
                .filter(|_| !self.busy)
                .map(|key| caption(l.text(key))),
            action(
                l.text(Key::CreateAccount),
                Some(Message::Link(
                    "https://tradeskillmaster.com/user/register".into()
                )),
            )
            .style(outline),
        ]
        .spacing(12);
        container(card(login)).max_width(600).into()
    }
    fn activity(&self) -> Element<'_, Message> {
        let l = self.language();
        let mut content = column![
            row![
                heading(l.text(Key::ActivityLog)),
                space().width(Length::Fill),
                action(
                    l.text(Key::SendToTsm),
                    (!self.busy && self.snapshot.logged_in).then_some(Message::UploadLog)
                )
                .style(secondary)
            ]
            .align_y(alignment::Vertical::Center),
            caption(l.text(Key::LogPrivacy))
        ]
        .spacing(16);
        if self.snapshot.log.is_empty() {
            content = content.push(text(l.text(Key::NoActivity)));
        }
        let destructive = self.theme().map_or(iced::Color::BLACK, |theme| {
            theme::tokens(&theme).destructive_text
        });
        for entry in &self.snapshot.log {
            let message = text(entry.message.render(l)).size(14);
            let message = if entry.error {
                message.style(|theme: &Theme| text::Style {
                    color: Some(theme::tokens(theme).destructive_text),
                })
            } else {
                message
            };
            let mut line = row![caption(entry.time.clone()).size(14)].spacing(12);
            if entry.error {
                line = line.push(container(dot(destructive)).padding(iced::padding::top(5)));
            }
            let mut item = column![line.push(message)].spacing(4);
            if entry.error && entry.message.is_network() {
                item = item.push(caption(l.text(Key::NetworkHint)));
            }
            content = content.push(item).push(rule::horizontal(1));
        }
        content.into()
    }
}

impl App {
    /// The launcher's version icon, when the Battle.net cache provided one.
    pub(super) fn version_icon<'a>(&self, game: Game, size: f32) -> Option<Element<'a, Message>> {
        let icon = self.version_icons.get(&game)?;
        Some(
            iced::widget::svg(icon.clone())
                .width(size)
                .height(size)
                .into(),
        )
    }
    /// Launcher list row: the icon spans name and summary. Without it a dot marks the name.
    fn version_summary<'a>(
        &self,
        game: Game,
        summary: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let name = text(game.label()).size(18);
        match self.version_icon(game, 44.0) {
            Some(icon) => row![icon, column![name, summary.into()].spacing(5)]
                .spacing(12)
                .align_y(alignment::Vertical::Center)
                .into(),
            None => column![
                row![self.version_mark(game, 0.0), name]
                    .spacing(8)
                    .align_y(alignment::Vertical::Center),
                summary.into()
            ]
            .spacing(5)
            .into(),
        }
    }
    /// Falls back to the version colour as a dot, and the version name beside it carries the meaning.
    pub(super) fn version_mark<'a>(&self, game: Game, size: f32) -> Element<'a, Message> {
        self.version_icon(game, size)
            .unwrap_or_else(|| dot(identity::game_color(game)))
    }
}
