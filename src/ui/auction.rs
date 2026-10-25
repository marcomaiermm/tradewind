//! Read-only auction browser with fixed controls and a scrolling results body.
use super::*;
mod demand;
pub(super) mod item;
pub(super) mod money;
#[cfg(test)]
mod scroll_tests;
mod sorting;
use application::{
    AuctionItem, AuctionSort, AuctionValue, Favorite, SortDirection, SortField,
    auction::{Browse, Browser},
};
use iced::widget::text::Wrapping;
use iced::widget::{column, responsive};
#[derive(Default)]
pub(super) struct Images {
    pub handles: std::collections::BTreeMap<u32, iced::widget::image::Handle>,
    /// What is loading or failed. The handles follow its generation.
    pub state: application::icons::Icons,
}
pub(super) const SEARCH_ID: &str = "auction-search";
pub(super) const RESULTS_ID: &str = "auction-results";
pub(super) const DETAIL_ID: &str = "auction-detail";
fn selector<'a, T: Clone + PartialEq + 'a>(
    id: &'static str,
    choices: Vec<Choice<T>>,
    selected: Choice<T>,
    on_select: impl Fn(Choice<T>) -> Message + 'a,
    width: Length,
) -> Element<'a, Message> {
    // Use Iced's own anchored, scrollable menu. Limit visible rows, not the choices.
    // A row is the 14px text's line height plus the select's vertical padding (9 + 9).
    let height = (choices.len() as f32 * (14.0 * 1.3 + 18.0)).min(206.0);
    container(
        select(choices, Some(selected), on_select)
            .width(Length::Fill)
            .menu_height(height),
    )
    .id(id)
    .width(width)
    .into()
}
fn message(action: Browse) -> Message {
    Message::Auction(action)
}
fn category(class: u8, l: Language) -> &'static str {
    l.text(match class {
        0 => Key::CategoryConsumable,
        1 => Key::CategoryContainer,
        2 => Key::CategoryWeapon,
        3 => Key::CategoryGem,
        4 => Key::CategoryArmor,
        5 => Key::CategoryReagent,
        6 => Key::CategoryProjectile,
        7 => Key::CategoryTrade,
        8 => Key::CategoryEnhancement,
        9 => Key::CategoryRecipe,
        11 => Key::CategoryQuiver,
        12 => Key::CategoryQuest,
        15 => Key::CategoryMisc,
        16 => Key::CategoryGlyph,
        17 => Key::CategoryPet,
        18 => Key::CategoryToken,
        19 => Key::CategoryProfession,
        _ => Key::Unknown,
    })
}
fn quality(quality: u8, l: Language) -> &'static str {
    l.text(match quality {
        0 => Key::QualityPoor,
        1 => Key::QualityCommon,
        2 => Key::QualityUncommon,
        3 => Key::QualityRare,
        4 => Key::QualityEpic,
        5 => Key::QualityLegendary,
        6 => Key::QualityArtifact,
        7 => Key::QualityHeirloom,
        8 => Key::QualityToken,
        _ => Key::Unknown,
    })
}
fn name(item: &AuctionItem, l: Language) -> String {
    item.name
        .clone()
        .unwrap_or_else(|| msg!(AuctionUnnamed, id = item.id.trim_start_matches("i:")).render(l))
}
fn sort_label(sort: AuctionSort, l: Language) -> &'static str {
    use SortDirection::{Ascending, Descending};
    use SortField::*;
    l.text(match sort {
        AuctionSort::Recommended => Key::AuctionSortRecommended,
        AuctionSort::By(Name, Ascending) => Key::AuctionSortNameUp,
        AuctionSort::By(Name, Descending) => Key::AuctionSortNameDown,
        AuctionSort::By(Quality, Ascending) => Key::AuctionSortQualityUp,
        AuctionSort::By(Quality, Descending) => Key::AuctionSortQualityDown,
        AuctionSort::By(Price, Ascending) => Key::AuctionSortPriceUp,
        AuctionSort::By(Price, Descending) => Key::AuctionSortPriceDown,
        AuctionSort::By(Market, Ascending) => Key::AuctionSortMarketUp,
        AuctionSort::By(Market, Descending) => Key::AuctionSortMarket,
        AuctionSort::By(Auctions, Ascending) => Key::AuctionSortSupplyUp,
        AuctionSort::By(Auctions, Descending) => Key::AuctionSortSupply,
    })
}
fn market_label(market: &application::Market, l: Language) -> String {
    let version = market.game.label();
    let region = if market.region_only {
        l.text(Key::AuctionRegion)
    } else {
        market.region.as_deref().unwrap_or("")
    };
    [market.scope.as_str(), region, version]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}
pub(super) fn view<'a>(
    browser: &'a Browser,
    l: Language,
    busy: bool,
    images: &'a Images,
    version_mark: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let catalog = &browser.catalog;
    let selected = catalog
        .selected
        .as_ref()
        .map_or_else(|| l.text(Key::AuctionMarket).into(), |m| market_label(m, l));
    let mut content = column![
        row![]
            .extend(version_mark)
            .push(selector(
                "auction-selector-market",
                catalog
                    .markets
                    .iter()
                    .map(|m| Choice::new(Some(m.clone()), market_label(m, l)))
                    .collect(),
                Choice::new(catalog.selected.clone(), selected),
                |choice| Message::BrowseMarket(choice.value),
                Length::Fill,
            ))
            .push(
                action(
                    l.text(Key::AuctionReload),
                    (!busy).then(|| Message::BrowseMarket(catalog.selected.clone())),
                )
                .style(outline),
            )
            .spacing(10)
            .align_y(alignment::Vertical::Center)
    ]
    .spacing(8);
    // The market label already names realm, faction and version, so no badge row repeats it.
    if catalog.selected.is_none() {
        return content
            .push(card(
                column![
                    heading(l.text(Key::AuctionEmpty)),
                    text(l.text(Key::AuctionEmptyHint)).size(15),
                    action(l.text(Key::Realms), Some(Message::Page(Page::Realms))).style(outline)
                ]
                .spacing(12),
            ))
            .into();
    }
    if let Some(item) = browser.detail() {
        // The page takes the item's quality colour, like its icon frame.
        let seed = item
            .quality
            .map_or(identity::NEUTRAL_SEED, item::quality_color);
        // Launcher layout: the list stays beside the item page while there is room for both.
        return content
            .push(responsive(move |size| {
                let page = game_page(browser, item, l, busy, images, seed);
                if size.width < SPLIT_WIDTH {
                    return page;
                }
                row![
                    column![
                        input(l.text(Key::AuctionSearch), &browser.query.text)
                            .id(SEARCH_ID)
                            .on_input(|s| message(Browse::Search(s))),
                        results(browser, l, images, Some(&item.id)),
                        rule::horizontal(1),
                        pagination(browser, l),
                    ]
                    .spacing(8)
                    .width(LIST_WIDTH),
                    page
                ]
                .spacing(16)
                .into()
            }))
            .height(Length::Fill)
            .into();
    }
    let query = &browser.query;
    let sorts = std::iter::once(AuctionSort::Recommended)
        .chain(
            [
                SortField::Name,
                SortField::Quality,
                SortField::Price,
                SortField::Market,
                SortField::Auctions,
            ]
            .into_iter()
            .flat_map(|field| {
                [SortDirection::Ascending, SortDirection::Descending]
                    .map(move |direction| AuctionSort::By(field, direction))
            }),
        )
        .map(|sort| Choice::new(sort, sort_label(sort, l)))
        .collect();
    let sort = selector(
        "auction-selector-sort",
        sorts,
        Choice::new(query.sort, sort_label(query.sort, l)),
        |choice| message(Browse::Sort(choice.value)),
        Length::Fixed(240.0),
    );
    content = content.push(
        row![
            input(l.text(Key::AuctionSearch), &query.text)
                .id(SEARCH_ID)
                .on_input(|s| message(Browse::Search(s))),
            sort
        ]
        .spacing(8),
    );
    // One wrapping filter line: on narrow windows the toggles move below the selects.
    content = content.push(
        row![
            selector(
                "auction-selector-category",
                std::iter::once(Choice::new(None, l.text(Key::AuctionAllCategories)))
                    .chain(
                        browser
                            .classes
                            .iter()
                            .map(|&c| Choice::new(Some(c), category(c, l)))
                    )
                    .collect(),
                Choice::new(
                    query.class,
                    query
                        .class
                        .map_or(l.text(Key::AuctionAllCategories), |c| category(c, l))
                ),
                |choice| message(Browse::Class(choice.value)),
                Length::Fixed(200.0),
            ),
            selector(
                "auction-selector-quality",
                std::iter::once(Choice::new(None, l.text(Key::AuctionAllQualities)))
                    .chain((0..=8).map(|q| Choice::new(Some(q), quality(q, l))))
                    .collect(),
                Choice::new(
                    query.quality,
                    query
                        .quality
                        .map_or(l.text(Key::AuctionAllQualities), |q| quality(q, l))
                ),
                |choice| message(Browse::Quality(choice.value)),
                Length::Fixed(180.0),
            ),
            container(
                row![
                    check(query.favorites_only)
                        .label(l.text(Key::AuctionWatchlist))
                        .on_toggle(|v| message(Browse::Favorites(v))),
                    check(query.available_only)
                        .label(l.text(Key::AuctionAvailable))
                        .on_toggle(|v| message(Browse::Available(v))),
                    check(query.hot_deals_only)
                        .label(l.text(Key::AuctionHotDealsOnly))
                        .on_toggle(|v| message(Browse::HotDeals(v))),
                    button(text(l.text(Key::AuctionReset)).size(13))
                        .padding([6, 8])
                        .style(ghost)
                        .on_press(message(Browse::Reset))
                ]
                .spacing(12)
                .align_y(alignment::Vertical::Center)
            )
            .height(38)
            .align_y(alignment::Vertical::Center),
        ]
        .spacing(12)
        .wrap()
        .vertical_spacing(8),
    );
    content = content.push(
        container(
            row![
                sorting::header(
                    l.text(Key::AuctionSortName),
                    SortField::Name,
                    query.sort,
                    Length::Fill,
                    l
                ),
                sorting::header(
                    l.text(Key::AuctionQuality),
                    SortField::Quality,
                    query.sort,
                    sorting::QUALITY_WIDTH.into(),
                    l
                ),
                sorting::header(
                    l.text(Key::AuctionMin),
                    SortField::Price,
                    query.sort,
                    sorting::PRICE_WIDTH.into(),
                    l
                ),
                sorting::header(
                    l.text(Key::AuctionCount),
                    SortField::Auctions,
                    query.sort,
                    sorting::COUNT_WIDTH.into(),
                    l
                ),
            ]
            .spacing(8),
        )
        .padding(iced::Padding {
            left: 8.0,
            right: 20.0,
            ..iced::Padding::ZERO
        }),
    );
    content
        .push(results(browser, l, images, None))
        .push(rule::horizontal(1))
        .push(pagination(browser, l))
        .height(Length::Fill)
        .into()
}
const SPLIT_WIDTH: f32 = 960.0;
const LIST_WIDTH: f32 = 380.0;
/// Result rows. `selected` switches to the compact list beside an open item page.
fn results<'a>(
    browser: &'a Browser,
    l: Language,
    images: &'a Images,
    selected: Option<&str>,
) -> Element<'a, Message> {
    let catalog = &browser.catalog;
    let mut notices = column![].spacing(8).width(Length::Fill);
    for warning in &catalog.warnings {
        notices = notices.push(text(warning.render(l)).size(14).width(Length::Fill));
    }
    if images.state.failed {
        notices = notices.push(caption(l.text(Key::AuctionIconsUnavailable)).width(Length::Fill));
    }
    let mut results = column![notices].spacing(0).width(Length::Fill);
    if browser.missing_names > 0 {
        results = results.push(
            container(
                caption(msg!(AuctionNamesHint, count = browser.missing_names).render(l))
                    .width(Length::Fill),
            )
            .padding([8, 0]),
        );
    }
    if browser.total() == 0 {
        results = results.push(card(
            column![
                heading(l.text(Key::AuctionNoMatches)),
                text(l.text(Key::AuctionNoMatchesHint)).size(15)
            ]
            .spacing(12),
        ));
    } else {
        for item in browser.visible() {
            let entry = if selected.is_some() {
                compact_row(item, l, images)
            } else {
                full_row(item, l, images)
            };
            let current = selected == Some(item.id.as_str());
            results = results
                .push(
                    button(entry)
                        .width(Length::Fill)
                        .padding(if selected.is_some() { [8, 8] } else { [10, 8] })
                        .on_press(message(Browse::Select(item.id.clone())))
                        .style(move |theme: &Theme, status| {
                            let mut style = ghost(theme, status);
                            if current {
                                let t = theme::tokens(theme);
                                style.background =
                                    Some(identity::mix(t.background, t.ring, 0.16).into());
                            }
                            style
                        }),
                )
                .push(rule::horizontal(1));
        }
    }
    let body = scroll(results).id(RESULTS_ID).height(Length::Fill);
    body.into()
}
fn full_row<'a>(item: &'a AuctionItem, l: Language, images: &'a Images) -> Element<'a, Message> {
    let time = item
        .min_buyout
        .map(|value| l.timestamp(value.timestamp, false))
        .unwrap_or_else(|| l.text(Key::AuctionNoValue).into());
    row![
        item::hover(
            row![
                item::icon(item, images, 40.0),
                column![
                    item::title(item, l, 15, identity::Surface::Page),
                    demand::summary(item, l, false)
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center)
            .width(Length::Fill),
            item,
            images,
            l
        ),
        sorting::quality_cell(item, l),
        column![
            money::view(
                item.min_buyout.map(|v| v.amount),
                images,
                l,
                identity::Surface::Page
            ),
            caption(time)
                .width(Length::Fill)
                .wrapping(Wrapping::WordOrGlyph)
        ]
        .spacing(5)
        .width(sorting::PRICE_WIDTH),
        text(
            item.auctions
                .map_or_else(|| "-".into(), |v| l.number(v.amount))
        )
        .size(14)
        .width(sorting::COUNT_WIDTH)
        .wrapping(Wrapping::WordOrGlyph)
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .into()
}
/// Beside the item page only name, demand and minimum price matter.
fn compact_row<'a>(item: &'a AuctionItem, l: Language, images: &'a Images) -> Element<'a, Message> {
    row![
        item::hover(
            row![
                item::icon(item, images, 28.0),
                column![
                    item::title(item, l, 14, identity::Surface::Page),
                    demand::summary(item, l, false)
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center)
            .width(Length::Fill),
            item,
            images,
            l
        ),
        money::view(
            item.min_buyout.map(|v| v.amount),
            images,
            l,
            identity::Surface::Page
        ),
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .into()
}
fn pagination(browser: &Browser, l: Language) -> Element<'static, Message> {
    let count = caption(msg!(AuctionResults, count = browser.total()).render(l));
    let count: Element<'_, Message> = if let Some(locale) = &browser.catalog.names_locale {
        tooltip(
            count,
            caption(msg!(AuctionNamesLocale, locale = locale).render(l)),
            Position::Top,
        )
        .style(popover)
        .into()
    } else {
        count.into()
    };
    row![
        count,
        space().width(Length::Fill),
        action(
            l.text(Key::AuctionPrevious),
            (browser.page > 0).then(|| message(Browse::Page(browser.page - 1)))
        )
        .style(outline),
        caption(if browser.page_count() == 0 {
            String::new()
        } else {
            msg!(
                AuctionPage,
                page = browser.page + 1,
                pages = browser.page_count()
            )
            .render(l)
        }),
        action(
            l.text(Key::AuctionNext),
            (browser.page + 1 < browser.page_count())
                .then(|| message(Browse::Page(browser.page + 1)))
        )
        .style(outline)
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Launcher game page: tinted hero with the item, then price tiles and facts.
/// Navigation stays fixed in the hero while the data below scrolls.
fn game_page<'a>(
    browser: &'a Browser,
    item: &'a AuctionItem,
    l: Language,
    busy: bool,
    images: &'a Images,
    seed: iced::Color,
) -> Element<'a, Message> {
    let favorite = Favorite {
        game: browser.catalog.selected.as_ref().unwrap().game,
        item: item.id.clone(),
    };
    let mut subtitle = Vec::new();
    if let Some(class) = item.class {
        subtitle.push(category(class, l).to_owned());
    }
    if let Some(q) = item.quality {
        subtitle.push(quality(q, l).to_owned());
    }
    subtitle.push(msg!(AuctionUnnamed, id = item.id.trim_start_matches("i:")).render(l));
    let hero = row![
        item::hover(item::icon(item, images, 80.0), item, images, l),
        column![
            item::title(item, l, 26, identity::Surface::Hero(seed)).font(BOLD),
            caption(subtitle.join(" · ")).wrapping(Wrapping::WordOrGlyph),
            demand::summary(item, l, true),
        ]
        .spacing(6)
        .width(Length::Fill),
        column![
            action(
                l.text(if browser.favorite(item) {
                    Key::AuctionUnwatch
                } else {
                    Key::AuctionWatch
                }),
                (!busy).then_some(Message::ToggleAuctionFavorite(favorite)),
            )
            .style(secondary),
            action(l.text(Key::AuctionBack), Some(message(Browse::Back))).style(outline),
        ]
        .spacing(8)
        .align_x(alignment::Horizontal::Right),
    ]
    .spacing(20)
    .align_y(alignment::Vertical::Center);
    let tile = |key: Key, value: Option<AuctionValue>, format: u8| -> Element<'a, Message> {
        let surface = identity::Surface::Tile(seed);
        let shown: Element<'a, Message> = match value {
            None => text(l.text(Key::AuctionNoValue)).size(16).into(),
            Some(v) if format == 0 => money::view(Some(v.amount), images, l, surface),
            Some(v) => text(metric(v, format, l)).size(16).into(),
        };
        let mut body = column![label(l.text(key)), shown].spacing(6);
        if let Some(value) = value {
            body = body.push(
                caption(msg!(AuctionAsOf, time = l.timestamp(value.timestamp, false)).render(l))
                    .wrapping(Wrapping::WordOrGlyph),
            );
        }
        container(body)
            .padding([12, 14])
            .width(Length::Fill)
            .style(identity::tile(seed))
            .into()
    };
    // Two by two: four price tiles beside the list leave each date no room on one line.
    let prices = column![
        row![
            tile(Key::AuctionMin, item.min_buyout, 0),
            tile(Key::AuctionMarketValue, item.market, 0),
        ]
        .spacing(10),
        row![
            tile(Key::AuctionRecent, item.recent, 0),
            tile(Key::AuctionHistorical, item.historical, 0),
        ]
        .spacing(10),
    ]
    .spacing(10);
    let market = row![
        tile(Key::AuctionCount, item.auctions, 1),
        tile(Key::AuctionSaleRate, item.sale_rate, 2),
        tile(Key::AuctionSoldPerDay, item.sold_per_day, 3),
    ]
    .spacing(10);
    // Own heading, so the regional values can drop "Regional" from their labels.
    let region = row![
        tile(Key::AuctionRegionMarket, item.region_market, 0),
        tile(Key::AuctionRegionHistorical, item.region_historical, 0),
        tile(Key::AuctionRegionSale, item.region_sale, 0),
    ]
    .spacing(10);
    let mut sections = column![
        label(l.text(Key::AuctionPrices)),
        prices,
        label(l.text(Key::AuctionMarketSection)),
        market,
        label(l.text(Key::AuctionRegionSection)),
        region,
        label(l.text(Key::AuctionItemSection)),
        container(item::facts(item, images, l, identity::Surface::Tile(seed)))
            .padding([12, 14])
            .width(Length::Fill)
            .style(identity::tile(seed)),
        caption(l.text(Key::AuctionTooltipLimit)),
    ]
    .spacing(10);
    if item.commodity {
        sections = sections.push(caption(l.text(Key::AuctionCommodity)));
    }
    if item.class.is_none() && item.quality.is_none() {
        sections = sections.push(caption(l.text(Key::AuctionUnknownMetadata)));
    }
    sections = sections
        .push(caption(l.text(Key::AuctionMetricHint)))
        .push(caption(l.text(Key::AuctionKeysHint)));
    container(
        column![
            container(hero).padding([20, 24]),
            // The hero casts a shadow over the tiles, so they read as scrolling beneath it.
            iced::widget::stack![
                container(
                    scroll(container(sections).padding(iced::Padding {
                        top: 12.0,
                        right: 0.0,
                        bottom: 20.0,
                        left: 24.0,
                    }))
                    .id(DETAIL_ID)
                    .height(Length::Fill)
                )
                .padding(iced::Padding {
                    right: 8.0,
                    ..iced::Padding::ZERO
                }),
                container(iced::widget::Space::new())
                    .width(Length::Fill)
                    .height(12)
                    .style(|theme: &Theme| {
                        let shadow = if theme.extended_palette().is_dark {
                            0.45
                        } else {
                            0.12
                        };
                        container::Style {
                            background: Some(
                                iced::gradient::Linear::new(std::f32::consts::PI)
                                    .add_stop(0.0, iced::Color::BLACK.scale_alpha(shadow))
                                    .add_stop(1.0, iced::Color::TRANSPARENT)
                                    .into(),
                            ),
                            ..Default::default()
                        }
                    }),
            ],
        ]
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(identity::tinted_page(seed))
    .into()
}
/// Counts and rates. Money goes through `money::view`.
fn metric(value: AuctionValue, format: u8, l: Language) -> String {
    let text = match format {
        2 => format!("{}.{:01}%", value.amount / 10, value.amount % 10),
        3 => format!("{}.{:03}", value.amount / 1000, value.amount % 1000),
        _ => return l.number(value.amount),
    };
    if l == Language::German {
        text.replace('.', ",")
    } else {
        text
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::Bounds;
    use iced::advanced::{Layout, layout, widget::Tree};
    #[test]
    fn item_tooltips_stay_inside_all_viewport_edges_and_close_when_leaving() {
        use iced::advanced::{clipboard, mouse};
        let item = AuctionItem {
            id: "i:1".into(),
            name: Some(
                "SupercalifragilisticHandcraftedEnchantedStormforgedReinforcedRunicEquipment"
                    .into(),
            ),
            quality: Some(4),
            min_buyout: AuctionValue::new(u64::MAX, 1791031782),
            ..Default::default()
        };
        crate::ui::components::load_fonts();
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
        let viewport = iced::Rectangle::with_size(iced::Size::new(820.0, 620.0));
        let images = Images {
            handles: application::icons::COIN_TEXTURE_IDS
                .into_iter()
                .map(|id| {
                    (
                        id,
                        iced::widget::image::Handle::from_rgba(1, 1, vec![255u8; 4]),
                    )
                })
                .collect(),
            ..Default::default()
        };
        for (x, y) in [
            (0.0, 0.0),
            (780.0, 0.0),
            (0.0, 580.0),
            (780.0, 580.0),
            (300.0, 520.0),
        ] {
            let mut element: Element<'_, Message> = container(item::hover(
                space().width(40).height(40),
                &item,
                &images,
                Language::German,
            ))
            .padding(iced::Padding {
                top: y,
                left: x,
                ..Default::default()
            })
            .into();
            let mut tree = Tree::new(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(viewport.size(), viewport.size()),
            );
            let cursor = mouse::Cursor::Available(iced::Point::new(x + 20.0, y + 20.0));
            let mut messages = vec![];
            let mut update = |element: &mut Element<'_, Message>, tree: &mut Tree, cursor| {
                element.as_widget_mut().update(
                    tree,
                    &iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                    &viewport,
                );
            };
            update(&mut element, &mut tree, cursor);
            std::thread::sleep(std::time::Duration::from_millis(310));
            update(&mut element, &mut tree, cursor);
            {
                let mut overlay = element
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        iced::Vector::ZERO,
                    )
                    .unwrap();
                let node = overlay.as_overlay_mut().layout(&renderer, viewport.size());
                fn contained(layout: Layout<'_>) {
                    let rect = layout.bounds();
                    assert!(
                        rect.x >= 0.0
                            && rect.y >= 0.0
                            && rect.x + rect.width <= 820.1
                            && rect.y + rect.height <= 620.1,
                        "tooltip layout escaped viewport: {rect:?}"
                    );
                    for child in layout.children() {
                        contained(child);
                    }
                }
                assert!(
                    !node.children().is_empty(),
                    "tooltip must contain its preview"
                );
                contained(Layout::new(&node));
                fn full_coins(node: &layout::Node) -> usize {
                    usize::from(node.size() == iced::Size::new(16.0, 16.0))
                        + node.children().iter().map(full_coins).sum::<usize>()
                }
                assert!(
                    full_coins(&node) >= 3,
                    "maximum gold amount must not squeeze a coin texture"
                );
            }
            if x == 300.0 {
                let tooltip_bounds = |element: &mut Element<'_, Message>, tree: &mut Tree| {
                    let mut overlay = element
                        .as_widget_mut()
                        .overlay(
                            tree,
                            Layout::new(&node),
                            &renderer,
                            &viewport,
                            iced::Vector::ZERO,
                        )
                        .unwrap();
                    let node = overlay.as_overlay_mut().layout(&renderer, viewport.size());
                    let mut tooltip = &node;
                    while tooltip.bounds() == viewport && tooltip.children().len() == 1 {
                        tooltip = &tooltip.children()[0];
                    }
                    tooltip.bounds()
                };
                let before = tooltip_bounds(&mut element, &mut tree);
                let moved = mouse::Cursor::Available(iced::Point::new(x + 28.0, y + 20.0));
                update(&mut element, &mut tree, moved);
                let after = tooltip_bounds(&mut element, &mut tree);
                assert!(
                    (after.x - before.x - 8.0).abs() < 0.1,
                    "preview must follow cursor inside the same item"
                );
            }
            update(&mut element, &mut tree, mouse::Cursor::Unavailable);
            assert!(
                element
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        iced::Vector::ZERO
                    )
                    .is_none()
            );
        }
    }
    #[test]
    fn compact_long_names_variants_and_large_values_keep_their_columns() {
        let mut item = AuctionItem {
            id: format!("i:123:0:1:{}", "1234567890".repeat(12)),
            name: Some(
                "SupercalifragilisticHandcraftedEnchantedStormforgedReinforcedRunicEquipment"
                    .into(),
            ),
            class: Some(8),
            quality: Some(5),
            min_buyout: AuctionValue::new(u64::MAX, 1791031782),
            auctions: AuctionValue::new(u64::MAX, 1791031782),
            ..AuctionItem::default()
        };
        item.index();
        let market = application::Market {
            game: Game::Retail,
            scope: "Silvermoon".into(),
            region: Some("EU".into()),
            region_only: false,
        };
        let mut browser = Browser::default();
        browser.replace(
            Some(&Arc::new(application::AuctionCatalog {
                selected: Some(market.clone()),
                markets: vec![market],
                items: vec![item.clone()],
                ..application::AuctionCatalog::default()
            })),
            &[],
        );
        for language in [Language::German, Language::English] {
            for details in [false, true] {
                browser.dispatch(if details {
                    Browse::Select(item.id.clone())
                } else {
                    Browse::Back
                });
                crate::ui::components::load_fonts();
                let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
                let images = Images::default();
                let mut element = view(&browser, language, false, &images, None);
                let mut tree = Tree::new(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(
                        iced::Size::new(534.0, 0.0),
                        iced::Size::new(534.0, 4000.0),
                    ),
                );
                let mut bounds = Bounds::default();
                element.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut bounds,
                );
                for (text, rect) in &bounds.texts {
                    assert!(
                        rect.x >= 0.0 && rect.x + rect.width <= 534.1,
                        "{language:?}, details={details}, text={text}: {rect:?}"
                    );
                }
                if !details {
                    let rect = |value: &str| bounds.text(value);
                    let name = rect(item.name.as_ref().unwrap());
                    let money = rect(&language.number(u64::MAX / 10_000));
                    let count = rect(&language.number(u64::MAX));
                    assert!(name.x + name.width <= money.x && money.x + money.width <= count.x);
                    assert!(name.height > 30.0, "unbroken item name must wrap");
                }
            }
        }
    }
}
