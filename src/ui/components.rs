//! Shared UI components: each constructor returns a pre-styled Iced widget, and
//! variants are plain style functions passed to `.style(...)`, e.g.
//! `action("Save", msg).style(outline)`. Launcher grammar: uppercase section
//! labels, tab navigation with an underline, one blue action per page.
use super::Message;
use super::theme::{RADIUS, tokens};
use iced::widget::{
    Button, Checkbox, Container, PickList, Scrollable, Text, TextInput, button, checkbox, column,
    container, overlay::menu, pick_list, scrollable, space, text, text_input,
};
use iced::{Background, Border, Color, Element, Font, Length, Shadow, Theme, Vector, font};
use std::borrow::Borrow;

fn border(color: Color) -> Border {
    Border {
        color,
        width: 1.0,
        radius: RADIUS.into(),
    }
}

pub(super) fn caption<'a>(label: impl text::IntoFragment<'a>) -> Text<'a> {
    text(label).size(13).style(|theme: &Theme| text::Style {
        color: Some(tokens(theme).muted_foreground),
    })
}
pub(super) fn heading<'a>(label: impl text::IntoFragment<'a>) -> Text<'a> {
    text(label).size(20).font(BOLD)
}
pub(super) const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};
/// Registers the bundled Open Sans (OFL-1.1, see `data/fonts`) as Iced's default
/// sans-serif. Layouts are tuned to its metrics, so it replaces any system copy,
/// whose version may differ. Call before the first renderer is created.
pub(super) fn load_fonts() {
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(|| {
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system lock");
        let db = fonts.raw().db_mut();
        let system: Vec<_> = db
            .faces()
            .filter(|face| face.families.iter().any(|(name, _)| name == "Open Sans"))
            .map(|face| face.id)
            .collect();
        for id in system {
            db.remove_face(id);
        }
        fonts.load_font(include_bytes!("../../data/fonts/OpenSans-Regular.ttf").into());
        fonts.load_font(include_bytes!("../../data/fonts/OpenSans-Bold.ttf").into());
    });
}
/// Launcher section label: small bold capitals carry rank instead of size.
pub(super) fn label<'a>(value: &str) -> Text<'a> {
    text(value.to_uppercase())
        .size(12)
        .font(BOLD)
        .style(|theme: &Theme| text::Style {
            color: Some(tokens(theme).muted_foreground),
        })
}

/// Top navigation entry: capitals, and a blue underline marks the current page.
pub(super) fn tab<'a>(title: &str, active: bool, message: Message) -> Element<'a, Message> {
    let underline = container(space().height(2))
        .width(Length::Fill)
        .style(move |theme: &Theme| container::Style {
            background: active.then(|| tokens(theme).ring.into()),
            ..Default::default()
        });
    button(
        column![text(title.to_uppercase()).size(13).font(BOLD), underline]
            .spacing(6)
            .width(Length::Shrink),
    )
    .padding(iced::Padding {
        top: 8.0,
        ..iced::Padding::ZERO
    })
    .on_press(message)
    .style(move |theme: &Theme, status| {
        let t = tokens(theme);
        button::Style {
            text_color: if active || status == button::Status::Hovered {
                t.foreground
            } else {
                t.muted_foreground
            },
            ..Default::default()
        }
    })
    .into()
}

pub(super) fn action<'a>(label: &'a str, message: Option<Message>) -> Button<'a, Message> {
    button(text(label).size(14))
        .padding([9, 16])
        .on_press_maybe(message)
        .style(primary)
}

fn filled(
    theme: &Theme,
    status: button::Status,
    background: Color,
    foreground: Color,
) -> button::Style {
    let t = tokens(theme);
    match status {
        button::Status::Disabled => button::Style {
            background: Some(t.muted.into()),
            text_color: t.muted_foreground,
            border: border(Color::TRANSPARENT),
            ..Default::default()
        },
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(background.scale_alpha(0.88).into()),
            text_color: foreground,
            border: border(Color::TRANSPARENT),
            ..Default::default()
        },
        button::Status::Active => button::Style {
            background: Some(background.into()),
            text_color: foreground,
            border: border(Color::TRANSPARENT),
            ..Default::default()
        },
    }
}
pub(super) fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    filled(theme, status, t.primary, t.primary_foreground)
}
pub(super) fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    filled(theme, status, t.secondary, t.foreground)
}
pub(super) fn destructive(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    filled(theme, status, t.destructive, t.destructive_foreground)
}
pub(super) fn outline(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    button::Style {
        border: border(if status == button::Status::Disabled {
            t.border
        } else {
            t.input
        }),
        ..ghost(theme, status)
    }
}
pub(super) fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| t.accent.into()),
        text_color: if status == button::Status::Disabled {
            t.muted_foreground
        } else {
            t.foreground
        },
        border: border(Color::TRANSPARENT),
        ..Default::default()
    }
}

pub(super) fn input<'a>(placeholder: &str, value: &str) -> TextInput<'a, Message> {
    text_input(placeholder, value)
        .padding([9, 12])
        .size(14)
        .style(input_style)
}
pub(super) fn input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let t = tokens(theme);
    text_input::Style {
        background: t.input_background.into(),
        border: border(match status {
            text_input::Status::Focused { .. } => t.ring,
            text_input::Status::Hovered => t.ring.scale_alpha(0.6),
            _ => t.input,
        }),
        icon: t.muted_foreground,
        placeholder: t.muted_foreground,
        value: if status == text_input::Status::Disabled {
            t.muted_foreground
        } else {
            t.foreground
        },
        selection: t.ring.scale_alpha(0.4),
    }
}

pub(super) fn select<'a, T, L, V>(
    options: L,
    selected: Option<V>,
    on_select: impl Fn(T) -> Message + 'a,
) -> PickList<'a, T, L, V, Message>
where
    T: ToString + PartialEq + Clone + 'a,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
{
    pick_list(options, selected, on_select)
        .padding([9, 12])
        .text_size(14)
        .style(|theme: &Theme, status| {
            let t = tokens(theme);
            pick_list::Style {
                text_color: t.foreground,
                placeholder_color: t.muted_foreground,
                handle_color: t.muted_foreground,
                background: t.input_background.into(),
                border: border(match status {
                    pick_list::Status::Active => t.input,
                    _ => t.ring,
                }),
            }
        })
        .menu_style(|theme: &Theme| {
            let t = tokens(theme);
            menu::Style {
                background: t.card.into(),
                border: border(t.border),
                text_color: t.foreground,
                selected_text_color: t.foreground,
                selected_background: t.accent.into(),
                shadow: Shadow {
                    color: Color::BLACK.scale_alpha(0.25),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
            }
        })
}

pub(super) fn check<'a>(checked: bool) -> Checkbox<'a, Message> {
    checkbox(checked)
        .size(16)
        .text_size(14)
        .style(|theme: &Theme, status| {
            let t = tokens(theme);
            let (checked, disabled) = match status {
                checkbox::Status::Active { is_checked } => (is_checked, false),
                checkbox::Status::Hovered { is_checked } => (is_checked, false),
                checkbox::Status::Disabled { is_checked } => (is_checked, true),
            };
            let style = checkbox::Style {
                background: if checked {
                    t.primary
                } else {
                    t.input_background
                }
                .into(),
                icon_color: t.primary_foreground,
                border: Border {
                    radius: 4.0.into(),
                    ..border(if checked { t.primary } else { t.input })
                },
                text_color: Some(t.foreground),
            };
            if disabled {
                checkbox::Style {
                    background: style.background.scale_alpha(0.5),
                    text_color: Some(t.muted_foreground),
                    ..style
                }
            } else {
                style
            }
        })
}

// Segmented toggle: one bordered group whose active option is filled.

pub(super) fn segmented<'a>(options: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(options).padding(2).style(|theme: &Theme| {
        let t = tokens(theme);
        container::Style {
            background: Some(t.input_background.into()),
            border: border(t.input),
            ..Default::default()
        }
    })
}
pub(super) fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = tokens(theme);
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: (selected || hovered)
                .then(|| if selected { t.secondary } else { t.accent }.into()),
            text_color: if selected || hovered {
                t.foreground
            } else {
                t.muted_foreground
            },
            border: border(if selected {
                t.input
            } else {
                Color::TRANSPARENT
            }),
            ..Default::default()
        }
    }
}
/// Small identity swatch beside a label. The label carries the meaning.
pub(super) fn dot<'a>(color: Color) -> Element<'a, Message> {
    container(space())
        .width(8)
        .height(8)
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
/// Vertical scroll body. The scrollbar sits beside the content with a fixed gap, never over it.
pub(super) fn scroll<'a>(content: impl Into<Element<'a, Message>>) -> Scrollable<'a, Message> {
    scrollable(content).direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new().spacing(10),
    ))
}
pub(super) fn card<'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .padding(20)
        .width(Length::Fill)
        .style(surface)
}
pub(super) fn surface(theme: &Theme) -> container::Style {
    let t = tokens(theme);
    container::Style {
        background: Some(t.card.into()),
        text_color: Some(t.foreground),
        border: border(t.border),
        ..Default::default()
    }
}
pub(super) fn chrome(theme: &Theme) -> container::Style {
    let t = tokens(theme);
    container::Style {
        background: Some(t.chrome.into()),
        text_color: Some(t.foreground),
        ..Default::default()
    }
}
pub(super) fn popover(theme: &Theme) -> container::Style {
    container::Style {
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.25),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..surface(theme)
    }
}
pub(super) fn alert(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens(theme).background)),
        ..surface(theme)
    }
}
pub(super) fn destructive_alert(theme: &Theme) -> container::Style {
    let t = tokens(theme);
    container::Style {
        border: border(t.destructive),
        ..alert(theme)
    }
}
