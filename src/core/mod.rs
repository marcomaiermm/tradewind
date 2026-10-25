//! Domain models and rules. No UI, network, paths or filesystem operations.
pub mod auction;
pub mod desktop;
mod game;
use anyhow::{Result, ensure};
pub use game::Game;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupId {
    pub account: String,
    pub system_id: String,
    pub timestamp: i64,
}
impl BackupId {
    pub fn validate(&self) -> Result<()> {
        ensure!(valid_account(&self.account), msg!(InvalidBackupAccount));
        ensure!(
            !self.system_id.is_empty()
                && self
                    .system_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            msg!(InvalidSystemId)
        );
        ensure!(self.timestamp > 0, msg!(InvalidBackupTime));
        Ok(())
    }
}
/// Older TSM builds named progression accounts `-BCC`.
const LEGACY_PROGRESSION_SUFFIX: &str = "-BCC";
pub fn valid_account(name: &str) -> bool {
    let name = Game::ALL
        .map(Game::account_suffix)
        .into_iter()
        .chain([LEGACY_PROGRESSION_SUFFIX])
        .filter(|suffix| !suffix.is_empty())
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '#')
}
pub fn same_account(left: &str, right: &str) -> bool {
    fn progression_base(name: &str) -> Option<&str> {
        name.strip_suffix(LEGACY_PROGRESSION_SUFFIX)
            .or_else(|| name.strip_suffix(Game::Progression.account_suffix()))
    }
    left == right
        || matches!((progression_base(left),progression_base(right)),(Some(a),Some(b)) if a==b)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Realm {
    pub game: Game,
    pub id: String,
    pub region: String,
    pub name: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Faction {
    Alliance,
    Horde,
}

/// The suffix is part of TSM's protocol identity, so strip it only for display.
pub fn realm_name_and_faction(game: Game, name: &str) -> (&str, Option<Faction>) {
    if game.faction_realms() {
        split_faction(name)
    } else {
        (name, None)
    }
}
/// `Realm-Horde` → (`Realm`, Horde), whatever the version.
pub fn split_faction(name: &str) -> (&str, Option<Faction>) {
    for (suffix, faction) in [("-Alliance", Faction::Alliance), ("-Horde", Faction::Horde)] {
        if let Some(name) = name.strip_suffix(suffix) {
            return (name, Some(faction));
        }
    }
    (name, None)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RealmFilters {
    pub game: Option<Game>,
    pub region: Option<String>,
    pub faction: Option<Faction>,
}
impl RealmFilters {
    pub fn matches(&self, realm: &Realm) -> bool {
        self.game.is_none_or(|game| game == realm.game)
            && self
                .region
                .as_ref()
                .is_none_or(|region| region == &realm.region)
            && self.faction.is_none_or(|faction| {
                realm_name_and_faction(realm.game, &realm.name).1 == Some(faction)
            })
    }
}
impl std::fmt::Display for Realm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · {} · {}", self.game, self.region, self.name)
    }
}
pub fn can_remove_realm(game: Game, region: &str, region_only: bool) -> bool {
    !region_only && game.subscribable() && !["KR", "TW"].contains(&region)
}
#[derive(Clone, Debug)]
pub struct Addon {
    pub game: Game,
    pub name: String,
    pub version: Option<String>,
    pub development: bool,
}
impl Addon {
    pub fn update_available(&self, latest: &str) -> bool {
        !self.development
            && !latest.is_empty()
            && self
                .version
                .as_ref()
                .is_some_and(|version| latest != version)
    }
}
pub fn backup_due(now: i64, last: i64, modified: i64, interval_seconds: i64, force: bool) -> bool {
    force || (modified > last && now.saturating_sub(last) >= interval_seconds)
}
pub fn sync_due(enabled: bool, authenticated: bool, elapsed_seconds: u64) -> bool {
    enabled && authenticated && elapsed_seconds >= 600
}
pub fn backup_check_due(elapsed_seconds: u64, interval_hours: u64) -> bool {
    elapsed_seconds >= interval_hours.saturating_mul(3600)
}
pub fn price_update_due(remote: i64, local: i64) -> bool {
    remote > local
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountingRecord {
    pub item_id: i64,
    pub price: i64,
    pub stack_size: i64,
    pub quantity: i64,
    pub occurred_at: i64,
    pub saved_at: i64,
    pub kind: i64,
}
impl AccountingRecord {
    pub fn newer_than(&self, cursor: i64) -> bool {
        self.saved_at > cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_cover_boundaries() {
        assert!(!price_update_due(42, 42));
        assert!(price_update_due(43, 42));
        assert!(!backup_due(3600, 0, 0, 3600, false));
        assert!(backup_due(3600, 0, 1, 3600, false));
        assert!(!can_remove_realm(Game::Retail, "KR", false));
        assert!(!can_remove_realm(Game::ClassicEra, "EU", false));
        assert!(can_remove_realm(Game::Progression, "EU", false));
        assert!(valid_account("ACCOUNT-BCC") && valid_account("ACCOUNT-Anniversary"));
        assert!(!valid_account("-Classic"));
        assert!(same_account("ACCOUNT-BCC", "ACCOUNT-Progression"));
        assert!(!same_account("ACCOUNT-Classic", "ACCOUNT-Progression"));
        assert!(!sync_due(false, true, 600));
        assert!(!sync_due(true, false, 600));
        assert!(backup_check_due(3600, 1));
        let mut addon = Addon {
            game: Game::Retail,
            name: "TradeSkillMaster".into(),
            version: None,
            development: false,
        };
        assert!(!addon.update_available("4.14.2"));
        addon.version = Some("4.14.1".into());
        assert!(addon.update_available("4.14.2"));
        addon.development = true;
        assert!(!addon.update_available("4.14.2"));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Sales,
    Purchases,
    Income,
    Expenses,
    Expired,
    Canceled,
}
impl ExportKind {
    pub const ALL: [Self; 6] = [
        Self::Sales,
        Self::Purchases,
        Self::Income,
        Self::Expenses,
        Self::Expired,
        Self::Canceled,
    ];
}
impl std::fmt::Display for ExportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sales => "sales",
            Self::Purchases => "purchases",
            Self::Income => "income",
            Self::Expenses => "expenses",
            Self::Expired => "expired",
            Self::Canceled => "canceled",
        })
    }
}

#[cfg(test)]
mod realm_filter_tests {
    use super::*;
    #[test]
    fn fixed_realms_keep_protocol_identity_when_filtered_by_faction_game_and_region() {
        let realms: Vec<Realm> =
            serde_json::from_str(include_str!("../../tests/fixtures/realm-selection.json"))
                .unwrap();
        let filters = RealmFilters {
            game: Some(Game::Progression),
            region: Some("EU".into()),
            faction: Some(Faction::Alliance),
        };
        let matching: Vec<_> = realms
            .iter()
            .filter(|realm| filters.matches(realm))
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].id, "eu-pyre-a");
        assert_eq!(matching[0].name, "Pyrewood Village-Alliance");
        assert_eq!(
            realm_name_and_faction(Game::Retail, "Test-Horde"),
            ("Test-Horde", None)
        );
        assert_eq!(
            realm_name_and_faction(Game::Progression, "Étoile-du-matin"),
            ("Étoile-du-matin", None)
        );
        assert!(RealmFilters::default().matches(&realms[5]));
        let json = serde_json::to_string(&filters).unwrap();
        assert!(json.contains("\"alliance\""));
        assert_eq!(
            serde_json::from_str::<RealmFilters>(&json).unwrap(),
            filters
        );
    }
}
