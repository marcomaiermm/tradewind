//! Design tokens: Battle.net launcher neutrals with one launcher-blue action colour,
//! mapped onto Iced themes (values measured from the Battle.net launcher).
//!
//! Components read tokens through [`tokens`]. Iced's own widgets (scrollbars,
//! rules, menus) pick them up through the generated palette.
use iced::theme::{Palette, palette};
use iced::{Color, Theme, color};
use std::sync::LazyLock;

/// Corner radius shared by controls and surfaces.
pub(super) const RADIUS: f32 = 4.0;

pub(super) struct Tokens {
    pub background: Color,
    pub foreground: Color,
    pub card: Color,
    pub chrome: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub accent: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub secondary: Color,
    pub destructive: Color,
    /// Destructive hue for text and dots on the page background (the fill is too dark there).
    pub destructive_text: Color,
    pub destructive_foreground: Color,
    pub border: Color,
    pub input: Color,
    pub input_background: Color,
    pub ring: Color,
    pub success: Color,
}

const DARK: Tokens = Tokens {
    background: color!(0x15171e),
    foreground: color!(0xf2f3f5),
    card: color!(0x1b1d23),
    chrome: color!(0x15171e),
    muted: color!(0x22252d),
    muted_foreground: color!(0xa9adb7),
    accent: color!(0x272b33),
    primary: color!(0x0b63c4),
    primary_foreground: color!(0xffffff),
    secondary: color!(0x272b33),
    destructive: color!(0xd23c3c),
    destructive_text: color!(0xef5f5f),
    destructive_foreground: color!(0xffffff),
    border: color!(0x2a2e37),
    input: color!(0x3a3f4a),
    input_background: color!(0x111318),
    ring: color!(0x148eff),
    success: color!(0x8fd16a),
};

const LIGHT: Tokens = Tokens {
    background: color!(0xeef0f3),
    foreground: color!(0x15171e),
    card: color!(0xffffff),
    chrome: color!(0xffffff),
    muted: color!(0xe4e7ec),
    muted_foreground: color!(0x525866),
    accent: color!(0xe4e7ec),
    primary: color!(0x0b63c4),
    primary_foreground: color!(0xffffff),
    secondary: color!(0xe4e7ec),
    destructive: color!(0xc62828),
    destructive_text: color!(0xc62828),
    destructive_foreground: color!(0xffffff),
    border: color!(0xd5d9e0),
    input: color!(0x9aa1ad),
    input_background: color!(0xffffff),
    ring: color!(0x0b63c4),
    success: color!(0x2e7d32),
};

pub(super) fn tokens(theme: &Theme) -> &'static Tokens {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}

fn build(name: &'static str, t: &'static Tokens) -> Theme {
    let palette = Palette {
        background: t.background,
        text: t.foreground,
        primary: t.primary,
        success: t.success,
        warning: color!(0xd97706),
        danger: t.destructive,
    };
    Theme::custom_with_fn(name, palette, |palette| {
        let mut extended = palette::Extended::generate(palette);
        extended.background.weak = palette::Pair::new(t.muted, t.foreground);
        extended.background.strong = palette::Pair::new(t.border, t.foreground);
        extended
    })
}

static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| build("TSM Dark", &DARK));
static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| build("TSM Light", &LIGHT));

pub(super) fn dark() -> Theme {
    DARK_THEME.clone()
}
pub(super) fn light() -> Theme {
    LIGHT_THEME.clone()
}
