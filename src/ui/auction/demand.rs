//! Present the core's snapshot assessment without making pricing decisions in the UI.
use super::*;
use application::AuctionDemand;
use iced::widget::column;

fn label(item: &AuctionItem, l: Language) -> &'static str {
    l.text(match item.demand() {
        AuctionDemand::Unknown => Key::AuctionDemandUnknown,
        AuctionDemand::Low => Key::AuctionDemandLow,
        AuctionDemand::Active => Key::AuctionDemandActive,
        AuctionDemand::High => Key::AuctionDemandHigh,
    })
}
/// WoW's recipe-difficulty ladder: orange, yellow, green, grey. The word beside it carries the meaning.
fn color(item: &AuctionItem) -> iced::Color {
    match item.demand() {
        AuctionDemand::High => iced::color!(0xff9a4d),
        AuctionDemand::Active => iced::color!(0xffd24a),
        AuctionDemand::Low => iced::color!(0x5fd35f),
        AuctionDemand::Unknown => iced::color!(0x9d9d9d),
    }
}
fn flame() -> iced::widget::Image {
    static IMAGE: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    let handle = IMAGE.get_or_init(|| {
        iced::widget::image::Handle::from_rgba(
            64,
            64,
            include_bytes!("../../../data/icons/hot-deal.rgba").to_vec(),
        )
    });
    iced::widget::image(handle.clone()).width(18).height(18)
}
/// `long` spells out the discount ("60% below market value"), rows keep the short "−60%".
pub(super) fn summary(item: &AuctionItem, l: Language, long: bool) -> Element<'static, Message> {
    let mut content = row![
        dot(color(item)),
        caption(label(item, l))
            .size(12)
            .wrapping(Wrapping::WordOrGlyph)
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill);
    if let Some(discount) = item.hot_deal() {
        content = content.push(
            row![
                flame(),
                caption(if long {
                    msg!(AuctionDealDiscount, discount = u64::from(discount)).render(l)
                } else {
                    format!("−{discount}%")
                })
                .size(12)
            ]
            .spacing(3)
            .align_y(alignment::Vertical::Center),
        );
    }
    content.wrap().into()
}
pub(super) fn preview(
    item: &AuctionItem,
    l: Language,
    color: iced::Color,
) -> Element<'static, Message> {
    let mut content = column![text(label(item, l)).size(13).color(color)]
        .spacing(4)
        .width(Length::Fill);
    if let Some(discount) = item.hot_deal() {
        content = content.push(
            row![
                flame(),
                text(msg!(AuctionDealDiscount, discount = u64::from(discount)).render(l))
                    .size(13)
                    .color(color)
                    .width(Length::Fill)
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
        );
    }
    if item.demand() == AuctionDemand::Unknown {
        content = content.push(
            text(l.text(Key::AuctionDemandMissing))
                .size(12)
                .color(color)
                .width(Length::Fill),
        );
    } else if let (Some(rate), Some(sold)) = (item.sale_rate, item.sold_per_day) {
        let timestamp = rate.timestamp.min(sold.timestamp);
        let separator = if l == Language::German { ',' } else { '.' };
        let rate = format!("{}{separator}{}%", rate.amount / 10, rate.amount % 10);
        let sold = format!(
            "{}{separator}{:03}",
            l.number(sold.amount / 1000),
            sold.amount % 1000
        );
        let sold = sold.trim_end_matches('0').trim_end_matches(separator);
        content = content.push(
            text(msg!(AuctionDemandNumbers, rate = rate, sold = sold).render(l))
                .size(12)
                .color(color)
                .width(Length::Fill),
        );
        content = content.push(
            text(msg!(AuctionAsOf, time = l.timestamp(timestamp, true)).render(l))
                .size(12)
                .color(color),
        );
    }
    content
        .push(
            text(l.text(Key::AuctionDemandScope))
                .size(12)
                .color(color)
                .width(Length::Fill),
        )
        .into()
}
