//! Read-only AuctionDB snapshots. Amounts are copper, zero means unavailable in TSM.
use super::Game;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Market {
    pub game: Game,
    pub scope: String,
    pub region: Option<String>,
    pub region_only: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorite {
    pub game: Game,
    pub item: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value {
    pub amount: u64,
    pub timestamp: i64,
}
impl Value {
    pub fn new(amount: u64, timestamp: i64) -> Option<Self> {
        (amount > 0).then_some(Self { amount, timestamp })
    }
    pub fn coins(self) -> (u64, u64, u64) {
        (
            self.amount / 10_000,
            self.amount / 100 % 100,
            self.amount % 100,
        )
    }
}
#[derive(Clone, Debug, Default)]
pub struct ItemDetails {
    pub level: Option<u32>,
    pub required_level: Option<u32>,
    /// Copper. Unlike AuctionDB prices, a zero vendor price is a known value.
    pub vendor_price: Option<u32>,
    pub stack_size: Option<u32>,
    pub binds_on_pickup: bool,
    pub crafting_reagent: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub id: String,
    pub name: Option<String>,
    pub class: Option<u8>,
    pub quality: Option<u8>,
    pub texture_id: Option<u32>,
    /// Only populated for the exact cached item string, never a base-item fallback.
    pub details: Option<ItemDetails>,
    pub min_buyout: Option<Value>,
    pub auctions: Option<Value>,
    pub market: Option<Value>,
    pub recent: Option<Value>,
    pub historical: Option<Value>,
    pub region_market: Option<Value>,
    pub region_historical: Option<Value>,
    pub region_sale: Option<Value>,
    /// TSM encodes both regional rates in thousandths.
    pub sold_per_day: Option<Value>,
    pub sale_rate: Option<Value>,
    pub commodity: bool,
    pub search_text: String,
}
/// An application heuristic over TSM's regional averages, not a forecast for this realm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Demand {
    Unknown,
    Low,
    Active,
    High,
}
pub const HOT_DEAL_DISCOUNT_PERCENT: u8 = 30;
impl Item {
    pub fn index(&mut self) {
        self.search_text =
            format!("{} {}", self.name.as_deref().unwrap_or(""), self.id).to_lowercase();
    }
    pub fn is_favorite(&self, game: Game, favorites: &[Favorite]) -> bool {
        favorites
            .iter()
            .any(|f| f.game == game && f.item == self.id)
    }
    pub fn demand(&self) -> Demand {
        let (Some(rate), Some(sold)) = (self.sale_rate, self.sold_per_day) else {
            return Demand::Unknown;
        };
        if rate.amount > 1000 {
            return Demand::Unknown;
        }
        match (rate.amount, sold.amount) {
            (200.., 5000..) => Demand::High,
            (100.., 1000..) => Demand::Active,
            _ => Demand::Low,
        }
    }
    /// Whole percent below this market's value. No regional-price fallback.
    pub fn discount_percent(&self) -> Option<u8> {
        let (price, market) = (self.min_buyout?, self.market?);
        if price.amount == 0 || market.amount <= price.amount {
            return None;
        }
        Some(((u128::from(market.amount - price.amount) * 100) / u128::from(market.amount)) as u8)
    }
    pub fn hot_deal(&self) -> Option<u8> {
        let discount = self.discount_percent()?;
        (discount >= HOT_DEAL_DISCOUNT_PERCENT && self.demand() >= Demand::Active)
            .then_some(discount)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub markets: Vec<Market>,
    pub selected: Option<Market>,
    pub items: Vec<Item>,
    pub names_locale: Option<String>,
    pub warnings: Vec<crate::i18n::Text>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Recommended,
    By(SortField, SortDirection),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortField {
    Name,
    Quality,
    Price,
    Market,
    Auctions,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}
impl Sort {
    pub fn toggle_column(self, field: SortField) -> Self {
        use SortDirection::*;
        let direction = match self {
            Self::By(current, Ascending) if current == field => Descending,
            Self::By(current, Descending) if current == field => Ascending,
            _ => match field {
                SortField::Name | SortField::Price => Ascending,
                SortField::Quality | SortField::Market | SortField::Auctions => Descending,
            },
        };
        Self::By(field, direction)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: String,
    pub class: Option<u8>,
    pub quality: Option<u8>,
    pub favorites_only: bool,
    pub available_only: bool,
    pub hot_deals_only: bool,
    pub sort: Sort,
}
/// Missing values always sort last, including descending sorts.
fn numeric_order(a: Option<Value>, b: Option<Value>, descending: bool) -> Ordering {
    optional_order(a.map(|v| v.amount), b.map(|v| v.amount), descending)
}
fn optional_order<T: Ord>(a: Option<T>, b: Option<T>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            if descending {
                b.cmp(&a)
            } else {
                a.cmp(&b)
            }
        }
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}
pub fn search(catalog: &Catalog, query: &Query, favorites: &[Favorite]) -> Vec<usize> {
    let terms: Vec<_> = query
        .text
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let game = catalog.selected.as_ref().map(|m| m.game);
    let mut indices: Vec<_> = catalog
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            query.class.is_none_or(|class| item.class == Some(class))
                && query
                    .quality
                    .is_none_or(|quality| item.quality == Some(quality))
                && (!query.available_only || item.min_buyout.is_some())
                && (!query.hot_deals_only || item.hot_deal().is_some())
                && (!query.favorites_only
                    || game.is_some_and(|game| item.is_favorite(game, favorites)))
                && terms.iter().all(|term| item.search_text.contains(term))
        })
        .map(|(index, _)| index)
        .collect();
    indices.sort_by(|a, b| {
        let a = &catalog.items[*a];
        let b = &catalog.items[*b];
        let order = match query.sort {
            Sort::Recommended => b
                .min_buyout
                .is_some()
                .cmp(&a.min_buyout.is_some())
                .then_with(|| b.hot_deal().is_some().cmp(&a.hot_deal().is_some()))
                .then_with(|| b.demand().cmp(&a.demand()))
                .then_with(|| b.hot_deal().cmp(&a.hot_deal()))
                .then_with(|| numeric_order(a.sold_per_day, b.sold_per_day, true))
                .then_with(|| numeric_order(a.sale_rate, b.sale_rate, true)),
            Sort::By(field, direction) => {
                let descending = direction == SortDirection::Descending;
                match field {
                    SortField::Name => optional_order(
                        a.name.as_ref().map(|_| &a.search_text),
                        b.name.as_ref().map(|_| &b.search_text),
                        descending,
                    ),
                    SortField::Quality => optional_order(
                        a.quality.filter(|q| *q <= 8),
                        b.quality.filter(|q| *q <= 8),
                        descending,
                    ),
                    SortField::Price => numeric_order(a.min_buyout, b.min_buyout, descending),
                    SortField::Market => numeric_order(
                        a.market.or(a.region_market),
                        b.market.or(b.region_market),
                        descending,
                    ),
                    SortField::Auctions => numeric_order(a.auctions, b.auctions, descending),
                }
            }
        };
        order
            .then_with(|| a.name.is_none().cmp(&b.name.is_none()))
            .then_with(|| a.search_text.cmp(&b.search_text))
            .then_with(|| a.id.cmp(&b.id))
    });
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_columns_sort_both_ways_with_missing_values_last() {
        let mut items = vec![];
        for (id, name, quality, price, supply) in [
            ("i:3", Some("Copper"), Some(1), Some(50), Some(10)),
            ("i:2", Some("Blade"), Some(4), Some(1000), Some(2)),
            ("i:1", Some("Armor"), Some(2), Some(200), Some(5)),
            ("i:4", None, None, None, None),
        ] {
            let mut item = Item {
                id: id.into(),
                name: name.map(str::to_owned),
                quality,
                min_buyout: price.and_then(|v| Value::new(v, 100)),
                market: price.and_then(|v| Value::new(v * 2, 100)),
                auctions: supply.and_then(|v| Value::new(v, 100)),
                ..Default::default()
            };
            item.index();
            items.push(item);
        }
        let catalog = Catalog {
            items,
            ..Default::default()
        };
        for (field, ascending, descending) in [
            (SortField::Name, [2, 1, 0, 3], [0, 1, 2, 3]),
            (SortField::Quality, [0, 2, 1, 3], [1, 2, 0, 3]),
            (SortField::Price, [0, 2, 1, 3], [1, 2, 0, 3]),
            (SortField::Market, [0, 2, 1, 3], [1, 2, 0, 3]),
            (SortField::Auctions, [1, 2, 0, 3], [0, 2, 1, 3]),
        ] {
            for (direction, expected) in [
                (SortDirection::Ascending, ascending),
                (SortDirection::Descending, descending),
            ] {
                let query = Query {
                    sort: Sort::By(field, direction),
                    ..Default::default()
                };
                assert_eq!(
                    search(&catalog, &query, &[]),
                    expected,
                    "{field:?} {direction:?}"
                );
            }
        }
    }
    #[test]
    fn quality_order_is_numeric_and_ties_remain_stable() {
        let mut items: Vec<_> = [
            Some(4),
            Some(2),
            Some(1),
            Some(0),
            Some(8),
            Some(7),
            None,
            Some(255),
        ]
        .into_iter()
        .enumerate()
        .map(|(id, quality)| Item {
            id: format!("i:{id}"),
            name: Some("Same name".into()),
            quality,
            ..Default::default()
        })
        .collect();
        for item in &mut items {
            item.index();
        }
        items.reverse();
        let catalog = Catalog {
            items,
            ..Default::default()
        };
        let ids = |direction| {
            search(
                &catalog,
                &Query {
                    sort: Sort::By(SortField::Quality, direction),
                    ..Default::default()
                },
                &[],
            )
            .into_iter()
            .map(|i| catalog.items[i].id.as_str())
            .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(SortDirection::Ascending),
            ["i:3", "i:2", "i:1", "i:0", "i:5", "i:4", "i:6", "i:7"]
        );
        assert_eq!(
            ids(SortDirection::Descending),
            ["i:4", "i:5", "i:0", "i:1", "i:2", "i:3", "i:6", "i:7"]
        );
    }
    #[test]
    fn deals_require_discount_and_real_sales_demand() {
        let mut item = Item {
            min_buyout: Value::new(700, 100),
            market: Value::new(1000, 100),
            sale_rate: Value::new(100, 100),
            sold_per_day: Value::new(1000, 100),
            ..Default::default()
        };
        assert_eq!((item.demand(), item.hot_deal()), (Demand::Active, Some(30)));
        item.min_buyout = Value::new(701, 100);
        assert_eq!(
            item.hot_deal(),
            None,
            "29.9% is below the confirmed threshold"
        );
        item.min_buyout = Value::new(100, 100);
        for (rate, sold, demand) in [
            (99, 1_000_000, Demand::Low),
            (1000, 999, Demand::Low),
            (199, 5000, Demand::Active),
            (200, 4999, Demand::Active),
            (200, 5000, Demand::High),
            (1001, 5000, Demand::Unknown),
        ] {
            item.sale_rate = Value::new(rate, 100);
            item.sold_per_day = Value::new(sold, 100);
            assert_eq!(item.demand(), demand);
            assert_eq!(item.hot_deal().is_some(), demand >= Demand::Active);
        }
        item.sale_rate = None;
        item.auctions = Value::new(1_000_000, 100);
        assert_eq!(
            (item.demand(), item.hot_deal()),
            (Demand::Unknown, None),
            "supply is not demand"
        );
        item.sale_rate = Value::new(200, 100);
        item.sold_per_day = None;
        assert_eq!(item.hot_deal(), None);
        item.sold_per_day = Value::new(5000, 100);
        item.market = None;
        item.region_market = Value::new(1000, 100);
        assert_eq!(
            item.hot_deal(),
            None,
            "regional values cannot replace the selected market"
        );
        item.market = Value::new(u64::MAX, 100);
        assert_eq!(item.hot_deal(), Some(99), "large amounts cannot overflow");
        item.min_buyout = Value::new(u64::MAX, 100);
        assert_eq!(item.hot_deal(), None);
        item.min_buyout = None;
        assert_eq!(item.hot_deal(), None);
    }
    #[test]
    fn recommendations_prioritize_liquid_deals_and_filter_composes() {
        let items = [
            ("slow-cheap", Some(1), Some(5), Some(20)),
            ("unknown", Some(1), None, None),
            ("active-deal", Some(50), Some(100), Some(1000)),
            ("high-deal", Some(70), Some(200), Some(5000)),
            ("high-normal", Some(100), Some(400), Some(9000)),
            ("unavailable", None, Some(1000), Some(100_000)),
        ]
        .into_iter()
        .map(|(id, price, rate, sold)| {
            let mut item = Item {
                id: id.into(),
                name: Some(id.into()),
                market: Value::new(100, 100),
                min_buyout: price.and_then(|v| Value::new(v, 100)),
                sale_rate: rate.and_then(|v| Value::new(v, 100)),
                sold_per_day: sold.and_then(|v| Value::new(v, 100)),
                ..Default::default()
            };
            item.index();
            item
        })
        .collect();
        let catalog = Catalog {
            items,
            ..Default::default()
        };
        let mut query = Query::default();
        assert_eq!(query.sort, Sort::Recommended);
        assert_eq!(search(&catalog, &query, &[]), [3, 2, 4, 0, 1, 5]);
        query.hot_deals_only = true;
        assert_eq!(search(&catalog, &query, &[]), [3, 2]);
        query.sort = Sort::By(SortField::Price, SortDirection::Ascending);
        assert_eq!(search(&catalog, &query, &[]), [2, 3]);
        query.text = "active".into();
        assert_eq!(search(&catalog, &query, &[]), [2]);
        query.class = Some(7);
        assert!(search(&catalog, &query, &[]).is_empty());
    }
    fn catalog() -> Catalog {
        let mut items = vec![
            Item {
                id: "i:42".into(),
                name: Some("Kupfererz".into()),
                min_buyout: Value::new(12345, 100),
                class: Some(7),
                quality: Some(1),
                ..Item::default()
            },
            Item {
                id: "i:43".into(),
                name: Some("Kupferbarren".into()),
                min_buyout: Value::new(678, 100),
                class: Some(7),
                quality: Some(1),
                ..Item::default()
            },
            Item {
                id: "i:44".into(),
                ..Item::default()
            },
        ];
        for item in &mut items {
            item.index();
        }
        Catalog {
            selected: Some(Market {
                game: Game::Retail,
                scope: "EU".into(),
                region: Some("EU".into()),
                region_only: true,
            }),
            items,
            ..Catalog::default()
        }
    }
    #[test]
    fn queries_preserve_missing_prices_currency_and_scoped_favorites() {
        let catalog = catalog();
        let mut q = Query {
            sort: Sort::By(SortField::Price, SortDirection::Ascending),
            ..Query::default()
        };
        assert_eq!(search(&catalog, &q, &[]), [1, 0, 2]);
        q.sort = Sort::By(SortField::Price, SortDirection::Descending);
        assert_eq!(search(&catalog, &q, &[]), [0, 1, 2]);
        assert_eq!(catalog.items[0].min_buyout.unwrap().coins(), (1, 23, 45));
        assert_eq!(Value::new(0, 100), None);
        q.text = "KUPFER erz".into();
        assert_eq!(search(&catalog, &q, &[]), [0]);
        q.text = "i:44".into();
        assert_eq!(search(&catalog, &q, &[]), [2]);
        q = Query {
            favorites_only: true,
            ..Query::default()
        };
        let f = Favorite {
            game: Game::ClassicEra,
            item: "i:42".into(),
        };
        assert!(search(&catalog, &q, &[f]).is_empty());
        assert_eq!(
            search(
                &catalog,
                &q,
                &[Favorite {
                    game: Game::Retail,
                    item: "i:43".into()
                }]
            ),
            [1]
        );
        q = Query {
            class: Some(7),
            quality: Some(1),
            available_only: true,
            ..Query::default()
        };
        assert_eq!(search(&catalog, &q, &[]).len(), 2);
    }
}
