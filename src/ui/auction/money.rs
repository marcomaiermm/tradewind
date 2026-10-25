//! Copper amounts rendered with the game's local MoneyFrame textures.
use super::*;

pub(in crate::ui) const COLORS: [iced::Color; 3] = [
    iced::color!(0xFFD70A),
    iced::color!(0xC7C7CF),
    iced::color!(0xEDA55F),
];

pub(super) fn view<'a>(
    amount: Option<u64>,
    images: &Images,
    l: Language,
    surface: identity::Surface,
) -> Element<'a, Message> {
    let Some(amount) = amount else {
        return text("-").size(15).into();
    };
    let (gold, silver, copper) = AuctionValue {
        amount,
        timestamp: 0,
    }
    .coins();
    let mut coins = row![].spacing(7).align_y(alignment::Vertical::Center);
    // Only the units that carry value, unpadded: 80s, 2g, 1g 5c. Zero still reads 0c.
    for (index, (value, unit)) in [
        (gold, Key::AuctionGold),
        (silver, Key::AuctionSilver),
        (copper, Key::AuctionCopper),
    ]
    .into_iter()
    .enumerate()
    .filter(|&(index, (value, _))| value > 0 || (index == 2 && amount == 0))
    {
        let label = l.number(value);
        let symbol: Element<'a, Message> = match images
            .handles
            .get(&application::icons::COIN_TEXTURE_IDS[index])
        {
            Some(handle) => iced::widget::image(handle.clone())
                .width(16)
                .height(16)
                .content_fit(iced::ContentFit::Contain)
                .into(),
            None => text(l.text(unit)).size(13).into(),
        };
        coins = coins.push(
            container(
                row![text(label).size(15).wrapping(Wrapping::WordOrGlyph), symbol]
                    .spacing(2)
                    .align_y(alignment::Vertical::Center),
            )
            .style(move |theme: &Theme| container::Style {
                text_color: Some(identity::readable_color(
                    surface.color(theme),
                    COLORS[index],
                )),
                ..Default::default()
            }),
        );
    }
    let value: Element<'a, Message> = coins.wrap().vertical_spacing(2).into();
    // Tooltip contents never nest another tooltip.
    if matches!(surface, identity::Surface::Fixed(_)) {
        value
    } else {
        tooltip(
            value,
            text(
                msg!(
                    AuctionMoneyDescription,
                    gold = l.number(gold),
                    silver = silver,
                    copper = copper
                )
                .render(l),
            )
            .size(13),
            Position::Top,
        )
        .style(popover)
        .into()
    }
}
