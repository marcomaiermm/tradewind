//! Item identity and cached facts, presented consistently in rows, details and hover previews.
use super::*;
use iced::widget::column;

const TOOLTIP_BACKGROUND: iced::Color = iced::color!(0x090B10);
const TOOLTIP_TEXT: iced::Color = iced::color!(0xEAE7DF);
const TOOLTIP_MUTED: iced::Color = iced::color!(0xB5B8C2);

pub(in crate::ui) fn quality_color(quality: u8) -> iced::Color {
    match quality {
        0 => iced::color!(0x9D9D9D),
        1 => iced::Color::WHITE,
        2 => iced::color!(0x1EFF00),
        3 => iced::color!(0x0070DD),
        4 => iced::color!(0xA335EE),
        5 => iced::color!(0xFF8000),
        6 => iced::color!(0xE6CC80),
        7 | 8 => iced::color!(0x00CCFF),
        _ => iced::color!(0x9D9D9D),
    }
}
pub(super) fn title<'a>(
    item: &AuctionItem,
    l: Language,
    size: u32,
    surface: identity::Surface,
) -> iced::widget::Text<'a> {
    let quality = item.quality;
    text(name(item, l))
        .size(size)
        .width(Length::Fill)
        .wrapping(Wrapping::WordOrGlyph)
        .style(move |theme: &Theme| iced::widget::text::Style {
            // Common is WoW's white: plain foreground, so light mode keeps full contrast.
            color: quality
                .filter(|&q| q != 1)
                .map(|q| identity::readable_color(surface.color(theme), quality_color(q))),
        })
}
pub(super) fn icon<'a>(item: &AuctionItem, images: &Images, size: f32) -> Element<'a, Message> {
    let content: Element<'a, Message> = item
        .texture_id
        .and_then(|id| images.handles.get(&id))
        .map_or_else(
            || space().width(Length::Fill).height(Length::Fill).into(),
            |handle| {
                iced::widget::image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(iced::ContentFit::Contain)
                    .into()
            },
        );
    let quality = item.quality;
    container(content)
        .padding(2)
        .width(size)
        .height(size)
        .style(move |theme: &Theme| container::Style {
            background: Some(TOOLTIP_BACKGROUND.into()),
            border: iced::Border {
                color: quality.map_or(
                    theme.extended_palette().background.strong.color,
                    quality_color,
                ),
                width: 2.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn facts<'a>(
    item: &AuctionItem,
    images: &Images,
    l: Language,
    surface: identity::Surface,
) -> Element<'a, Message> {
    let mut facts = column![].spacing(4).width(Length::Fill);
    if let Some(details) = &item.details {
        if let Some(level) = details.level {
            facts = facts.push(
                text(msg!(AuctionItemLevel, level = u64::from(level)).render(l))
                    .size(14)
                    .style(move |theme: &Theme| iced::widget::text::Style {
                        color: Some(identity::readable_color(
                            surface.color(theme),
                            money::COLORS[0],
                        )),
                    }),
            );
        }
        if details.binds_on_pickup {
            facts = facts.push(text(l.text(Key::AuctionBindsOnPickup)).size(14));
        }
        if details.crafting_reagent {
            facts = facts.push(text(l.text(Key::AuctionCraftingReagent)).size(14));
        }
        if let Some(level) = details.required_level {
            facts = facts.push(
                text(msg!(AuctionRequiredLevel, level = u64::from(level)).render(l)).size(14),
            );
        }
        if let Some(count) = details.stack_size {
            facts = facts
                .push(text(msg!(AuctionStackSize, count = u64::from(count)).render(l)).size(14));
        }
        if let Some(price) = details.vendor_price {
            facts = facts.push(
                row![
                    text(l.text(Key::AuctionVendorPrice))
                        .size(14)
                        .width(Length::Fill)
                        .wrapping(Wrapping::WordOrGlyph),
                    money::view(Some(price as u64), images, l, surface),
                ]
                .spacing(10)
                .align_y(alignment::Vertical::Center),
            );
        }
    }
    facts.into()
}

fn preview<'a>(item: &AuctionItem, images: &Images, l: Language) -> Element<'a, Message> {
    let mut content = column![title(
        item,
        l,
        17,
        identity::Surface::Fixed(TOOLTIP_BACKGROUND)
    )]
    .spacing(10)
    .width(340);
    let labels: Vec<_> = [
        item.quality.map(|q| quality(q, l)),
        item.class.map(|c| category(c, l)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !labels.is_empty() {
        content = content.push(text(labels.join(" · ")).size(13).color(TOOLTIP_MUTED));
    }
    content = content.push(demand::preview(item, l, TOOLTIP_MUTED));
    content = content.push(facts(
        item,
        images,
        l,
        identity::Surface::Fixed(TOOLTIP_BACKGROUND),
    ));
    content = content.push(rule::horizontal(1).style(|_| iced::widget::rule::Style {
        color: iced::color!(0x383C48),
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }));
    for (label, value) in [
        (Key::AuctionMin, item.min_buyout),
        (Key::AuctionMarketValue, item.market),
    ] {
        let mut metric = column![
            row![
                text(l.text(label))
                    .size(14)
                    .width(Length::Fill)
                    .wrapping(Wrapping::WordOrGlyph),
                container(money::view(
                    value.map(|v| v.amount),
                    images,
                    l,
                    identity::Surface::Fixed(TOOLTIP_BACKGROUND)
                ))
                .width(175),
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center)
        ]
        .spacing(3);
        if let Some(value) = value {
            metric = metric.push(
                text(msg!(AuctionAsOf, time = l.timestamp(value.timestamp, true)).render(l))
                    .size(12)
                    .color(TOOLTIP_MUTED),
            );
        }
        content = content.push(metric);
    }
    if item
        .texture_id
        .is_none_or(|id| !images.handles.contains_key(&id))
    {
        content = content.push(
            text(
                l.text(if images.state.loading() && item.texture_id.is_some() {
                    Key::AuctionIconsLoading
                } else {
                    Key::AuctionIconMissing
                }),
            )
            .size(12)
            .color(TOOLTIP_MUTED),
        );
    }
    content
        .push(
            text(l.text(Key::AuctionTooltipLimit))
                .size(12)
                .color(TOOLTIP_MUTED)
                .width(Length::Fill)
                .wrapping(Wrapping::WordOrGlyph),
        )
        .into()
}

pub(super) fn hover<'a>(
    content: impl Into<Element<'a, Message>>,
    item: &AuctionItem,
    images: &Images,
    l: Language,
) -> Element<'a, Message> {
    let border = item.quality.map_or(TOOLTIP_MUTED, quality_color);
    tooltip(content, preview(item, images, l), Position::FollowCursor)
        .delay(std::time::Duration::from_millis(300))
        .padding(14)
        .style(move |_| container::Style {
            background: Some(TOOLTIP_BACKGROUND.into()),
            text_color: Some(TOOLTIP_TEXT),
            border: iced::Border {
                color: border,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::identity::contrast;
    use crate::ui::theme::{dark, light, tokens};

    #[test]
    fn tooltips_meet_wcag_contrast_in_both_themes() {
        // The item preview is a fixed dark card, so its text needs 4.5:1 (WCAG 1.4.3).
        for color in [TOOLTIP_TEXT, TOOLTIP_MUTED] {
            assert!(contrast(color, TOOLTIP_BACKGROUND) >= 4.5, "{color:?}");
        }
        for theme in [light(), dark()] {
            let t = tokens(&theme);
            // Its edge must stand out from the page by 3:1 (1.4.11): the fill on
            // light pages, the quality border on dark ones.
            let edge = |border| {
                contrast(TOOLTIP_BACKGROUND, t.background).max(contrast(border, t.background))
            };
            for border in (0..=8).map(quality_color).chain([TOOLTIP_MUTED]) {
                assert!(edge(border) >= 3.0, "{border:?} on {theme:?}");
            }
            // Themed popovers (sort headers, prices).
            for color in [t.foreground, t.muted_foreground] {
                assert!(contrast(color, t.card) >= 4.5, "{color:?} on {theme:?}");
            }
        }
    }
}
