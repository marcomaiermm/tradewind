//! Presentation-independent browsing state over an immutable local snapshot.
use crate::core::auction::{self, Catalog, Favorite, Item, Query, Sort, SortField};
use std::sync::Arc;
pub const PAGE_SIZE: usize = 40;
#[derive(Clone, Debug)]
pub enum Browse {
    Search(String),
    Class(Option<u8>),
    Quality(Option<u8>),
    Sort(Sort),
    SortColumn(SortField),
    Available(bool),
    HotDeals(bool),
    Favorites(bool),
    Page(usize),
    Select(String),
    /// Arrow keys on an open item: next (`true`) or previous match, across pages.
    Step(bool),
    Back,
    Reset,
}
#[derive(Default)]
pub struct Browser {
    pub catalog: Arc<Catalog>,
    pub query: Query,
    pub page: usize,
    pub selected: Option<String>,
    matches: Vec<usize>,
    pub classes: Vec<u8>,
    pub missing_names: usize,
    favorites: Vec<Favorite>,
}
impl Browser {
    pub fn replace(&mut self, data: Option<&Arc<Catalog>>, favorites: &[Favorite]) {
        if data.is_none() && (self.catalog.selected.is_some() || !self.catalog.markets.is_empty()) {
            self.catalog = Arc::new(Catalog::default());
            self.selected = None;
            self.page = 0;
            self.matches.clear();
            self.classes.clear();
            self.missing_names = 0;
        }
        let changed = data.is_some_and(|data| !Arc::ptr_eq(data, &self.catalog));
        if changed {
            let data = data.unwrap();
            if data.selected != self.catalog.selected {
                self.selected = None;
                self.page = 0;
            }
            self.catalog = data.clone();
            self.classes = self.catalog.items.iter().filter_map(|i| i.class).collect();
            self.classes.sort();
            self.classes.dedup();
            self.missing_names = self
                .catalog
                .items
                .iter()
                .filter(|i| i.name.is_none())
                .count();
        }
        if changed || self.favorites != favorites {
            self.favorites = favorites.to_vec();
            self.refresh();
        }
    }
    pub fn dispatch(&mut self, action: Browse) {
        match action {
            Browse::Search(text) => self.query.text = text,
            Browse::Class(class) => self.query.class = class,
            Browse::Quality(quality) => self.query.quality = quality,
            Browse::Sort(sort) => self.query.sort = sort,
            Browse::SortColumn(field) => self.query.sort = self.query.sort.toggle_column(field),
            Browse::Available(available) => self.query.available_only = available,
            Browse::HotDeals(enabled) => self.query.hot_deals_only = enabled,
            Browse::Favorites(favorites) => self.query.favorites_only = favorites,
            Browse::Page(page) => {
                self.page = page.min(self.page_count().saturating_sub(1));
                return;
            }
            Browse::Select(id) => {
                self.selected = self.catalog.items.iter().any(|i| i.id == id).then_some(id);
                return;
            }
            Browse::Step(forward) => {
                let current = self.selected.as_ref().and_then(|id| {
                    self.matches
                        .iter()
                        .position(|&i| &self.catalog.items[i].id == id)
                });
                // Without an open item the arrows open the first item of the page.
                let Some(current) = current else {
                    if let Some(&first) = self.matches.get(self.page * PAGE_SIZE) {
                        self.selected = Some(self.catalog.items[first].id.clone());
                    }
                    return;
                };
                let next = if forward {
                    (current + 1).min(self.matches.len() - 1)
                } else {
                    current.saturating_sub(1)
                };
                self.selected = Some(self.catalog.items[self.matches[next]].id.clone());
                self.page = next / PAGE_SIZE;
                return;
            }
            Browse::Back => {
                self.selected = None;
                return;
            }
            Browse::Reset => self.query = Query::default(),
        }
        self.page = 0;
        self.refresh();
    }
    fn refresh(&mut self) {
        self.matches = auction::search(&self.catalog, &self.query, &self.favorites);
        self.page = self.page.min(self.page_count().saturating_sub(1));
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.catalog.items.iter().any(|i| &i.id == id))
        {
            self.selected = None;
        }
    }
    pub fn total(&self) -> usize {
        self.matches.len()
    }
    pub fn page_count(&self) -> usize {
        self.total().div_ceil(PAGE_SIZE)
    }
    pub fn visible(&self) -> impl Iterator<Item = &Item> {
        self.matches
            .iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|index| &self.catalog.items[*index])
    }
    pub fn detail(&self) -> Option<&Item> {
        self.selected
            .as_ref()
            .and_then(|id| self.catalog.items.iter().find(|i| &i.id == id))
    }
    pub fn favorite(&self, item: &Item) -> bool {
        self.catalog
            .selected
            .as_ref()
            .is_some_and(|m| item.is_favorite(m.game, &self.favorites))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn column_clicks_toggle_whole_result_and_reset_page_without_clearing_filters() {
        use auction::{SortDirection::*, Value};
        let mut browser = Browser::default();
        let items = (0..100)
            .map(|n| {
                let mut item = Item {
                    id: format!("i:{n}"),
                    name: Some(format!("Item {n:03}")),
                    class: Some(7),
                    quality: Some((n % 5) as u8),
                    min_buyout: Value::new(n + 1, 100),
                    ..Default::default()
                };
                item.index();
                item
            })
            .collect();
        browser.replace(
            Some(&Arc::new(Catalog {
                items,
                ..Default::default()
            })),
            &[],
        );
        browser.dispatch(Browse::Step(true));
        assert_eq!(
            browser.selected.as_deref(),
            Some("i:0"),
            "arrows open the first item of the page"
        );
        browser.dispatch(Browse::Select("i:39".into()));
        browser.dispatch(Browse::Step(true));
        assert_eq!(
            (browser.selected.as_deref(), browser.page),
            (Some("i:40"), 1)
        );
        browser.dispatch(Browse::Step(false));
        assert_eq!(
            (browser.selected.as_deref(), browser.page),
            (Some("i:39"), 0)
        );
        browser.dispatch(Browse::Select("i:99".into()));
        browser.dispatch(Browse::Step(true));
        assert_eq!(
            browser.selected.as_deref(),
            Some("i:99"),
            "the last item stays put"
        );
        browser.dispatch(Browse::Back);
        browser.dispatch(Browse::Class(Some(7)));
        browser.dispatch(Browse::Search("Item".into()));
        browser.dispatch(Browse::Available(true));
        browser.dispatch(Browse::Page(2));
        browser.dispatch(Browse::SortColumn(SortField::Price));
        assert_eq!(browser.query.sort, Sort::By(SortField::Price, Ascending));
        assert_eq!(
            (browser.page, browser.visible().next().unwrap().id.as_str()),
            (0, "i:0")
        );
        browser.dispatch(Browse::SortColumn(SortField::Price));
        assert_eq!(browser.query.sort, Sort::By(SortField::Price, Descending));
        assert_eq!(
            browser.visible().next().unwrap().id,
            "i:99",
            "sort all results before taking the page"
        );
        browser.dispatch(Browse::SortColumn(SortField::Quality));
        assert_eq!(browser.query.sort, Sort::By(SortField::Quality, Descending));
        assert_eq!(browser.visible().next().unwrap().quality, Some(4));
        browser.dispatch(Browse::SortColumn(SortField::Quality));
        assert_eq!(browser.query.sort, Sort::By(SortField::Quality, Ascending));
        browser.dispatch(Browse::SortColumn(SortField::Name));
        assert_eq!(browser.query.sort, Sort::By(SortField::Name, Ascending));
        assert_eq!(browser.query.class, Some(7));
        assert_eq!(browser.query.text, "Item");
        assert!(browser.query.available_only);
        browser.dispatch(Browse::Reset);
        assert_eq!(browser.query.sort, Sort::Recommended);
    }
    #[test]
    fn pages_search_and_selection_remain_valid_after_reload() {
        let mut items: Vec<_> = (1..=95)
            .map(|n| Item {
                id: format!("i:{n}"),
                name: Some(format!("Item {n:03}")),
                ..Item::default()
            })
            .collect();
        for item in &mut items {
            item.index();
        }
        let mut browser = Browser::default();
        browser.replace(
            Some(&Arc::new(Catalog {
                items,
                ..Catalog::default()
            })),
            &[],
        );
        assert_eq!(browser.visible().count(), 40);
        browser.dispatch(Browse::Page(999));
        assert_eq!(browser.page, 2);
        assert_eq!(browser.visible().count(), 15);
        browser.dispatch(Browse::Select("i:95".into()));
        assert!(browser.detail().is_some());
        browser.dispatch(Browse::Search("001".into()));
        assert_eq!((browser.total(), browser.page), (1, 0));
        browser.replace(Some(&Arc::new(Catalog::default())), &[]);
        assert!(browser.detail().is_none());
        assert_eq!((browser.page_count(), browser.page), (0, 0));
    }
    #[test]
    fn hot_deal_toggle_resets_page_and_reset_restores_recommendations() {
        let mut browser = Browser::default();
        browser.replace(
            Some(&Arc::new(Catalog {
                items: (0..50)
                    .map(|id| Item {
                        id: format!("i:{id}"),
                        min_buyout: auction::Value::new(50, 100),
                        market: auction::Value::new(100, 100),
                        sale_rate: auction::Value::new(if id < 2 { 200 } else { 5 }, 100),
                        sold_per_day: auction::Value::new(5000, 100),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            })),
            &[],
        );
        browser.dispatch(Browse::Page(1));
        browser.dispatch(Browse::HotDeals(true));
        assert_eq!(
            (browser.total(), browser.page, browser.page_count()),
            (2, 0, 1)
        );
        browser.dispatch(Browse::HotDeals(false));
        assert_eq!(browser.total(), 50);
        browser.dispatch(Browse::HotDeals(true));
        browser.dispatch(Browse::Sort(Sort::By(
            SortField::Name,
            auction::SortDirection::Ascending,
        )));
        browser.dispatch(Browse::Reset);
        assert!(!browser.query.hot_deals_only);
        assert_eq!(
            (browser.total(), browser.query.sort),
            (50, Sort::Recommended)
        );
    }
}
