use super::*;
use crate::ui::test_support::Bounds;
use iced::advanced::{Layout, Renderer as _, layout, mouse, renderer, widget::Tree};
use renderer::Headless;

struct Table<'a> {
    view: Element<'a, Message>,
    tree: Tree,
    node: layout::Node,
    renderer: iced::Renderer,
    viewport: iced::Rectangle,
    /// Where `hover` left the pointer. Drawing reflects it.
    cursor: mouse::Cursor,
}
impl<'a> Table<'a> {
    fn new(app: &'a App, size: iced::Size) -> Self {
        Self::with_tree(app, size, None)
    }
    fn into_tree(self) -> Tree {
        self.tree
    }
    fn with_tree(app: &'a App, size: iced::Size, previous: Option<Tree>) -> Self {
        let mut view = app.view();
        let mut tree = previous.unwrap_or_else(|| Tree::new(view.as_widget()));
        tree.diff(view.as_widget());
        crate::ui::components::load_fonts();
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        Self {
            view,
            tree,
            node,
            renderer,
            viewport: iced::Rectangle::with_size(size),
            cursor: mouse::Cursor::Unavailable,
        }
    }
    fn overlay_bounds(&mut self) -> Option<iced::Rectangle> {
        let mut overlay = self.view.as_widget_mut().overlay(
            &mut self.tree,
            Layout::new(&self.node),
            &self.renderer,
            &self.viewport,
            iced::Vector::ZERO,
        )?;
        let node = overlay
            .as_overlay_mut()
            .layout(&self.renderer, self.viewport.size());
        // Containers can nest overlay groups, each covering the full window.
        let mut menu = &node;
        while menu.bounds() == self.viewport && menu.children().len() == 1 {
            menu = &menu.children()[0];
        }
        Some(menu.bounds())
    }
    /// Every overlay node's bounds, outermost first.
    fn overlay_nodes(&mut self) -> Vec<iced::Rectangle> {
        fn walk(node: &layout::Node, offset: iced::Vector, out: &mut Vec<iced::Rectangle>) {
            let bounds = node.bounds() + offset;
            out.push(bounds);
            for child in node.children() {
                walk(child, bounds.position() - iced::Point::ORIGIN, out);
            }
        }
        let Some(mut overlay) = self.view.as_widget_mut().overlay(
            &mut self.tree,
            Layout::new(&self.node),
            &self.renderer,
            &self.viewport,
            iced::Vector::ZERO,
        ) else {
            return vec![];
        };
        let node = overlay
            .as_overlay_mut()
            .layout(&self.renderer, self.viewport.size());
        let mut nodes = vec![];
        walk(&node, iced::Vector::ZERO, &mut nodes);
        nodes
    }
    fn bounds(&mut self) -> Bounds {
        let mut bounds = Bounds::default();
        self.view.as_widget_mut().operate(
            &mut self.tree,
            Layout::new(&self.node),
            &self.renderer,
            &mut bounds,
        );
        bounds
    }
    fn event(&mut self, event: iced::Event, cursor: iced::Point) -> Vec<Message> {
        let mut messages = vec![];
        let mut shell = iced::advanced::Shell::new(&mut messages);
        if let Some(mut overlay) = self.view.as_widget_mut().overlay(
            &mut self.tree,
            Layout::new(&self.node),
            &self.renderer,
            &self.viewport,
            iced::Vector::ZERO,
        ) {
            let node = overlay
                .as_overlay_mut()
                .layout(&self.renderer, self.viewport.size());
            overlay.as_overlay_mut().update(
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(cursor),
                &self.renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
        }
        if shell.is_event_captured() {
            return messages;
        }
        self.view.as_widget_mut().update(
            &mut self.tree,
            &event,
            Layout::new(&self.node),
            mouse::Cursor::Available(cursor),
            &self.renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &self.viewport,
        );
        messages
    }
    fn draw(&mut self, scale: f32, theme: &Theme) -> Vec<u8> {
        self.record(theme);
        self.renderer.screenshot(
            iced::Size::new(
                (self.viewport.width * scale) as u32,
                (self.viewport.height * scale) as u32,
            ),
            scale,
            theme.palette().background,
        )
    }
    /// Records the frame's layers without rasterizing them.
    fn record(&mut self, theme: &Theme) {
        self.event(
            iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
            self.cursor.position().unwrap_or(iced::Point::ORIGIN),
        );
        self.renderer.reset(self.viewport);
        self.view.as_widget().draw(
            &self.tree,
            &mut self.renderer,
            theme,
            &renderer::Style {
                text_color: theme.palette().text,
            },
            Layout::new(&self.node),
            self.cursor,
            &self.viewport,
        );
        if let Some(mut overlay) = self.view.as_widget_mut().overlay(
            &mut self.tree,
            Layout::new(&self.node),
            &self.renderer,
            &self.viewport,
            iced::Vector::ZERO,
        ) {
            let node = overlay
                .as_overlay_mut()
                .layout(&self.renderer, self.viewport.size());
            self.renderer.with_layer(self.viewport, |renderer| {
                overlay.as_overlay().draw(
                    renderer,
                    theme,
                    &renderer::Style {
                        text_color: theme.palette().text,
                    },
                    Layout::new(&node),
                    self.cursor,
                )
            });
        }
    }
    fn hover(&mut self, point: iced::Point) {
        self.cursor = mouse::Cursor::Available(point);
        self.event(
            iced::Event::Mouse(mouse::Event::CursorMoved { position: point }),
            point,
        );
    }
    fn click(&mut self, point: iced::Point) -> Vec<Message> {
        self.event(
            iced::Event::Mouse(mouse::Event::CursorMoved { position: point }),
            point,
        );
        let mut messages = self.event(
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            point,
        );
        messages.extend(self.event(
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            point,
        ));
        messages
    }
}

fn app(language: LanguagePreference) -> App {
    let (mut app, _) = App::new(Args::default());
    app.page = Page::AuctionHouse;
    app.busy = false;
    app.settings.language = language;
    app.status = "Synthetic scroll test".into();
    let market = application::Market {
        game: Game::Anniversary,
        scope: "Thunderstrike-Horde".into(),
        region: Some("Fresh-EU".into()),
        region_only: false,
    };
    let items = (0..56)
        .map(|n| {
            let mut item = AuctionItem {
                id: format!("i:{n}"),
                name: Some(format!("Item {n:03}")),
                class: Some(7),
                quality: Some(1),
                min_buyout: AuctionValue::new(12345, 1791031782),
                ..Default::default()
            };
            item.index();
            item
        })
        .collect();
    app.auction.replace(
        Some(&Arc::new(application::AuctionCatalog {
            markets: vec![market.clone()],
            selected: Some(market),
            items,
            names_locale: Some("enUS".into()),
            ..Default::default()
        })),
        &[],
    );
    app
}

#[test]
fn only_table_rows_scroll_and_clicks_follow_visible_items() {
    for (language, theme, size, scale) in [
        (
            LanguagePreference::German,
            super::super::theme::dark(),
            iced::Size::new(1120.0, 760.0),
            1.0,
        ),
        (
            LanguagePreference::English,
            super::super::theme::light(),
            iced::Size::new(820.0, 620.0),
            1.25,
        ),
    ] {
        let app = app(language);
        let mut table = Table::new(&app, size);
        let bounds = table.bounds();
        assert_eq!(
            bounds.scrolls.len(),
            1,
            "no outer page scrollbar on the list"
        );
        let body = bounds.scroll(RESULTS_ID);
        assert!(body.height > 60.0);
        let footer = bounds.text(app.language().text(Key::AuctionNext));
        assert!(footer.y > body.y + body.height && footer.y + footer.height <= size.height - 16.0);
        let first = bounds.text("Item 000");
        let row_height = bounds.text("Item 001").y - first.y;
        let cursor = iced::Point::new(first.x + 5.0, first.y + 5.0);
        let baseline = table.draw(scale, &theme);
        table.event(
            iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels {
                    x: 0.0,
                    y: -6.0 * row_height,
                },
            }),
            cursor,
        );
        let scrolled = table.draw(scale, &theme);
        assert_ne!(scrolled, baseline, "wheel must change visible rows");
        let width = (size.width * scale) as usize;
        let body_pixels = (body * scale).expand(1.0);
        for (index, pixel) in scrolled.as_chunks::<4>().0.iter().enumerate() {
            let point = iced::Point::new((index % width) as f32, (index / width) as f32);
            if !body_pixels.contains(point) {
                assert_eq!(
                    pixel,
                    &baseline[index * 4..index * 4 + 4],
                    "scroll painted outside table at {point:?}"
                );
            }
        }
        assert!(
            matches!(table.click(cursor).as_slice(), [Message::Auction(Browse::Select(id))] if id == "i:6")
        );
        assert!(matches!(
            table.click(footer.center()).as_slice(),
            [Message::Auction(Browse::Page(1))]
        ));
    }
}

#[test]
fn filter_menus_and_empty_results_keep_footer_inside_minimum_window() {
    let size = iced::Size::new(820.0, 620.0);
    for language in [LanguagePreference::German, LanguagePreference::English] {
        let mut app = app(language);
        for id in [
            "auction-selector-market",
            "auction-selector-category",
            "auction-selector-quality",
            "auction-selector-sort",
        ] {
            let mut table = Table::new(&app, size);
            let bounds = table.bounds();
            let body = bounds.scroll(RESULTS_ID);
            let next = bounds.text(app.language().text(Key::AuctionNext));
            let anchor = bounds.container(id);
            assert!(next.y > body.y + body.height && next.y + next.height < size.height);
            assert!(table.click(anchor.center()).is_empty());
            let overlay = table.overlay_bounds().expect("standard dropdown must open");
            assert!(
                (overlay.x - anchor.x).abs() < 1.0 && (overlay.width - anchor.width).abs() < 1.0,
                "{id}: menu={overlay:?} anchor={anchor:?}"
            );
            assert!(overlay.height <= 207.0);
            assert!(
                (overlay.y - anchor.y - anchor.height).abs() < 1.0
                    || (overlay.y + overlay.height - anchor.y).abs() < 1.0,
                "menu must touch its own field"
            );
            assert!(overlay.y >= 0.0 && overlay.y + overlay.height <= size.height);
            assert_eq!(
                table.bounds().text(app.language().text(Key::AuctionNext)),
                next
            );
            assert!(table.click(iced::Point::new(10.0, 10.0)).is_empty());
            assert!(
                table.overlay_bounds().is_none(),
                "outside click closes standard dropdown"
            );
        }
        app.auction.dispatch(Browse::Search("absent item".into()));
        let mut table = Table::new(&app, size);
        let bounds = table.bounds();
        assert!(
            bounds.text(app.language().text(Key::AuctionNoMatches)).y
                >= bounds.scroll(RESULTS_ID).y
        );
        let next = bounds.text(app.language().text(Key::AuctionNext));
        assert!(
            table.click(next.center()).is_empty(),
            "empty result cannot change page"
        );
        assert!(
            !bounds
                .texts
                .iter()
                .any(|(text, _)| text.contains("1 of 0") || text.contains("1 von 0"))
        );
    }
}

#[test]
fn headers_emit_column_sorts_and_native_menu_selects_scrolled_options() {
    for (language, size, scale, theme) in [
        (
            LanguagePreference::English,
            iced::Size::new(1120.0, 760.0),
            1.0,
            super::super::theme::dark(),
        ),
        (
            LanguagePreference::German,
            iced::Size::new(820.0, 620.0),
            1.25,
            super::super::theme::light(),
        ),
    ] {
        let app = app(language);
        let mut table = Table::new(&app, size);
        for (key, field) in [
            (Key::AuctionSortName, SortField::Name),
            (Key::AuctionQuality, SortField::Quality),
            (Key::AuctionMin, SortField::Price),
            (Key::AuctionCount, SortField::Auctions),
        ] {
            let bounds = table.bounds().text(app.language().text(key));
            assert!(matches!(table.click(bounds.center()).as_slice(),
            [Message::Auction(Browse::SortColumn(value))] if *value == field));
        }
        let anchor = table.bounds().container("auction-selector-sort");
        table.click(anchor.center());
        let menu = table.overlay_bounds().unwrap();
        let baseline = table.draw(scale, &theme);
        table.event(
            iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -500.0 },
            }),
            menu.center(),
        );
        let scrolled = table.draw(scale, &theme);
        assert!(baseline != scrolled, "menu wheel changed no pixels");
        let width = (size.width * scale) as usize;
        for (index, pixel) in scrolled.as_chunks::<4>().0.iter().enumerate() {
            let point = iced::Point::new((index % width) as f32, (index / width) as f32);
            if !(menu * scale).expand(1.0).contains(point) {
                assert_eq!(
                    pixel,
                    &baseline[index * 4..index * 4 + 4],
                    "menu scrolling leaked outside {point:?}"
                );
            }
        }
        assert!(matches!(
            table
                .click(iced::Point::new(menu.x + 20.0, menu.y + menu.height - 15.0))
                .as_slice(),
            [Message::Auction(Browse::Sort(AuctionSort::By(
                SortField::Auctions,
                SortDirection::Descending
            )))]
        ));
        assert!(table.overlay_bounds().is_none());
    }
}

#[test]
fn compact_demand_text_and_deal_icon_never_overlap() {
    let item = AuctionItem {
        min_buyout: AuctionValue::new(40, 100),
        market: AuctionValue::new(100, 100),
        sale_rate: AuctionValue::new(200, 100),
        sold_per_day: AuctionValue::new(5000, 100),
        ..Default::default()
    };
    for language in [Language::German, Language::English] {
        for width in [60.0, 80.0, 100.0, 300.0] {
            let mut view = demand::summary(&item, language, false);
            let mut tree = Tree::new(view.as_widget());
            crate::ui::components::load_fonts();
            let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 300.0)),
            );
            let mut bounds = Bounds::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut bounds);
            let label = bounds.text(language.text(Key::AuctionDemandHigh));
            let discount = bounds.text("−60%");
            // The 18px icon and its 3px gap precede the written discount.
            let deal = iced::Rectangle {
                x: discount.x - 21.0,
                y: discount.center_y() - 9.0,
                width: discount.width + 21.0,
                height: 18.0,
            };
            assert!(
                !deal.intersects(&label),
                "{language:?} {width}px: {label:?} overlaps {deal:?}"
            );
            assert!(deal.x >= 0.0 && deal.x + deal.width <= width + 0.1);
            assert!(label.x + label.width <= width + 0.1);
        }
    }
}

#[test]
fn detail_back_button_returns_to_filtered_results() {
    for language in [LanguagePreference::German, LanguagePreference::English] {
        let mut app = app(language);
        app.auction.dispatch(Browse::Search("Item".into()));
        app.auction.dispatch(Browse::SortColumn(SortField::Quality));
        app.auction.dispatch(Browse::Select("i:0".into()));
        let mut table = Table::new(&app, iced::Size::new(1120.0, 760.0));
        let back = table.bounds().text(app.language().text(Key::AuctionBack));
        let body = table.bounds().scroll(DETAIL_ID);
        assert!(back.y + back.height < body.y);
        // Wide windows keep the result list beside the item page.
        assert_eq!(table.bounds().scrolls.len(), 2);
        let list = table.bounds().scroll(RESULTS_ID);
        assert!(list.x + list.width < body.x);
        table.event(
            iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -450.0 },
            }),
            body.center(),
        );
        assert_eq!(
            table.bounds().text(app.language().text(Key::AuctionBack)),
            back,
            "detail navigation must stay fixed while its data scrolls"
        );
        let messages = table.click(back.center());
        assert!(
            matches!(messages.as_slice(), [Message::Auction(Browse::Back)]),
            "Back must emit navigation"
        );
        drop(table);
        let _ = app.update(messages.into_iter().next().unwrap());
        assert!(app.auction.detail().is_none());
        assert_eq!(app.auction.query.text, "Item");
        // The minimum window stacks the page alone.
        app.auction.dispatch(Browse::Select("i:0".into()));
        let mut table = Table::new(&app, iced::Size::new(820.0, 620.0));
        assert_eq!(table.bounds().scrolls.len(), 1);
        table.bounds().scroll(DETAIL_ID);
    }
}

#[test]
fn returning_from_details_survives_existing_widget_state_and_tooltips() {
    let mut app = app(LanguagePreference::English);
    let size = iced::Size::new(1120.0, 760.0);
    let mut previous = None;
    for _ in 0..3 {
        let mut table = Table::with_tree(&app, size, previous.take());
        let point = table.bounds().text("Item 000").center();
        let select = table.click(point);
        assert!(matches!(
            select.as_slice(),
            [Message::Auction(Browse::Select(_))]
        ));
        previous = Some(table.into_tree());
        let _ = app.update(select.into_iter().next().unwrap());
        let mut table = Table::with_tree(&app, size, previous.take());
        let identity = table.bounds().text("Item 000").center();
        table.event(
            iced::Event::Mouse(mouse::Event::CursorMoved { position: identity }),
            identity,
        );
        std::thread::sleep(Duration::from_millis(310));
        table.event(
            iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
            identity,
        );
        assert!(table.overlay_bounds().is_some());
        let back = table
            .bounds()
            .text(app.language().text(Key::AuctionBack))
            .center();
        let messages = table.click(back);
        assert!(matches!(
            messages.as_slice(),
            [Message::Auction(Browse::Back)]
        ));
        previous = Some(table.into_tree());
        let _ = app.update(messages.into_iter().next().unwrap());
        assert!(app.auction.detail().is_none());
    }
}

#[test]
fn sidebar_and_escape_leave_details_without_a_reload_or_filter_loss() {
    for busy in [false, true] {
        for route in [Message::Page(Page::AuctionHouse), Message::CancelConfirm] {
            let mut app = app(LanguagePreference::English);
            app.busy = busy;
            app.auction.dispatch(Browse::Search("Item".into()));
            app.auction.dispatch(Browse::SortColumn(SortField::Price));
            app.auction.dispatch(Browse::Page(1));
            app.auction.dispatch(Browse::Select("i:40".into()));
            let catalog = app.auction.catalog.clone();
            let _ = app.update(route);
            assert!(app.auction.detail().is_none());
            assert_eq!(app.auction.page, 1);
            assert_eq!(app.auction.query.text, "Item");
            assert_eq!(
                app.auction.query.sort,
                AuctionSort::By(SortField::Price, SortDirection::Ascending)
            );
            assert_eq!(app.busy, busy, "navigation must not start a catalog reload");
            assert!(Arc::ptr_eq(&catalog, &app.auction.catalog));
        }
    }
}

/// Overlays share a layer that paints quads before text. A tooltip opened over
/// a select menu must still hide the menu's labels.
#[test]
fn item_tooltips_cover_an_open_select_menu() {
    let size = iced::Size::new(1120.0, 760.0);
    let app = app(LanguagePreference::English);
    let theme = super::super::theme::dark();
    let mut table = Table::new(&app, size);
    let row = table.bounds().text("Item 000");
    let anchor = table.bounds().container("auction-selector-market");
    let point = iced::Point::new(anchor.x + 40.0, row.y + 4.0);
    let tooltip_over = |table: &mut Table| {
        table.hover(point);
        // Hover delay, then the fade-in.
        std::thread::sleep(std::time::Duration::from_millis(350));
        table.record(&theme);
        std::thread::sleep(std::time::Duration::from_millis(200));
        table.draw(1.0, &theme)
    };
    table.click(anchor.center());
    let menu = table.overlay_bounds().unwrap();
    let with_menu = tooltip_over(&mut table);
    let tip = table
        .overlay_nodes()
        .into_iter()
        .filter(|node| node.width < size.width && !node.is_within(&menu))
        .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
        .unwrap();
    let covered = tip.intersection(&menu).expect("tooltip overlaps the menu");
    table.click(iced::Point::new(10.0, 10.0));
    assert!(table.overlay_bounds().is_none(), "menu and tooltip closed");
    let without_menu = tooltip_over(&mut table);
    let width = size.width as usize;
    let differs = (covered.y as usize + 2..(covered.y + covered.height) as usize - 2)
        .flat_map(|y| {
            (covered.x as usize + 2..(covered.x + covered.width) as usize - 2)
                .map(move |x| (y * width + x) * 4)
        })
        .filter(|&i| with_menu[i..i + 4] != without_menu[i..i + 4])
        .count();
    assert_eq!(differs, 0, "menu shows through the tooltip in {covered:?}");
}

/// A select's label is centred on its line, so it paints above its anchor.
/// Changing it must repaint all of it, not only the lower half.
#[test]
fn changed_select_labels_are_damaged_where_they_paint() {
    use iced::advanced::graphics::damage;
    let label = |content: &str| {
        let mut layer = iced_tiny_skia::Layer {
            bounds: iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(400.0, 100.0)),
            ..Default::default()
        };
        layer.draw_text(
            iced::advanced::Text {
                content: content.to_owned(),
                bounds: iced::Size::new(300.0, 18.2),
                size: iced::Pixels(14.0),
                line_height: iced::widget::text::LineHeight::default(),
                font: iced::Font::DEFAULT,
                align_x: iced::advanced::text::Alignment::Left,
                align_y: iced::alignment::Vertical::Center,
                shaping: iced::advanced::text::Shaping::Advanced,
                wrapping: iced::advanced::text::Wrapping::None,
            },
            iced::Point::new(10.0, 50.0),
            iced::Color::WHITE,
            layer.bounds,
            iced::Transformation::IDENTITY,
        );
        vec![layer]
    };
    let damage = damage::diff(
        &label("Thunderstrike-Horde"),
        &label("Spineshatter-Horde"),
        |layer| vec![layer.bounds],
        iced_tiny_skia::Layer::damage,
    );
    let top = iced::Point::new(20.0, 50.0 - 18.2 / 2.0 + 1.0);
    assert!(damage.iter().any(|area| area.contains(top)), "{damage:?}");
}

/// Replays what the window compositor does on a swapchain: it redraws only the
/// damage since the buffer it got back was last presented. Every frame must
/// still match a full redraw, or reused buffers alternate between stale pixels.
#[test]
fn partial_redraws_of_open_menus_match_full_redraws() {
    let size = iced::Size::new(1120.0, 760.0);
    let mut app = app(LanguagePreference::German);
    let catalog = application::AuctionCatalog {
        markets: [
            "Thunderstrike-Horde",
            "Thunderstrike-Allianz",
            "Spineshatter-Horde",
        ]
        .map(|scope| application::Market {
            game: Game::Anniversary,
            scope: scope.into(),
            region: Some("Fresh-EU".into()),
            region_only: false,
        })
        .into(),
        ..Default::default()
    };
    app.auction.replace(Some(&Arc::new(catalog)), &[]);
    replay_damage(&app, size, 9, |table, frame| {
        let anchor = table.bounds().container("auction-selector-market");
        match frame {
            0 => {}
            1 => {
                table.click(anchor.center());
            }
            8 => {
                table.click(iced::Point::new(10.0, 10.0));
            }
            _ => {
                let menu = table.overlay_bounds().unwrap();
                assert!(
                    (menu.height - 3.0 * 36.2).abs() < 0.5,
                    "three markets fit without a scrollbar: {menu:?}"
                );
                let row = [0.5, 1.5, 2.5, 1.5, 0.5, 2.5][frame - 2];
                let point = iced::Point::new(menu.x + 20.0, menu.y + 36.2 * row);
                table.event(
                    iced::Event::Mouse(mouse::Event::CursorMoved { position: point }),
                    point,
                );
            }
        }
    });
}

/// Draws `frames` frames, calling `act` before each, through the compositor's
/// damage path on swapchains of one to three buffers at two scales.
fn replay_damage(app: &App, size: iced::Size, frames: usize, act: impl Fn(&mut Table, usize)) {
    use iced::advanced::graphics::{Viewport, damage};
    let whole = iced::Rectangle::with_size(size);
    for (scale, theme) in [
        (1.0, super::super::theme::dark()),
        (1.25, super::super::theme::light()),
    ] {
        let viewport = Viewport::with_physical_size(
            iced::Size::new((size.width * scale) as u32, (size.height * scale) as u32),
            scale,
        );
        let (width, height) = (
            viewport.physical_size().width,
            viewport.physical_size().height,
        );
        let background = theme.palette().background;
        let mut clip = tiny_skia::Mask::new(width, height).unwrap();
        for buffers in 1..=3 {
            let mut table = Table::new(app, size);
            // Each buffer keeps its pixels and the layers it was last drawn from.
            let mut swapchain: Vec<(tiny_skia::Pixmap, Option<Vec<iced_tiny_skia::Layer>>)> = (0
                ..buffers)
                .map(|_| (tiny_skia::Pixmap::new(width, height).unwrap(), None))
                .collect();
            for frame in 0..frames {
                act(&mut table, frame);
                table.record(&theme);
                let layers = table.renderer.layers().to_vec();
                let (pixels, shown) = &mut swapchain[frame % buffers];
                let damage = shown.as_ref().map_or_else(
                    || vec![whole],
                    |previous| {
                        damage::diff(
                            previous,
                            &layers,
                            |layer| vec![layer.bounds],
                            iced_tiny_skia::Layer::damage,
                        )
                    },
                );
                if !damage.is_empty() {
                    let damage = damage::group(damage, whole);
                    table.renderer.draw(
                        &mut pixels.as_mut(),
                        &mut clip,
                        &viewport,
                        &damage,
                        background,
                    );
                }
                *shown = Some(layers);
                let mut expected = tiny_skia::Pixmap::new(width, height).unwrap();
                table.renderer.draw(
                    &mut expected.as_mut(),
                    &mut clip,
                    &viewport,
                    &[whole],
                    background,
                );
                let stale = pixels
                    .data()
                    .chunks(4)
                    .zip(expected.data().chunks(4))
                    .enumerate()
                    // Masked and unmasked tiny-skia pipelines round differently.
                    .filter(|(_, (a, b))| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 2))
                    .map(|(i, _)| (i as u32 % width, i as u32 / width))
                    .collect::<Vec<_>>();
                assert!(
                    stale.is_empty(),
                    "scale {scale}, {buffers} buffers, frame {frame}: {} stale pixels, \
                     first {:?}, last {:?}",
                    stale.len(),
                    stale.first(),
                    stale.last()
                );
            }
        }
    }
}

#[test]
fn hovering_controls_beside_the_version_icon_matches_full_redraws() {
    let size = iced::Size::new(1120.0, 760.0);
    let mut app = app(LanguagePreference::English);
    app.version_icons.insert(
        Game::Anniversary,
        iced::widget::svg::Handle::from_memory(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#8c3"/></svg>"##
                .as_slice(),
        ),
    );
    let controls = [
        "auction-selector-category",
        "auction-selector-quality",
        "auction-selector-sort",
        "auction-selector-market",
        "auction-selector-category",
    ];
    replay_damage(&app, size, controls.len() * 2 + 1, |table, frame| {
        if frame > 0 {
            let bounds = table.bounds().container(controls[(frame - 1) / 2]);
            // Enter, then move within the control.
            let x = if frame % 2 == 1 { 0.3 } else { 0.6 };
            table.hover(iced::Point::new(
                bounds.x + bounds.width * x,
                bounds.center_y(),
            ));
        }
    });
}
