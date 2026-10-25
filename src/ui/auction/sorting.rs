use super::*;

pub(super) const QUALITY_WIDTH: u32 = 96;
pub(super) const PRICE_WIDTH: u32 = 175;
pub(super) const COUNT_WIDTH: u32 = 80;

pub(super) fn arrow(direction: Option<SortDirection>) -> iced::widget::Image {
    static UP: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    static DOWN: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    static NEUTRAL: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    let (cache, bytes): (_, &[u8]) = match direction {
        None => (
            &NEUTRAL,
            include_bytes!("../../../data/icons/sort-neutral.rgba"),
        ),
        Some(SortDirection::Ascending) => (
            &UP,
            include_bytes!("../../../data/icons/sort-ascending.rgba"),
        ),
        Some(SortDirection::Descending) => (
            &DOWN,
            include_bytes!("../../../data/icons/sort-descending.rgba"),
        ),
    };
    iced::widget::image(
        cache
            .get_or_init(|| iced::widget::image::Handle::from_rgba(32, 32, bytes.to_vec()))
            .clone(),
    )
    .width(12)
    .height(12)
}
pub(super) fn header(
    label: &'static str,
    field: SortField,
    sort: AuctionSort,
    width: Length,
    l: Language,
) -> Element<'static, Message> {
    let direction = match sort {
        AuctionSort::By(current, direction) if current == field => Some(direction),
        _ => None,
    };
    let content = row![text(label).size(13).width(Length::Fill), arrow(direction)]
        .spacing(4)
        .align_y(alignment::Vertical::Center);
    let button = button(content)
        .padding([6, 0])
        .width(width)
        .on_press(message(Browse::SortColumn(field)))
        .style(move |theme: &Theme, status| {
            // Table heads are muted, the sorted column reads at full strength.
            let mut style = ghost(theme, status);
            if direction.is_none() {
                style.text_color = super::super::theme::tokens(theme).muted_foreground;
            }
            style
        });
    tooltip(
        button,
        text(
            msg!(
                AuctionSortHint,
                order = sort_label(sort.toggle_column(field), l)
            )
            .render(l),
        )
        .size(13),
        Position::Top,
    )
    .delay(Duration::from_millis(600))
    .style(popover)
    .into()
}
pub(super) fn quality_cell(item: &AuctionItem, l: Language) -> iced::widget::Text<'static> {
    let value = item.quality.filter(|q| *q <= 8);
    text(value.map_or(l.text(Key::Unknown), |q| quality(q, l)))
        .size(13)
        .width(QUALITY_WIDTH)
        .wrapping(Wrapping::WordOrGlyph)
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: value.map(|q| {
                identity::readable_color(theme.palette().background, item::quality_color(q))
            }),
        })
}
