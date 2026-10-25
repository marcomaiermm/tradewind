//! Coordinates bounded background icon batches. Stale results cannot cross catalogues.
use super::auction::Browser;
pub use crate::adapters::icons::{COIN_TEXTURE_IDS, Icon};
use crate::{adapters::icons::LocalIcons, core::auction::Catalog};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone)]
pub struct Request {
    generation: u64,
    root: PathBuf,
    catalog: Arc<Catalog>,
    ids: Vec<u32>,
}
#[derive(Clone)]
pub struct Loaded {
    generation: u64,
    pub images: BTreeMap<u32, Option<Icon>>,
    failed: bool,
}
#[derive(Default)]
pub struct Icons {
    generation: u64,
    root: PathBuf,
    catalog: Option<Arc<Catalog>>,
    completed: BTreeSet<u32>,
    pub(crate) in_flight: bool,
    pub failed: bool,
}
impl Icons {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn loading(&self) -> bool {
        self.in_flight
    }
    pub fn next(&mut self, root: &std::path::Path, browser: &Browser) -> Option<Request> {
        if self.root != root
            || self
                .catalog
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &browser.catalog))
        {
            self.generation = self.generation.wrapping_add(1);
            self.root = root.to_owned();
            self.catalog = Some(browser.catalog.clone());
            self.completed.clear();
            self.failed = false;
        }
        if self.in_flight || self.failed || browser.catalog.selected.is_none() {
            return None;
        }
        let mut ids: BTreeSet<_> = if let Some(item) = browser.detail() {
            item.texture_id.into_iter().collect()
        } else {
            browser
                .visible()
                .filter_map(|item| item.texture_id)
                .collect()
        };
        ids.extend(COIN_TEXTURE_IDS);
        // Keep the display cache bounded. Disk entries survive navigation/restarts.
        if self.completed.len() + ids.len() > 256 {
            self.completed.retain(|id| ids.contains(id));
        }
        let ids: Vec<_> = ids
            .into_iter()
            .filter(|id| !self.completed.contains(id))
            .take(40)
            .collect();
        if ids.is_empty() {
            return None;
        }
        self.in_flight = true;
        Some(Request {
            generation: self.generation,
            root: self.root.clone(),
            catalog: browser.catalog.clone(),
            ids,
        })
    }
    pub fn accept(&mut self, loaded: &Loaded) -> bool {
        self.in_flight = false;
        if loaded.generation != self.generation {
            return false;
        }
        self.failed = loaded.failed;
        self.completed.extend(loaded.images.keys());
        true
    }
    pub fn retains(&self, id: &u32) -> bool {
        self.completed.contains(id)
    }
}

#[derive(Default)]
pub(super) struct Worker {
    catalog: Option<Arc<Catalog>>,
    root: PathBuf,
    reader: LocalIcons,
}
impl Worker {
    pub fn load(&mut self, request: Request) -> Loaded {
        if self.root != request.root
            || self
                .catalog
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &request.catalog))
        {
            self.reader = LocalIcons::default();
            self.root = request.root.clone();
            self.catalog = Some(request.catalog.clone());
        }
        let result = (|| {
            let Some(market) = &request.catalog.selected else {
                return Ok(BTreeMap::new());
            };
            let known: Vec<_> = request
                .catalog
                .items
                .iter()
                .filter_map(|i| i.texture_id)
                .chain(COIN_TEXTURE_IDS)
                .collect();
            let cache = crate::adapters::icons::cache_directory()?;
            self.reader.load(
                &crate::adapters::wow::normalize_root(&request.root),
                market.game,
                &cache,
                &request.ids,
                &known,
            )
        })();
        Loaded {
            generation: request.generation,
            failed: result.is_err(),
            images: result.unwrap_or_default(),
        }
    }
}
pub(super) fn failed(request: &Request) -> Loaded {
    Loaded {
        generation: request.generation,
        images: BTreeMap::new(),
        failed: true,
    }
}

#[cfg(test)]
pub(crate) fn preview(
    root: &std::path::Path,
    cache: &std::path::Path,
    catalog: &mut Catalog,
) -> anyhow::Result<BTreeMap<u32, Icon>> {
    let installation = crate::adapters::wow::scan(root)?;
    let source = crate::adapters::auction::load(
        root,
        &installation,
        catalog.selected.as_ref(),
        &[],
        crate::i18n::Language::English,
    )?;
    for item in &mut catalog.items {
        item.texture_id = source
            .items
            .iter()
            .find(|other| other.id == item.id)
            .and_then(|other| other.texture_id);
    }
    let ids: Vec<_> = catalog
        .items
        .iter()
        .filter_map(|i| i.texture_id)
        .chain(COIN_TEXTURE_IDS)
        .collect();
    Ok(LocalIcons::default()
        .load(
            root,
            catalog.selected.as_ref().unwrap().game,
            cache,
            &ids,
            &ids,
        )?
        .into_iter()
        .filter_map(|(id, icon)| icon.map(|icon| (id, icon)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::auction::{Item, Market};
    #[test]
    fn coin_textures_and_a_full_page_finish_in_bounded_batches() {
        let catalog = Arc::new(Catalog {
            selected: Some(Market {
                game: crate::core::Game::Anniversary,
                scope: "Realm".into(),
                region: None,
                region_only: false,
            }),
            items: (1..=40)
                .map(|id| Item {
                    id: format!("i:{id}"),
                    texture_id: Some(id),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        let mut browser = Browser::default();
        browser.replace(Some(&catalog), &[]);
        let mut state = Icons::default();
        let root = PathBuf::from("installation");
        for expected in [40, 3] {
            let request = state.next(&root, &browser).unwrap();
            assert_eq!(request.ids.len(), expected);
            assert!(state.accept(&Loaded {
                generation: request.generation,
                images: request.ids.into_iter().map(|id| (id, None)).collect(),
                failed: false
            }));
        }
        assert!(COIN_TEXTURE_IDS.iter().all(|id| state.retains(id)));
        assert!(state.next(&root, &browser).is_none());
    }
    #[test]
    fn batches_are_deduplicated_and_old_results_rejected_after_source_change() {
        let catalog = Arc::new(Catalog {
            selected: Some(Market {
                game: crate::core::Game::Retail,
                scope: "EU".into(),
                region: None,
                region_only: true,
            }),
            items: (0..100)
                .map(|i| Item {
                    id: i.to_string(),
                    texture_id: Some(i / 2),
                    ..Item::default()
                })
                .collect(),
            ..Catalog::default()
        });
        let mut browser = Browser::default();
        browser.replace(Some(&catalog), &[]);
        let mut state = Icons::default();
        let request = state
            .next(PathBuf::from("first").as_path(), &browser)
            .unwrap();
        assert!(request.ids.len() <= 40);
        assert!(
            state
                .next(PathBuf::from("first").as_path(), &browser)
                .is_none()
        );
        assert!(
            state
                .next(PathBuf::from("second").as_path(), &browser)
                .is_none()
        );
        assert!(!state.accept(&failed(&request)));
        assert!(!state.failed);
        let request = state
            .next(PathBuf::from("second").as_path(), &browser)
            .unwrap();
        assert!(state.accept(&failed(&request)));
        assert!(
            state
                .next(PathBuf::from("second").as_path(), &browser)
                .is_none()
        );
        browser.replace(Some(&Arc::new((*catalog).clone())), &[]);
        assert!(
            state
                .next(PathBuf::from("second").as_path(), &browser)
                .is_some()
        );
    }
}
