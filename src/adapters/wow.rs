use crate::{
    adapters::{
        files::{atomic_write, within},
        lua,
    },
    core::{Addon, ExportKind, Game},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub game: Game,
    pub path: PathBuf,
}
impl Account {
    pub fn protocol_name(&self) -> String {
        format!("{}{}", self.name, self.game.account_suffix())
    }
    pub fn db(&self, addon: &str) -> Result<Map<String, Value>> {
        let path = self.path.join(format!("{addon}.lua"));
        if !path.exists() {
            return Ok(Map::new());
        }
        lua::parse(&fs::read_to_string(path)?)
    }
    pub fn data(&self, addon: &str) -> Result<Value> {
        Ok(self
            .db(addon)?
            .remove(&format!("{addon}DB"))
            .unwrap_or(Value::Null))
    }
}
#[derive(Clone, Debug, Default)]
pub struct Installation {
    pub games: Vec<Game>,
    pub accounts: Vec<Account>,
    pub addons: Vec<Addon>,
}
pub fn addon_dir(root: &Path, game: Game) -> PathBuf {
    let base = root.join(game.folder()).join("Interface");
    if base.join("AddOns").is_dir() {
        base.join("AddOns")
    } else if base.join("Addons").is_dir() {
        base.join("Addons")
    } else {
        base.join("AddOns")
    }
}
pub fn normalize_root(path: &Path) -> PathBuf {
    if path
        .file_name()
        .and_then(|name| Game::from_folder(&name.to_string_lossy()))
        .is_some()
    {
        path.parent().unwrap_or(path).to_owned()
    } else {
        path.to_owned()
    }
}
pub fn scan(path: &Path) -> Result<Installation> {
    let root = normalize_root(path);
    ensure!(root.is_dir(), msg!(WowNotFound));
    let mut result = Installation::default();
    for game in Game::ALL {
        let base = root.join(game.folder());
        if !base.is_dir() {
            continue;
        }
        result.games.push(game);
        if !game.tsm_supported() {
            continue;
        }
        for name in ["TradeSkillMaster", "TradeSkillMaster_AppHelper"] {
            let path = addon_dir(&root, game)
                .join(name)
                .join(format!("{name}.toc"));
            let version = if path.is_file() {
                Some(
                    fs::read_to_string(&path)?
                        .lines()
                        .find_map(|line| line.strip_prefix("## Version:").map(str::trim))
                        .unwrap_or("")
                        .to_owned(),
                )
            } else {
                None
            };
            let development = version.as_ref().is_some_and(|v| {
                v.contains("@project-version@") || v.contains("@tsm-project-version@")
            });
            result.addons.push(Addon {
                game,
                name: name.into(),
                version,
                development,
            });
        }
        let accounts = base.join("WTF/Account");
        if !accounts.is_dir() {
            continue;
        }
        for entry in fs::read_dir(accounts)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type()?.is_dir()
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '#')
            {
                continue;
            }
            let path = entry.path().join("SavedVariables");
            if path.join("TradeSkillMaster.lua").is_file() {
                result.accounts.push(Account { name, game, path });
            }
        }
    }
    ensure!(!result.games.is_empty(), msg!(NoGameFolders));
    result.accounts.sort_by_key(Account::protocol_name);
    Ok(result)
}
pub fn discover() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    [
        home.join("Games/World of Warcraft"),
        home.join(".wine/drive_c/Program Files (x86)/World of Warcraft"),
        home.join("Games/battlenet/drive_c/Program Files (x86)/World of Warcraft"),
    ]
    .into_iter()
    .find(|path| scan(path).is_ok())
}
pub fn running() -> bool {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                return false;
            }
            let name = fs::read_to_string(entry.path().join("comm"))
                .unwrap_or_default()
                .trim()
                .to_lowercase();
            [
                "wow.exe",
                "wowclassic.exe",
                "wowclassic_t.exe",
                "wow-64.exe",
                "wow",
            ]
            .contains(&name.as_str())
        })
}

#[derive(Clone, Debug, Default)]
pub struct AppData {
    entries: BTreeMap<(String, String), (i64, String)>,
}
impl AppData {
    pub(crate) fn records(&self) -> impl Iterator<Item = (&str, &str, i64, &str)> {
        self.entries.iter().map(|((kind, scope), (time, body))| {
            (kind.as_str(), scope.as_str(), *time, body.as_str())
        })
    }
    pub fn load(path: &Path) -> Result<Self> {
        let mut data = Self::default();
        if !path.exists() {
            return Ok(data);
        }
        use std::io::Read;
        let mut source = String::new();
        fs::File::open(path)?
            .take(128 * 1024 * 1024 + 1)
            .read_to_string(&mut source)?;
        ensure!(
            source.len() <= 128 * 1024 * 1024,
            msg!(SavedVariablesTooLarge)
        );
        for line in source.lines() {
            if let Some((body, marker)) = line.rsplit_once(" --<") {
                let parts: Vec<_> = marker.trim_end_matches('>').split(',').collect();
                if parts.len() == 3
                    && let Ok(time) = parts[2].parse()
                {
                    data.entries
                        .insert((parts[0].into(), parts[1].into()), (time, body.into()));
                }
            }
        }
        Ok(data)
    }
    pub fn last(&self, kind: &str, realm: &str) -> i64 {
        self.entries
            .get(&(kind.into(), realm.into()))
            .map_or(0, |x| x.0)
    }
    pub fn update(
        &mut self,
        kind: &str,
        realm: &str,
        payload: &str,
        time: i64,
        raw: bool,
    ) -> Result<()> {
        ensure!(
            [
                "AUCTIONDB_NON_COMMODITY_DATA",
                "AUCTIONDB_NON_COMMODITY_SCAN_STAT",
                "AUCTIONDB_NON_COMMODITY_HISTORICAL",
                "AUCTIONDB_COMMODITY_DATA",
                "AUCTIONDB_COMMODITY_SCAN_STAT",
                "AUCTIONDB_COMMODITY_HISTORICAL",
                "AUCTIONDB_REGION_STAT",
                "AUCTIONDB_REGION_HISTORICAL",
                "AUCTIONDB_REGION_SALE",
                "SHOPPING_SEARCHES",
                "APP_INFO"
            ]
            .contains(&kind),
            msg!(UnknownAppData)
        );
        ensure!(
            !realm.contains([',', '\n', '\r', '>']),
            msg!(InvalidRealmName)
        );
        ensure!(!payload.contains(['\n', '\r']), msg!(MultilineAppData));
        let payload = if raw {
            payload.to_owned()
        } else {
            // A long-string delimiter absent from the payload avoids terminating the Lua string.
            let mut eq = String::new();
            while payload.contains(&format!("]{eq}]")) {
                eq.push('=');
            }
            format!("[{eq}[return {payload}]{eq}]")
        };
        let line = format!(
            "select(2, ...).LoadData({},{},{payload})",
            lua::quote(kind),
            lua::quote(realm)
        );
        self.entries
            .insert((kind.into(), realm.into()), (time, line));
        Ok(())
    }
    pub fn save(&self, root: &Path, path: &Path) -> Result<()> {
        within(root, path)?;
        let content: String = self
            .entries
            .iter()
            .map(|((kind, realm), (time, body))| format!("{body} --<{kind},{realm},{time}>\n"))
            .collect();
        atomic_write(path, content.as_bytes())
    }
}

pub fn scopes(db: &Value, scope: &str) -> Vec<String> {
    lua::values(&db["_scopeKeys"][scope])
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}
pub fn active_realms(accounts: &[Account], game: Game) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for account in accounts.iter().filter(|a| a.game == game) {
        let helper = account.data("TradeSkillMaster_AppHelper")?;
        let region = helper["region"]
            .as_str()
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("");
        if region.is_empty() {
            continue;
        }
        for factionrealm in scopes(&account.data("TradeSkillMaster")?, "factionrealm") {
            let parts: Vec<_> = factionrealm.split(" - ").collect();
            result.push((
                region.to_owned(),
                parts.into_iter().rev().collect::<Vec<_>>().join("-"),
            ));
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

/// RFC4180 records. TSM's strings may contain literal backslash-n separators.
pub fn csv(input: &str) -> Result<Vec<Vec<String>>> {
    let input = input.replace("\\n", "\n");
    let mut chars = input.chars().peekable();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' if quoted => {
                quoted = false;
                ensure!(
                    chars.peek().is_none_or(|c| [',', '\n', '\r'].contains(c)),
                    msg!(InvalidCsvString)
                );
            }
            '"' if field.is_empty() => quoted = true,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => (),
            _ => field.push(c),
        }
    }
    ensure!(!quoted, msg!(UnclosedCsvString));
    if !row.is_empty() || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    if let Some(header) = rows.first() {
        ensure!(
            rows.iter().all(|r| r.len() == header.len()),
            msg!(CsvColumns)
        );
    }
    Ok(rows)
}
pub fn csv_records(input: &str) -> Result<Vec<BTreeMap<String, String>>> {
    let rows = csv(input)?;
    let Some(header) = rows.first() else {
        return Ok(Vec::new());
    };
    Ok(rows[1..]
        .iter()
        .map(|row| header.iter().cloned().zip(row.iter().cloned()).collect())
        .collect())
}
pub fn accounting_realms(account: &Account) -> Result<Vec<String>> {
    Ok(scopes(&account.data("TradeSkillMaster")?, "realm"))
}
pub fn modified(account: &Account) -> Result<i64> {
    Ok(fs::metadata(account.path.join("TradeSkillMaster.lua"))?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64)
}
pub fn export_key(kind: ExportKind) -> &'static str {
    match kind {
        ExportKind::Sales => "csvSales",
        ExportKind::Purchases => "csvBuys",
        ExportKind::Income => "csvIncome",
        ExportKind::Expenses => "csvExpense",
        ExportKind::Expired => "csvExpired",
        ExportKind::Canceled => "csvCancelled",
    }
}
pub fn export(
    account: &Account,
    realm: &str,
    kind: ExportKind,
    destination: &Path,
) -> Result<usize> {
    let kind = export_key(kind);
    let globals = account.db("TradeSkillMaster")?;
    let db = globals
        .get("TradeSkillMasterDB")
        .context(msg!(MissingTsmData))?;
    let data = db[format!("r@{realm}@internalData@{kind}")]
        .as_str()
        .context(msg!(NoAccountingData))?;
    let mut rows = csv(data)?;
    ensure!(!rows.is_empty(), msg!(NoTransactions));
    let info = globals.get("TSMItemInfoDB").unwrap_or(&Value::Null);
    let split = |v: &Value| -> Vec<String> {
        if let Some(s) = v.as_str() {
            s.split('\x02').map(str::to_owned).collect()
        } else {
            lua::values(v)
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .flat_map(|s| s.split('\x02').map(str::to_owned).collect::<Vec<_>>())
                .collect()
        }
    };
    let lookup: BTreeMap<_, _> = split(&info["itemStrings"])
        .into_iter()
        .zip(split(&info["names"]))
        .collect();
    if let Some(index) = rows[0].iter().position(|s| s == "itemString") {
        rows[0].insert(index + 1, "itemName".into());
        for row in &mut rows[1..] {
            row.insert(
                index + 1,
                lookup
                    .get(&row[index])
                    .cloned()
                    .unwrap_or_else(|| "?".into()),
            );
        }
    }
    let output: String = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|s| {
                    // Prevent spreadsheet formulas when exporting user-authored character/item names.
                    let s = if s.starts_with(['=', '+', '-', '@']) {
                        format!("'{s}")
                    } else {
                        s.clone()
                    };
                    if s.contains([',', '"', '\n', '\r']) {
                        format!("\"{}\"", s.replace('"', "\"\""))
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
                + "\r\n"
        })
        .collect();
    atomic_write(destination, output.as_bytes())?;
    Ok(rows.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_roundtrip_and_appdata_preservation() {
        assert_eq!(
            csv("itemString,name\\ni:1,\"a,b\"\\ni:2,\"a\"\"b\"").unwrap()[2][1],
            "a\"b"
        );
        assert!(csv("a,b\n1").is_err());
        assert!(csv("a\n\"unterminated").is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AppData.lua");
        let mut data = AppData::default();
        data.update("SHOPPING_SEARCHES", "Realm", "{text=\"]]\"}", 42, false)
            .unwrap();
        data.save(dir.path(), &path).unwrap();
        let data = AppData::load(&path).unwrap();
        assert_eq!(data.last("SHOPPING_SEARCHES", "Realm"), 42);
        assert!(fs::read_to_string(path).unwrap().contains("[=[return"));
    }
    #[test]
    fn games_without_tsm_support_are_listed_but_never_scanned() {
        let root = tempfile::tempdir().unwrap();
        for game in [Game::Retail, Game::Forever] {
            let saved = root
                .path()
                .join(game.folder())
                .join("WTF/Account/ACCOUNT/SavedVariables");
            fs::create_dir_all(&saved).unwrap();
            fs::write(saved.join("TradeSkillMaster.lua"), "").unwrap();
        }
        let found = scan(root.path()).unwrap();
        assert_eq!(found.games, [Game::Retail, Game::Forever]);
        assert!(found.accounts.iter().all(|a| a.game == Game::Retail));
        assert!(found.addons.iter().all(|a| a.game == Game::Retail));
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn accounting_csv_fixture_and_appdata_update() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("TradeSkillMaster.lua"),
            include_str!("../../tests/fixtures/savedvariables.lua"),
        )
        .unwrap();
        let account = Account {
            name: "ACCOUNT".into(),
            game: Game::Retail,
            path: root.path().into(),
        };
        let path = root.path().join("sales.csv");
        assert_eq!(
            export(&account, "Test Realm", ExportKind::Sales, &path).unwrap(),
            3
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            include_bytes!("../../tests/fixtures/accounting-sales.csv")
        );
        for kind in ExportKind::ALL {
            assert!(export(&account, "Test Realm", kind, &path).is_ok());
        }
        let path = root.path().join("AppData.lua");
        fs::write(&path, include_str!("../../tests/fixtures/appdata.lua")).unwrap();
        let mut data = AppData::load(&path).unwrap();
        assert_eq!(data.last("AUCTIONDB_REGION_STAT", "EU"), 1700000000);
        data.update(
            "AUCTIONDB_NON_COMMODITY_DATA",
            "Test Realm",
            "{price=1}",
            1700000002,
            false,
        )
        .unwrap();
        data.save(root.path(), &path).unwrap();
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("[[return {price=1}]]"));
        assert!(content.contains("[[return {value=42}]]"));
        assert!(data.update("APP_INFO", "bad,realm", "{}", 1, true).is_err());
    }
}
