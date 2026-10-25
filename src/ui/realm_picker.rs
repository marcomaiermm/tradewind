use super::*;
use iced::widget::column;
use std::collections::BTreeSet;

/// Matches shown inline. The page scrolls, so more rows ask for a narrower search instead.
const LIMIT: usize = 6;
pub(super) const SEARCH_ID: &str = "realm-search";

pub(super) struct State {
    pub query: String,
    pub filters: RealmFilters,
}
impl State {
    pub fn new(filters: RealmFilters) -> Self {
        Self {
            query: String::new(),
            filters,
        }
    }
    pub fn visible<'a>(&self, realms: &'a [Realm], l: Language) -> Vec<&'a Realm> {
        let query = self.query.trim().to_lowercase();
        realms
            .iter()
            .filter(|realm| {
                let faction = realm_name_and_faction(realm.game, &realm.name).1;
                // `realm` already displays the version label.
                let searchable = format!(
                    "{realm} {}",
                    faction
                        .map(|f| identity::faction_label(f, l))
                        .unwrap_or_default()
                )
                .to_lowercase();
                self.filters.matches(realm)
                    && query
                        .split_whitespace()
                        .all(|part| searchable.contains(part))
            })
            .collect()
    }
}

fn chip<'a>(
    label: &str,
    color: Option<iced::Color>,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    let mut content = row![].spacing(6).align_y(alignment::Vertical::Center);
    if let Some(color) = color {
        content = content.push(dot(color));
    }
    button(content.push(text(label.to_owned()).size(14)))
        .padding([6, 12])
        .on_press(message)
        .style(segment(selected))
        .into()
}

pub(super) fn view<'a>(
    state: &'a State,
    snapshot: &Snapshot,
    busy: bool,
    adding: Option<&Realm>,
    l: Language,
) -> Element<'a, Message> {
    let mut games = row![chip(
        l.text(Key::AllVersions),
        None,
        state.filters.game.is_none(),
        Message::RealmGame(None)
    )]
    .spacing(2);
    for game in Game::ALL.into_iter().filter(|game| game.subscribable()) {
        games = games.push(chip(
            game.label(),
            Some(identity::game_color(game)),
            state.filters.game == Some(game),
            Message::RealmGame(Some(game)),
        ));
    }
    let mut regions = vec![Choice::new(None, l.text(Key::AllRegions))];
    regions.extend(
        snapshot
            .realms
            .iter()
            .map(|realm| realm.region.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|region| Choice::new(Some(region.clone()), region)),
    );
    let region = Choice::new(
        state.filters.region.clone(),
        state
            .filters
            .region
            .as_deref()
            .unwrap_or(l.text(Key::AllRegions)),
    );
    let mut content = column![
        row![
            segmented(games),
            space().width(Length::Fill),
            select(regions, Some(region), |choice| Message::RealmRegion(
                choice.value
            ))
            .width(140)
            .menu_height(200)
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(12);
    let reset = action(l.text(Key::Reset), Some(Message::ResetRealmFilters)).style(ghost);
    if state.filters.game.is_none_or(Game::faction_realms) {
        let mut factions = row![chip(
            l.text(Key::All),
            None,
            state.filters.faction.is_none(),
            Message::RealmFaction(None)
        )]
        .spacing(2);
        for faction in [Faction::Alliance, Faction::Horde] {
            factions = factions.push(chip(
                identity::faction_label(faction, l),
                Some(identity::faction_color(faction)),
                state.filters.faction == Some(faction),
                Message::RealmFaction(Some(faction)),
            ));
        }
        content = content.push(
            row![
                caption(l.text(Key::Faction)),
                segmented(factions),
                space().width(Length::Fill),
                reset
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
        );
    } else {
        content = content.push(
            row![
                caption("Retail · World of Warcraft: Midnight"),
                space().width(Length::Fill),
                reset
            ]
            .align_y(alignment::Vertical::Center),
        );
    }
    let options = state.visible(&snapshot.realms, l);
    let submit = match options.as_slice() {
        [realm] if !busy && snapshot.logged_in && !snapshot.realm_registered(realm) => {
            Some(Message::AddRealm((*realm).clone()))
        }
        _ => None,
    };
    content
        .push(
            input(l.text(Key::SearchRealms), &state.query)
                .id(SEARCH_ID)
                .on_input(Message::RealmSearch)
                .on_submit_maybe(submit),
        )
        .push(results(state, snapshot, busy, adding, l))
        .into()
}

fn results(
    state: &State,
    snapshot: &Snapshot,
    busy: bool,
    adding: Option<&Realm>,
    l: Language,
) -> Element<'static, Message> {
    let options = state.visible(&snapshot.realms, l);
    if options.is_empty() {
        return container(
            column![
                text(if snapshot.realms.is_empty() {
                    l.text(Key::NoRealms)
                } else {
                    l.text(Key::NoMatchingRealms)
                })
                .size(16),
                caption(if !snapshot.logged_in {
                    l.text(Key::SignInForRealms)
                } else if snapshot.realms.is_empty() {
                    l.text(Key::SyncForRealms)
                } else {
                    l.text(Key::AdjustFilters)
                }),
            ]
            .spacing(8),
        )
        .padding(16)
        .width(Length::Fill)
        .style(surface)
        .into();
    }
    let count = options.len();
    let can_submit =
        count == 1 && !busy && snapshot.logged_in && !snapshot.realm_registered(options[0]);
    let rows = column(options.into_iter().take(LIMIT).map(|realm| {
        let (name, faction) = realm_name_and_faction(realm.game, &realm.name);
        let mut metadata: Vec<Element<'_, Message>> = Vec::new();
        if state.filters.game.is_none() {
            metadata.push(identity::version_badge(realm.game));
        }
        if state.filters.region.is_none() {
            metadata.push(caption(realm.region.clone()).into());
        }
        if let Some(faction) = faction {
            metadata.push(identity::badge(
                identity::faction_label(faction, l),
                identity::faction_color(faction),
            ));
        }
        let mut label = column![text(name.to_owned()).size(15)]
            .spacing(5)
            .width(Length::Fill);
        if !metadata.is_empty() {
            label = label.push(
                iced::widget::Row::with_children(metadata)
                    .spacing(8)
                    .align_y(alignment::Vertical::Center),
            );
        }
        let control: Element<'_, Message> = if snapshot.realm_registered(realm) {
            container(caption(l.text(Key::Added)))
                .padding([8, 12])
                .into()
        } else if adding == Some(realm) {
            caption(l.text(Key::Adding)).into()
        } else {
            action(
                l.text(Key::Add),
                (!busy && snapshot.logged_in).then(|| Message::AddRealm(realm.clone())),
            )
            .style(secondary)
            .into()
        };
        container(
            row![label, control]
                .spacing(12)
                .align_y(alignment::Vertical::Center),
        )
        .padding([10, 12])
        .width(Length::Fill)
        .into()
    }));
    // Regular text widgets avoid tiny-skia's cached menu-text clipping defect.
    column![
        row![
            caption(msg!(Matches, count = count).render(l)),
            space().width(Length::Fill),
            caption(if can_submit {
                l.text(Key::EnterToAdd)
            } else {
                l.text(Key::SearchShortcut)
            })
        ],
        container(rows).style(surface),
    ]
    .push((count > LIMIT).then(|| caption(msg!(RealmsMore, count = count - LIMIT).render(l))))
    .spacing(8)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{Layout, Renderer as _, layout, mouse, renderer, widget::Tree};
    use renderer::Headless;

    struct Picker {
        view: Element<'static, Message>,
        tree: Tree,
        node: layout::Node,
        renderer: iced::Renderer,
        viewport: iced::Rectangle,
    }

    impl Picker {
        fn new(realms: &[Realm], query: &str, width: f32) -> Self {
            let mut view: Element<'_, Message> = container(super::results(
                &State {
                    query: query.into(),
                    filters: RealmFilters::default(),
                },
                &Snapshot {
                    realms: realms.to_vec(),
                    logged_in: true,
                    ..Default::default()
                },
                false,
                None,
                Language::German,
            ))
            .padding(20)
            .width(Length::Fill)
            .into();
            let mut tree = Tree::new(view.as_widget());
            let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
            let size = iced::Size::new(width, 320.0);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            Self {
                view,
                tree,
                node,
                renderer,
                viewport: iced::Rectangle::with_size(size),
            }
        }

        fn event(&mut self, event: iced::Event, cursor: iced::Point) -> Vec<Message> {
            let mut messages = Vec::new();
            self.view.as_widget_mut().update(
                &mut self.tree,
                &event,
                Layout::new(&self.node),
                mouse::Cursor::Available(cursor),
                &self.renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &self.viewport,
            );
            messages
        }

        fn draw(&mut self, scale: f32, theme: &Theme) -> Vec<u8> {
            self.event(
                iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
                iced::Point::ORIGIN,
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
                mouse::Cursor::Unavailable,
                &self.viewport,
            );
            self.renderer.screenshot(
                iced::Size::new(
                    (self.viewport.width * scale) as u32,
                    (self.viewport.height * scale) as u32,
                ),
                scale,
                iced::Color::BLACK,
            )
        }

        fn click(&mut self, cursor: iced::Point) -> Vec<Message> {
            self.event(
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                cursor,
            );
            self.event(
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                cursor,
            )
        }
    }

    fn realms() -> Vec<Realm> {
        (0..120)
            .map(|i| Realm {
                game: Game::Progression,
                region: "EU".into(),
                id: format!("realm-{i:03}"),
                name: format!("Étoile-du-matin-{i:03}-Alliance"),
            })
            .collect()
    }

    #[test]
    fn search_combines_localized_factions_with_current_version_and_region_filters() {
        let realms: Vec<Realm> =
            serde_json::from_str(include_str!("../../tests/fixtures/realm-selection.json"))
                .unwrap();
        let mut state = State::new(RealmFilters {
            game: Some(Game::Progression),
            region: Some("EU".into()),
            faction: Some(Faction::Alliance),
        });
        for query in ["pyre allianz", "village alliance", "mop EU", "  PYREWOOD  "] {
            state.query = query.into();
            let visible = state.visible(&realms, Language::German);
            assert_eq!(visible.len(), 1, "{query}");
            assert_eq!(visible[0].id, "eu-pyre-a");
        }
        state.filters.faction = Some(Faction::Horde);
        state.query = "allianz".into();
        assert!(state.visible(&realms, Language::German).is_empty());
    }

    #[test]
    fn long_match_lists_stop_at_the_limit_and_ask_for_a_narrower_search() {
        let realms = realms();
        let picker = Picker::new(&realms, "", 760.0);
        let parts: Vec<_> = Layout::new(&picker.node)
            .children()
            .next()
            .unwrap()
            .children()
            .collect();
        assert_eq!(parts.len(), 3, "header, rows and the narrow-search hint");
        let rows = parts[1].children().next().unwrap().children().count();
        assert_eq!(rows, LIMIT);
        let few = Picker::new(&realms, "matin-073", 760.0);
        let parts = Layout::new(&few.node).children().next().unwrap().children();
        assert_eq!(parts.count(), 2, "no hint when every match is shown");
    }

    #[test]
    fn selecting_a_filtered_realm_preserves_its_identity() {
        let realms = realms();
        let mut picker = Picker::new(&realms, "  ÉTOILE-DU-MATIN-073  ", 760.0);
        let _ = picker.draw(1.0, &super::super::theme::dark());
        let messages = picker.click(iced::Point::new(680.0, 74.0));
        assert!(matches!(messages.as_slice(), [Message::AddRealm(realm)] if realm == &realms[73]));

        let mut empty = Picker::new(&realms, "missing realm", 760.0);
        assert!(empty.click(iced::Point::new(60.0, 40.0)).is_empty());
    }
}
