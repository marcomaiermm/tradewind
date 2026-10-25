//! Version accents follow Blizzard's current artwork, faction seeds follow WoW UI constants.
//! Sources and the distinction between official values and UI choices: docs/architecture.md#colors.
use super::*;
use iced::Color;

pub(super) fn game_color(game: Game) -> Color {
    match game {
        Game::Retail => iced::color!(0x7B5BC8),
        Game::Progression => iced::color!(0x3D9B78),
        Game::ClassicEra => iced::color!(0xC69B57),
        Game::Anniversary => iced::color!(0x8BAF3C),
        Game::Forever => iced::color!(0x016787),
    }
}
pub(super) fn faction_label(faction: Faction, l: Language) -> &'static str {
    match faction {
        Faction::Alliance => l.text(Key::Alliance),
        Faction::Horde => l.text(Key::Horde),
    }
}
pub(super) fn faction_color(faction: Faction) -> Color {
    match faction {
        Faction::Alliance => iced::color!(0x4954E8),
        Faction::Horde => iced::color!(0xE50C11),
    }
}

pub(super) fn mix(background: Color, foreground: Color, amount: f32) -> Color {
    Color::from_rgb(
        background.r + (foreground.r - background.r) * amount,
        background.g + (foreground.g - background.g) * amount,
        background.b + (foreground.b - background.b) * amount,
    )
}
fn luminance(color: Color) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}
pub(super) fn contrast(left: Color, right: Color) -> f32 {
    let a = luminance(left);
    let b = luminance(right);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn tones(theme: &Theme, seed: Color, selected: bool) -> (Color, Color) {
    let background = mix(
        theme.palette().background,
        seed,
        if selected { 0.22 } else { 0.10 },
    );
    (background, readable_color(background, seed))
}
pub(super) fn readable_color(background: Color, seed: Color) -> Color {
    let target = if luminance(background) < 0.179 {
        Color::WHITE
    } else {
        Color::BLACK
    };
    (0..=20)
        .map(|i| mix(seed, target, i as f32 / 20.0))
        .find(|color| contrast(*color, background) >= 4.5)
        .unwrap_or(target)
}
/// Tint for views without a game version: the launcher accent.
pub(super) const NEUTRAL_SEED: Color = iced::color!(0x148eff);
/// What text sits on, resolved per theme so contrast checks follow light/dark.
#[derive(Clone, Copy, Debug)]
pub(super) enum Surface {
    Page,
    Fixed(Color),
    Hero(Color),
    Tile(Color),
}
impl Surface {
    pub(super) fn color(self, theme: &Theme) -> Color {
        match self {
            Self::Page => theme.palette().background,
            Self::Fixed(color) => color,
            Self::Hero(seed) => page_tint(theme, seed).0,
            Self::Tile(seed) => page_tint(theme, seed).2,
        }
    }
}
/// Launcher game page: the edition colour washes the surface, strongest at the top.
/// Returns (top, bottom, tile) so text can be checked against every stop.
pub(super) fn page_tint(theme: &Theme, seed: Color) -> (Color, Color, Color) {
    let t = theme::tokens(theme);
    let mut strength: f32 = if theme.extended_palette().is_dark {
        0.24
    } else {
        0.16
    };
    // Bright seeds (Common white in dark mode) back off until muted text still reads.
    while strength > 0.0 && contrast(t.muted_foreground, mix(t.card, seed, strength)) < 4.5 {
        strength -= 0.02;
    }
    let strength = strength.max(0.0);
    (
        mix(t.card, seed, strength),
        mix(t.card, seed, strength / 4.0),
        // Half-muted base: tiles stay visible even when the seed matches the card (Common in light).
        mix(mix(t.card, t.muted, 0.5), seed, strength * 0.55),
    )
}
pub(super) fn tinted_page(seed: Color) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let (top, bottom, _) = page_tint(theme, seed);
        container::Style {
            background: Some(
                iced::gradient::Linear::new(std::f32::consts::PI)
                    .add_stop(0.0, top)
                    .add_stop(0.45, bottom)
                    .into(),
            ),
            text_color: Some(theme::tokens(theme).foreground),
            border: iced::Border {
                radius: theme::RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
pub(super) fn tile(seed: Color) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(page_tint(theme, seed).2.into()),
        border: iced::Border {
            radius: theme::RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub(super) fn badge<'a>(label: impl Into<String>, color: Color) -> Element<'a, Message> {
    container(text(label.into()).size(12))
        .padding([3, 7])
        .style(move |theme: &Theme| {
            let (background, foreground) = tones(theme, color, false);
            container::Style {
                background: Some(background.into()),
                text_color: Some(foreground),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}
pub(super) fn version_badge<'a>(game: Game) -> Element<'a, Message> {
    badge(game.label(), game_color(game))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn game_page_tints_keep_text_readable_for_every_quality() {
        use crate::ui::theme::{dark, light, tokens};
        for theme in [dark(), light()] {
            let t = tokens(&theme);
            for edition in Game::ALL
                .map(game_color)
                .into_iter()
                .chain((0..=8).map(crate::ui::auction::item::quality_color))
                .chain([NEUTRAL_SEED])
            {
                let (top, bottom, tile) = page_tint(&theme, edition);
                for surface in [top, bottom, tile] {
                    for color in [t.foreground, t.muted_foreground] {
                        assert!(
                            contrast(color, surface) >= 4.5,
                            "{edition:?} {color:?} on {surface:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn item_quality_and_money_text_are_readable_on_light_dark_and_tooltip_surfaces() {
        for background in [
            super::super::theme::light().palette().background,
            super::super::theme::dark().palette().background,
            iced::color!(0x090B10),
        ] {
            for seed in (0..=8)
                .map(super::super::auction::item::quality_color)
                .chain(super::super::auction::money::COLORS)
            {
                assert!(contrast(background, readable_color(background, seed)) >= 4.5);
            }
        }
    }
    #[test]
    fn version_and_faction_labels_have_readable_contrast_in_both_themes() {
        let colors = Game::ALL.map(game_color).into_iter().chain([
            faction_color(Faction::Alliance),
            faction_color(Faction::Horde),
        ]);
        for theme in [super::super::theme::light(), super::super::theme::dark()] {
            for color in colors.clone() {
                for selected in [false, true] {
                    let (background, foreground) = tones(&theme, color, selected);
                    assert!(contrast(background, foreground) >= 4.5);
                }
            }
        }
    }
    #[test]
    fn destructive_text_is_readable_on_the_page_background() {
        for theme in [super::super::theme::light(), super::super::theme::dark()] {
            let t = super::super::theme::tokens(&theme);
            assert!(
                contrast(t.destructive_text, t.background) >= 4.5,
                "{theme:?}"
            );
        }
    }
    #[test]
    fn input_placeholders_remain_readable_in_both_themes_and_all_states() {
        for theme in [super::super::theme::light(), super::super::theme::dark()] {
            for status in [
                iced::widget::text_input::Status::Active,
                iced::widget::text_input::Status::Hovered,
                iced::widget::text_input::Status::Focused { is_hovered: false },
                iced::widget::text_input::Status::Disabled,
            ] {
                let style = input_style(&theme, status);
                let iced::Background::Color(background) = style.background else {
                    panic!("expected solid native input background");
                };
                let painted = mix(background, style.placeholder, style.placeholder.a);
                assert!(contrast(background, painted) >= 4.5, "{theme:?} {status:?}");
            }
        }
    }
}
