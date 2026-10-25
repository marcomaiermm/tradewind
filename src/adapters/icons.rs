//! Local-only CASC access. Game archives are opened read-only, only our cache is written.
mod blp;
mod casc;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};

// UI-GoldIcon, UI-SilverIcon, UI-CopperIcon: verified against the local WoW root manifest.
pub const COIN_TEXTURE_IDS: [u32; 3] = [237618, 237620, 237617];

#[derive(Clone, Debug)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

#[derive(Default)]
pub struct LocalIcons {
    reader: Option<casc::Reader>,
}
impl LocalIcons {
    pub fn load(
        &mut self,
        root: &Path,
        game: crate::core::Game,
        cache: &Path,
        ids: &[u32],
        known_ids: &[u32],
    ) -> Result<BTreeMap<u32, Option<Icon>>> {
        let build = casc::Build::read(root, game)?;
        let mut identity = Sha256::new();
        identity.update(root.as_os_str().as_bytes());
        identity.update(b"\0");
        identity.update(build.product);
        let namespace = format!("{:x}", identity.finalize());
        let namespace = cache.join("item-icons-v1").join(namespace);
        let directory = namespace.join(&build.key);
        let mut result = BTreeMap::new();
        let mut wrote_cache = false;
        for &id in ids.iter().take(40) {
            let path = directory.join(format!("{id}.rgba"));
            if let Ok(icon) = read_cached(&path) {
                result.insert(id, Some(icon));
                continue;
            }
            if self.reader.as_ref().is_none_or(|r| r.build != build) {
                self.reader = None;
                self.reader = Some(casc::Reader::open(build.clone(), known_ids)?);
            }
            let icon = self
                .reader
                .as_ref()
                .unwrap()
                .read(id)
                .and_then(|data| blp::decode(&data))
                .ok();
            if let Some(icon) = &icon {
                // A full/unwritable cache must never hide a successfully decoded image.
                wrote_cache |= super::files::atomic_write(&path, &encode_cached(icon)).is_ok();
            }
            result.insert(id, icon);
        }
        if wrote_cache {
            let _ = trim_cache(&namespace, 64 * 1024 * 1024);
        }
        Ok(result)
    }
}
// 64 MiB per installation/product, including older builds. Never follow symlinks
// or remove files outside the cache's own build/texture naming scheme.
fn trim_cache(namespace: &Path, budget: u64) -> Result<()> {
    let mut entries = Vec::new();
    let mut total = 0;
    for build in std::fs::read_dir(namespace)? {
        let build = build?;
        let name = build.file_name();
        let name = name.to_string_lossy();
        if !build.file_type()?.is_dir()
            || name.len() != 32
            || !name.bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        for file in std::fs::read_dir(build.path())? {
            let file = file?;
            if !file.file_type()?.is_file() {
                continue;
            }
            let path = file.path();
            if path.extension().is_none_or(|ext| ext != "rgba")
                || path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_none()
            {
                continue;
            }
            let metadata = file.metadata()?;
            total += metadata.len();
            entries.push((metadata.modified()?, path, metadata.len()));
        }
    }
    entries.sort();
    for (_, path, size) in entries {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}
fn encode_cached(icon: &Icon) -> Vec<u8> {
    let mut data = b"TSI1".to_vec();
    data.extend(icon.width.to_le_bytes());
    data.extend(icon.height.to_le_bytes());
    data.extend(Sha256::digest(&icon.rgba));
    data.extend_from_slice(&icon.rgba);
    data
}
fn read_cached(path: &Path) -> Result<Icon> {
    use std::io::Read;
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 64 * 4 + 45)
        .read_to_end(&mut data)?;
    decode_cached(&data)
}
fn decode_cached(data: &[u8]) -> Result<Icon> {
    ensure!(
        data.len() >= 44 && &data[..4] == b"TSI1",
        "Invalid icon cache"
    );
    let width = u32::from_le_bytes(data[4..8].try_into()?);
    let height = u32::from_le_bytes(data[8..12].try_into()?);
    ensure!(
        (1..=64).contains(&width) && (1..=64).contains(&height),
        "Invalid icon dimensions"
    );
    ensure!(
        data.len() == 44 + (width * height * 4) as usize,
        "Truncated icon cache"
    );
    ensure!(
        Sha256::digest(&data[44..]).as_slice() == &data[12..44],
        "Corrupt icon cache"
    );
    Ok(Icon {
        width,
        height,
        rgba: data[44..].into(),
    })
}
pub fn cache_directory() -> Result<PathBuf> {
    Ok(directories::ProjectDirs::from("", "", "tradewind")
        .context("Missing cache directory")?
        .cache_dir()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_budget_covers_old_builds_and_does_not_follow_links() {
        let root = tempfile::tempdir().unwrap();
        let build = root.path().join("0123456789abcdef0123456789abcdef");
        std::fs::create_dir(&build).unwrap();
        std::fs::write(build.join("1.rgba"), [0; 48]).unwrap();
        std::fs::write(build.join("2.rgba"), [0; 48]).unwrap();
        std::fs::write(build.join("keep.txt"), "keep").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("3.rgba"), [0; 48]).unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            root.path().join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        )
        .unwrap();
        trim_cache(root.path(), 48).unwrap();
        assert_eq!(
            ["1.rgba", "2.rgba"]
                .iter()
                .filter(|name| build.join(name).exists())
                .count(),
            1
        );
        assert!(build.join("keep.txt").exists());
        assert!(outside.path().join("3.rgba").exists());
    }
    #[test]
    fn cache_rejects_truncation_dimensions_and_corruption() {
        let icon = Icon {
            width: 1,
            height: 1,
            rgba: Arc::from([10, 20, 30, 40]),
        };
        let mut data = encode_cached(&icon);
        assert_eq!(&*decode_cached(&data).unwrap().rgba, &[10, 20, 30, 40]);
        assert!(decode_cached(&data[..47]).is_err());
        data[44] ^= 1;
        assert!(decode_cached(&data).is_err());
        data[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_cached(&data).is_err());
    }
    #[test]
    #[ignore = "Read-only probe of an explicitly supplied local WoW installation"]
    fn local_icons_from_installed_wow() {
        let root =
            PathBuf::from(std::env::var("TSM_AUCTION_WOW_PATH").expect("TSM_AUCTION_WOW_PATH"));
        let cache = PathBuf::from(std::env::var("TSM_ICON_PROBE_DIR").expect("TSM_ICON_PROBE_DIR"));
        let installation = super::super::wow::scan(&root).unwrap();
        let catalog = super::super::auction::load(
            &root,
            &installation,
            None,
            &[],
            crate::i18n::Language::English,
        )
        .unwrap();
        let ids: Vec<_> = catalog
            .items
            .iter()
            .filter_map(|item| item.texture_id)
            .chain(COIN_TEXTURE_IDS)
            .collect();
        let game = catalog.selected.as_ref().unwrap().game;
        let coins = LocalIcons::default()
            .load(&root, game, &cache, &COIN_TEXTURE_IDS, &COIN_TEXTURE_IDS)
            .unwrap();
        assert!(
            COIN_TEXTURE_IDS.iter().all(|id| coins[id].is_some()),
            "all three original MoneyFrame textures must decode"
        );
        eprintln!("MoneyFrame coins: 3/3");
        let images = LocalIcons::default()
            .load(&root, game, &cache, &ids[..ids.len().min(40)], &ids)
            .unwrap();
        let count = images.values().filter(|icon| icon.is_some()).count();
        eprintln!("Local icons: {count}/{}", images.len());
        assert!(count > 0);
        let mut reopened = LocalIcons::default();
        let cached = reopened
            .load(
                &root,
                game,
                &cache,
                &images.keys().copied().collect::<Vec<_>>(),
                &ids,
            )
            .unwrap();
        assert_eq!(cached.values().filter(|i| i.is_some()).count(), count);
        assert!(reopened.reader.is_none(), "cache hits must not reopen CASC");
        for (id, icon) in images {
            if let Some(icon) = icon {
                super::super::desktop::save_capture(
                    &cache.join(format!("{id}.ppm")),
                    icon.width,
                    icon.height,
                    &icon.rgba,
                )
                .unwrap();
            }
        }
    }
}
