//! Offscreen captures of every page for manual visual review (`TSM_CAPTURE_DIR`).
use super::test_support::Bounds;
use super::*;
use LanguagePreference::{English as En, German as De};
use iced::advanced::{Layout, layout, mouse, renderer, widget::Tree};
use renderer::Headless;
use theme::{dark, light};

/// Name (also selects the setup below), page, window size, theme, language.
type Scenario = (
    &'static str,
    Page,
    u32,
    u32,
    fn() -> Theme,
    LanguagePreference,
);
#[rustfmt::skip]
const SCENARIOS: &[Scenario] = &[
    ("auction-header-hover-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-header-hover-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-sort-quality-header-hover-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-detail-navigation-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-detail-navigation-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-detail-scrolled-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-hot-deals-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-hot-deals-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-hot-tooltip-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-menu-sort-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-menu-sort-scrolled-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-menu-quality-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-menu-market-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-menu-category-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-sort-quality-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-sort-quality-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-sort-price-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-table-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-table-scrolled-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-table-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-table-compact-de", Page::AuctionHouse, 820, 620, dark, De),
    ("auction-tooltip-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-tooltip-compact-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-long-tooltip-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-icons-missing-tooltip-de", Page::AuctionHouse, 820, 620, dark, De),
    ("auction-qualities-de", Page::AuctionHouse, 1120, 1060, dark, De),
    ("auction-icons-missing-en", Page::AuctionHouse, 820, 760, light, En),
    ("auction-icons-loading-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-long-de", Page::AuctionHouse, 820, 760, dark, De),
    ("auction-long-en", Page::AuctionHouse, 820, 760, light, En),
    ("auction-long-detail-de", Page::AuctionHouse, 820, 760, dark, De),
    ("auction-long-detail-en", Page::AuctionHouse, 820, 760, light, En),
    ("auction-category-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-en-compact", Page::AuctionHouse, 820, 620, light, En),
    ("auction-detail-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-detail-en-compact", Page::AuctionHouse, 820, 760, light, En),
    ("auction-watchlist-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("auction-empty-en", Page::AuctionHouse, 820, 620, light, En),
    ("auction-busy-de", Page::AuctionHouse, 1120, 760, dark, De),
    ("overview-de", Page::Overview, 1120, 760, dark, De),
    ("overview-en", Page::Overview, 820, 620, dark, En),
    ("settings-tray-de", Page::Settings, 820, 760, dark, De),
    ("settings-tray-en", Page::Settings, 1120, 760, light, En),
    ("settings-de", Page::Settings, 820, 760, dark, De),
    ("settings-en", Page::Settings, 1120, 760, light, En),
    ("account-en", Page::Account, 820, 760, dark, En),
    ("accounting-en", Page::Accounting, 1120, 760, light, En),
    ("addons-en", Page::Addons, 1120, 760, dark, En),
    ("backups-en", Page::Backups, 1120, 760, dark, En),
    ("activity-de", Page::Activity, 1120, 760, dark, De),
    ("activity-en", Page::Activity, 1120, 760, light, En),
    ("realms-de", Page::Realms, 1120, 760, dark, De),
    ("realms-compact-de", Page::Realms, 820, 620, dark, De),
    ("realms-en", Page::Realms, 1120, 760, light, En),
    ("realms-filtered-en", Page::Realms, 1120, 760, dark, En),
    ("realms-busy-de", Page::Realms, 1120, 760, dark, De),
    ("realms-empty-en", Page::Realms, 820, 620, light, En),
    ("realms-prices-de", Page::Realms, 1120, 760, dark, De),
];

#[test]
#[ignore = "manual offscreen visual verification; writes captures to TSM_CAPTURE_DIR"]
fn render_native_views_without_compositor() {
    let output = PathBuf::from(std::env::var_os("TSM_CAPTURE_DIR").expect("TSM_CAPTURE_DIR"));
    let root = tempfile::tempdir().unwrap();
    let account = root
        .path()
        .join("_retail_/WTF/Account/ACCOUNT/SavedVariables");
    std::fs::create_dir_all(&account).unwrap();
    std::fs::write(
        account.join("TradeSkillMaster.lua"),
        include_str!("../../tests/fixtures/savedvariables.lua"),
    )
    .unwrap();
    for addon in ["TradeSkillMaster", "TradeSkillMaster_AppHelper"] {
        let path = root.path().join("_retail_/Interface/AddOns").join(addon);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join(format!("{addon}.toc")), "## Version: 4.14.2\n").unwrap();
    }
    let (mut app, _) = App::new(Args::default());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(
        app.coordinator
            .as_ref()
            .unwrap()
            .inspect_at(root.path().into()),
    );
    assert!(result.result.is_ok());
    let _ = app.update(Message::Finished(result));
    app.root = "/Games/World of Warcraft".into();
    // App::new reads the developer's settings, and frames must not show a real address.
    app.email = "name@example.com".into();
    app.snapshot.realms =
        serde_json::from_str(include_str!("../../tests/fixtures/realm-selection.json")).unwrap();
    app.snapshot.registered_realms = vec![app.snapshot.realms[0].clone()];
    app.snapshot.sync = [
        (Game::Retail, "Silvermoon", "EU"),
        (Game::Progression, "Pyrewood Village-Alliance", "EU"),
        (Game::ClassicEra, "Firemaw-Horde", "Classic-EU"),
        (Game::Anniversary, "Thunderstrike-Horde", "Fresh-EU"),
    ]
    .into_iter()
    .map(|(game, name, region)| SyncRow {
        game,
        name: name.into(),
        region: region.into(),
        timestamp: 1_790_000_000,
        error: None,
        removable: game.subscribable(),
    })
    .collect();
    let entry = |time: &str, message, error| application::LogLine {
        time: time.into(),
        message,
        error,
    };
    app.snapshot.log = vec![
        entry("09:41:07", msg!(InstallationRead), false),
        entry(
            "09:40:52",
            msg!(
                TransportError,
                operation = "TSM API",
                host = "example.invalid",
                reason = msg!(CertificateUnverified)
            ),
            true,
        ),
        entry(
            "09:38:15",
            msg!(SyncComplete, updates = msg!(UpdateCount, count = 1_usize)),
            false,
        ),
    ];
    app.snapshot.backups = vec![Backup {
        account: "ACCOUNT-Anniversary".into(),
        system_id: "abc".into(),
        timestamp: 1_790_000_000,
        local: Some("/example/backup.zip".into()),
        remote: true,
        keep: true,
    }];
    for game in [Game::Progression, Game::ClassicEra, Game::Anniversary] {
        app.snapshot.installation.games.push(game);
        // Detection lists both addons for every game, installed or not.
        for name in ["TradeSkillMaster", "TradeSkillMaster_AppHelper"] {
            app.snapshot.installation.addons.push(crate::core::Addon {
                game,
                name: name.into(),
                version: None,
                development: false,
            });
        }
    }
    let mut auction_catalog = auction_preview();
    let icon_previews = if let Some(wow_path) = std::env::var_os("TSM_AUCTION_WOW_PATH") {
        application::icons::preview(
            &PathBuf::from(wow_path),
            &output.join("icon-cache"),
            Arc::make_mut(&mut auction_catalog),
        )
        .unwrap()
    } else {
        Default::default()
    };
    for &(name, page, width, height, theme, language) in SCENARIOS {
        let theme = theme();
        if std::env::var("TSM_CAPTURE_FILTER")
            .is_ok_and(|filter| !filter.split(',').any(|selected| selected == name))
        {
            continue;
        }
        app.page = page;
        app.width = width as f32;
        app.minimize_supported = Some(name == "settings-tray-en");
        app.auction = application::auction::Browser::default();
        app.auction_images = auction::Images::default();
        if name.contains("icons-missing") {
            app.auction_images.state.failed = true;
        } else if name.contains("icons-loading") {
            app.auction_images.state.in_flight = true;
        } else {
            for (&id, icon) in &icon_previews {
                app.auction_images.handles.insert(
                    id,
                    iced::widget::image::Handle::from_rgba(
                        icon.width,
                        icon.height,
                        icon.rgba.to_vec(),
                    ),
                );
            }
        }
        app.auction.replace(
            Some(&auction_catalog),
            &[application::Favorite {
                game: Game::Anniversary,
                item: "i:2770".into(),
            }],
        );
        if name.contains("auction-table") {
            let mut catalog = (*auction_catalog).clone();
            catalog.items = (0..3)
                .flat_map(|variant| {
                    auction_catalog.items.iter().cloned().map(move |mut item| {
                        if variant > 0 {
                            item.id = format!("{}:0:{variant}", item.id);
                        }
                        item.index();
                        item
                    })
                })
                .collect();
            app.auction.replace(Some(&Arc::new(catalog)), &[]);
        }
        if name.contains("auction-sort-quality") {
            app.auction
                .dispatch(application::auction::Browse::SortColumn(
                    application::SortField::Quality,
                ));
        }
        if name.contains("auction-sort-price") {
            app.auction
                .dispatch(application::auction::Browse::SortColumn(
                    application::SortField::Price,
                ));
        }
        if name.contains("auction-long") {
            let mut catalog = (*auction_catalog).clone();
            let mut item = catalog.items[0].clone();
            item.name = Some(
                "SupercalifragilisticHandcraftedEnchantedStormforgedReinforcedRunicEquipment"
                    .into(),
            );
            item.id = format!("i:123:0:1:{}", "1234567890".repeat(12));
            item.quality = Some(5);
            item.class = Some(8);
            item.min_buyout = application::AuctionValue::new(u64::MAX, 1791031782);
            item.auctions = application::AuctionValue::new(u64::MAX, 1791031782);
            item.region_historical = application::AuctionValue::new(u64::MAX, 1791031782);
            item.index();
            let id = item.id.clone();
            catalog.items = vec![item];
            app.auction.replace(Some(&Arc::new(catalog)), &[]);
            if name.contains("detail") {
                app.auction
                    .dispatch(application::auction::Browse::Select(id));
            }
        }
        if name.contains("auction-detail") {
            app.auction
                .dispatch(application::auction::Browse::Select("i:2770".into()));
        }
        if name.contains("auction-watchlist") {
            app.auction
                .dispatch(application::auction::Browse::Favorites(true));
        }
        if name.contains("auction-hot-deals") {
            app.auction
                .dispatch(application::auction::Browse::HotDeals(true));
        }
        if name == "auction-empty-en" {
            app.auction = application::auction::Browser::default();
        }
        app.settings.language = language;
        if name.contains("tooltip")
            && !name.contains("long")
            && !name.contains("missing")
            && !name.contains("hot")
        {
            app.auction
                .dispatch(application::auction::Browse::Search("Edgemaster".into()));
        }
        if name == "auction-qualities-de" {
            let mut catalog = (*auction_catalog).clone();
            catalog.items.truncate(9);
            for (quality, item) in catalog.items.iter_mut().enumerate() {
                item.quality = Some(quality as u8);
            }
            app.auction.replace(Some(&Arc::new(catalog)), &[]);
        }
        app.snapshot.logged_in = page != Page::Account;
        app.busy = name.contains("busy");
        app.adding_realm = app.busy.then(|| app.snapshot.realms[1].clone());
        app.realm_picker = realm_picker::State::new(RealmFilters::default());
        if name.contains("filtered") {
            app.realm_picker.query = "Pyrewood".into();
            app.realm_picker.filters = RealmFilters {
                game: Some(Game::Progression),
                region: Some("EU".into()),
                faction: Some(Faction::Horde),
            };
        }
        if name.contains("empty") {
            app.realm_picker.query = "missing realm".into();
        }
        app.status = if language == LanguagePreference::German {
            "Layoutprüfung · synthetische Testdaten"
        } else {
            "Layout preview · synthetic test data"
        }
        .into();
        crate::ui::components::load_fonts();
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
        let hover_name = name
            .contains("tooltip")
            .then(|| app.auction.visible().next().unwrap().name.clone().unwrap());
        let mut view = app.view();
        let mut tree = Tree::new(view.as_widget());
        let size = iced::Size::new(width as f32, height as f32);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        if name.starts_with("settings-tray-") {
            let mut operation = iced::advanced::widget::operation::scrollable::snap_to::<()>(
                PAGE_CONTENT.into(),
                iced::widget::operation::RelativeOffset::END.into(),
            );
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut operation);
        }
        if name.contains("prices") || (name.contains("scrolled") && !name.contains("menu")) {
            let mut operation = iced::advanced::widget::operation::scrollable::snap_to::<()>(
                (if app.auction.detail().is_some() {
                    auction::DETAIL_ID
                } else {
                    auction::RESULTS_ID
                })
                .into(),
                iced::widget::operation::RelativeOffset::END.into(),
            );
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut operation);
        }
        let viewport = iced::Rectangle::with_size(size);
        let mut messages = Vec::new();
        let menu = if name.contains("menu-sort") {
            Some("auction-selector-sort")
        } else if name.contains("menu-quality") {
            Some("auction-selector-quality")
        } else if name.contains("menu-market") {
            Some("auction-selector-market")
        } else if name.contains("menu-category") || name.contains("auction-category") {
            Some("auction-selector-category")
        } else {
            None
        };
        if let Some(id) = menu {
            let mut bounds = Bounds::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut bounds);
            let position = bounds.container(id).center();
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &viewport,
            );
            if name.contains("scrolled") {
                let mut overlay = view
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        iced::Vector::ZERO,
                    )
                    .unwrap();
                let menu_node = overlay.as_overlay_mut().layout(&renderer, size);
                let mut bounds = &menu_node;
                while bounds.bounds() == viewport && bounds.children().len() == 1 {
                    bounds = &bounds.children()[0];
                }
                overlay.as_overlay_mut().update(
                    &iced::Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -500.0 },
                    }),
                    Layout::new(&menu_node),
                    mouse::Cursor::Available(bounds.bounds().center()),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                );
            }
        }
        let hover_name = if name.contains("header-hover") {
            Some(
                app.language()
                    .text(
                        if name.contains("compact") || name.contains("sort-quality") {
                            Key::AuctionQuality
                        } else {
                            Key::AuctionSortName
                        },
                    )
                    .into(),
            )
        } else {
            hover_name
        };
        let cursor = if let Some(value) = hover_name {
            let mut found = Bounds::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut found);
            let bounds = found.text(&value);
            mouse::Cursor::Available(iced::Point::new(bounds.x + 6.0, bounds.y + 6.0))
        } else {
            mouse::Cursor::Unavailable
        };
        view.as_widget_mut().update(
            &mut tree,
            &iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
            Layout::new(&node),
            cursor,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut iced::advanced::Shell::new(&mut messages),
            &viewport,
        );
        if name.contains("tooltip") {
            // Wait for the hover delay, then for the open animation to settle.
            for wait in [320, 200] {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                    &viewport,
                );
            }
        }
        view.as_widget().draw(
            &tree,
            &mut renderer,
            &theme,
            &renderer::Style {
                text_color: theme.palette().text,
            },
            Layout::new(&node),
            cursor,
            &viewport,
        );
        if name.contains("tooltip") || menu.is_some() {
            let mut overlay = view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &viewport,
                    iced::Vector::ZERO,
                )
                .expect("interaction must open the real overlay");
            let node = overlay.as_overlay_mut().layout(&renderer, size);
            iced::advanced::Renderer::with_layer(&mut renderer, viewport, |renderer| {
                overlay.as_overlay().draw(
                    renderer,
                    &theme,
                    &renderer::Style {
                        text_color: theme.palette().text,
                    },
                    Layout::new(&node),
                    cursor,
                );
            });
        }
        let rgba = renderer.screenshot(
            iced::Size::new(width, height),
            1.0,
            theme.palette().background,
        );
        crate::adapters::desktop::save_capture(
            &output.join(format!("{name}.ppm")),
            width,
            height,
            &rgba,
        )
        .unwrap();
    }
}
fn auction_preview() -> Arc<application::AuctionCatalog> {
    use application::{AuctionCatalog, AuctionItem, AuctionValue as V, Market};
    let market = Market {
        game: Game::Anniversary,
        scope: "Thunderstrike-Horde".into(),
        region: Some("Fresh-EU".into()),
        region_only: false,
    };
    let mut items = vec![];
    for (n, (id, name, class, quality)) in [
        (2770, "Copper Ore", 7, 1),
        (2771, "Tin Ore", 7, 1),
        (2772, "Iron Ore", 7, 1),
        (2775, "Silver Ore", 7, 2),
        (2776, "Gold Ore", 7, 2),
        (2840, "Copper Bar", 7, 1),
        (2447, "Peacebloom", 7, 1),
        (2449, "Earthroot", 7, 1),
        (2450, "Briarthorn", 7, 1),
        (2589, "Linen Cloth", 7, 1),
        (4306, "Silk Cloth", 7, 1),
        (4338, "Mageweave Cloth", 7, 1),
        (13444, "Major Mana Potion", 0, 1),
        (13446, "Major Healing Potion", 0, 1),
        (14551, "Edgemaster's Handguards", 4, 4),
        (2244, "Krol Blade", 2, 4),
    ]
    .into_iter()
    .enumerate()
    {
        let mut item = AuctionItem {
            id: format!("i:{id}"),
            name: Some(name.into()),
            class: Some(class),
            quality: Some(quality),
            details: Some(crate::core::auction::ItemDetails {
                level: Some(70),
                required_level: Some(60),
                vendor_price: Some(12345),
                stack_size: Some(if class == 7 { 20 } else { 1 }),
                crafting_reagent: class == 7,
                ..Default::default()
            }),
            min_buyout: V::new(12345 + n as u64 * 21347, 1791031782),
            auctions: V::new(20 + n as u64, 1791031782),
            market: V::new(17456, 1791031782),
            recent: V::new(16432, 1791031782),
            historical: V::new(18220, 1790985909),
            region_market: V::new(15432, 1790985909),
            region_historical: V::new(19900, 1790985909),
            region_sale: V::new(11432, 1790986351),
            sale_rate: V::new(125, 1790986351),
            sold_per_day: V::new(14530, 1790986351),
            ..AuctionItem::default()
        };
        // Synthetic demand scenarios: liquid deals, a cheap slow seller, and unknown demand.
        if id == 2770 || id == 2771 {
            item.min_buyout = V::new(if id == 2770 { 8000 } else { 12000 }, 1791031782);
            item.market = V::new(20000, 1791031782);
            item.sale_rate = V::new(450, 1790986351);
            item.sold_per_day = V::new(82000, 1790986351);
        } else if id == 2244 {
            item.min_buyout = V::new(10000, 1791031782);
            item.market = V::new(200000, 1791031782);
            item.sale_rate = V::new(5, 1790986351);
            item.sold_per_day = V::new(20, 1790986351);
        } else if id == 14551 {
            item.sale_rate = None;
        }
        item.index();
        items.push(item);
    }
    Arc::new(AuctionCatalog {
        markets: vec![
            market.clone(),
            Market {
                scope: "Fresh-EU".into(),
                region_only: true,
                ..market.clone()
            },
        ],
        selected: Some(market),
        items,
        names_locale: Some("enUS".into()),
        warnings: vec![],
    })
}
