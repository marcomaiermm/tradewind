//! Version icons from the Battle.net launcher's own cache, read in place. Nothing is bundled.
//! The launcher keeps a manifest that names each version's play-selector icon by content hash.
use crate::core::Game;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The launcher's icon key for each version.
fn icon_key(game: Game) -> &'static str {
    match game {
        Game::Retail => "WOW_HUB_PLAY_SELECTOR_ICON_MAINLINE",
        Game::Progression => "WOW_HUB_PLAY_SELECTOR_ICON_CLASSIC_MOP",
        Game::ClassicEra => "WOW_HUB_PLAY_SELECTOR_ICON_CLASSIC_ERA",
        Game::Anniversary => "WOW_HUB_PLAY_SELECTOR_ICON_CLASSIC_BC_ANNIVERSARY",
        Game::Forever => "WOW_HUB_PLAY_SELECTOR_ICON_FOREVER",
    }
}

/// SVG documents per version, empty when no launcher cache is found.
pub fn version_icons(wow_root: &Path) -> BTreeMap<Game, Vec<u8>> {
    icons_from(caches(wow_root))
}

fn icons_from(caches: Vec<PathBuf>) -> BTreeMap<Game, Vec<u8>> {
    let Some((cache, files)) = caches
        .into_iter()
        .filter_map(|cache| newest_manifest(&cache).map(|found| (cache, found)))
        .max_by_key(|(_, (modified, _))| *modified)
        .map(|(cache, (_, files))| (cache, files))
    else {
        return BTreeMap::new();
    };
    Game::ALL
        .into_iter()
        .filter_map(|game| {
            let hash = files
                .get(&format!("world_of_warcraft#{}", icon_key(game)))?
                .get("hash")?
                .as_str()?;
            let svg = fs::read(cached(&cache, hash)?).ok()?;
            svg.starts_with(b"<svg").then_some((game, svg))
        })
        .collect()
}

fn cached(cache: &Path, hash: &str) -> Option<PathBuf> {
    (hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| cache.join(&hash[..2]).join(&hash[2..4]).join(hash))
}

/// The launcher caches every manifest revision, and the newest one with the icons wins.
fn newest_manifest(cache: &Path) -> Option<(SystemTime, serde_json::Map<String, Value>)> {
    let mut newest = None;
    for file in fs::read_dir(cache)
        .ok()?
        .flatten()
        .flat_map(|a| fs::read_dir(a.path()).into_iter().flatten().flatten())
        .flat_map(|b| fs::read_dir(b.path()).into_iter().flatten().flatten())
    {
        let Ok(meta) = file.metadata() else { continue };
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if meta.len() > 1 << 20 || newest.as_ref().is_some_and(|(at, _)| *at >= modified) {
            continue;
        }
        let Ok(data) = fs::read(file.path()) else {
            continue;
        };
        let marker = icon_key(Game::Retail).as_bytes();
        if !data.starts_with(b"{") || !data.windows(marker.len()).any(|w| w == marker) {
            continue;
        }
        let Ok(mut manifest) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
        if let Some(Value::Object(files)) = manifest.pointer_mut("/files/default").map(Value::take)
        {
            newest = Some((modified, files));
        }
    }
    newest
}

/// Battle.net caches inside Wine prefixes: the one holding WoW, `$WINEPREFIX`,
/// `~/.wine`, Steam's compatdata and launcher folders such as Lutris or Heroic under `~/Games`.
fn caches(wow_root: &Path) -> Vec<PathBuf> {
    let mut prefixes: Vec<PathBuf> = wow_root
        .ancestors()
        .filter(|dir| dir.join("drive_c").is_dir())
        .map(Path::to_owned)
        .collect();
    prefixes.extend(std::env::var_os("WINEPREFIX").map(PathBuf::from));
    if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()) {
        prefixes.push(home.join(".wine"));
        let mut found = vec![];
        find_prefixes(&home.join("Games"), 5, &mut found);
        find_prefixes(
            &home.join(".local/share/Steam/steamapps/compatdata"),
            2,
            &mut found,
        );
        prefixes.extend(found);
    }
    let mut caches = vec![];
    for prefix in prefixes {
        let Ok(users) = fs::read_dir(prefix.join("drive_c/users")) else {
            continue;
        };
        for user in users.flatten() {
            let cache = user.path().join("AppData/Local/Battle.net/Cache");
            if cache.is_dir() && !caches.contains(&cache) {
                caches.push(cache);
            }
        }
    }
    caches
}

fn find_prefixes(dir: &Path, depth: u8, found: &mut Vec<PathBuf>) {
    if dir.join("drive_c").is_dir() {
        found.push(dir.to_owned());
    }
    let Some(depth) = depth.checked_sub(1) else {
        return;
    };
    // Prefixes nest (Heroic keeps Battle.net's inside its default prefix).
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && name != "drive_c"
            && !name.to_string_lossy().starts_with('.')
        {
            find_prefixes(&entry.path(), depth, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_come_from_the_newest_manifest_in_a_prefix() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        let cache = prefix.join("drive_c/users/me/AppData/Local/Battle.net/Cache");
        let wow = prefix.join("drive_c/Program Files/World of Warcraft");
        fs::create_dir_all(&wow).unwrap();
        let store = |hash: &str, data: &str| {
            let path = cached(&cache, hash).unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        };
        let manifest = |hash: &str| {
            format!(
                r#"{{"files":{{"default":{{"world_of_warcraft#WOW_HUB_PLAY_SELECTOR_ICON_MAINLINE":{{"hash":"{hash}"}}}}}}}}"#
            )
        };
        let (old, new) = ("a".repeat(32), "b".repeat(32));
        store(&old, "<svg>old</svg>");
        store(&new, "<svg>new</svg>");
        store(&"c".repeat(32), &manifest(&old));
        std::thread::sleep(std::time::Duration::from_millis(20));
        store(&"d".repeat(32), &manifest(&new));
        assert_eq!(caches(&wow)[0], cache);
        let icons = icons_from(vec![cache.clone()]);
        assert_eq!(icons.len(), 1);
        assert_eq!(icons[&Game::Retail], b"<svg>new</svg>");
        assert!(cached(&cache, "../../etc/passwd").is_none());
    }
}
