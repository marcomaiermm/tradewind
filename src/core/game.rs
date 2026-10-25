//! WoW game versions. One type for the folder, protocol and display names, so adding a
//! version is a change here plus the exhaustive matches the compiler then points at.
use serde::{Deserialize, Serialize};

/// Serialized with TSM's protocol names, which settings and realm lists already use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Game {
    #[serde(rename = "retail")]
    Retail,
    #[serde(rename = "bcc")]
    Progression,
    #[serde(rename = "classic")]
    ClassicEra,
    #[serde(rename = "anniversary")]
    Anniversary,
    #[serde(rename = "forever")]
    Forever,
}
impl Game {
    pub const ALL: [Self; 5] = [
        Self::Retail,
        Self::Progression,
        Self::ClassicEra,
        Self::Anniversary,
        Self::Forever,
    ];
    /// Directory below the WoW root.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Retail => "_retail_",
            Self::Progression => "_classic_",
            Self::ClassicEra => "_classic_era_",
            Self::Anniversary => "_anniversary_",
            Self::Forever => "_forever_",
        }
    }
    /// TSM's API name (`realms2/add/<protocol>/…`, market and favorite keys).
    pub fn protocol(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::Progression => "bcc",
            Self::ClassicEra => "classic",
            Self::Anniversary => "anniversary",
            Self::Forever => "forever",
        }
    }
    pub fn from_folder(folder: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|game| game.folder() == folder)
    }
    pub fn from_protocol(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|game| game.protocol() == name)
    }
    /// The one name users see for this version, in badges, lists and messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Retail => "Midnight",
            Self::Progression => "MoP Classic",
            Self::ClassicEra => "Classic Era",
            Self::Anniversary => "TBC Anniversary",
            Self::Forever => "WoW: Forever",
        }
    }
    /// Appended to WTF account names in TSM's account, backup and addon names.
    pub fn account_suffix(self) -> &'static str {
        match self {
            Self::Retail | Self::Forever => "",
            Self::Progression => "-Progression",
            Self::ClassicEra => "-Classic",
            Self::Anniversary => "-Anniversary",
        }
    }
    /// TSM has no addon, accounts or price data for it yet: listed, but never scanned.
    pub fn tsm_supported(self) -> bool {
        self != Self::Forever
    }
    /// Realms are subscribed one by one through the TSM account. The other versions sync
    /// whatever realms the local SavedVariables mention.
    pub fn subscribable(self) -> bool {
        matches!(self, Self::Retail | Self::Progression)
    }
    /// Realm names end in `-Alliance`/`-Horde` (all versions but retail).
    pub fn faction_realms(self) -> bool {
        self != Self::Retail
    }
    /// The status response's realm and region list keys.
    pub fn status_keys(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Retail => Some(("realms", "regions")),
            Self::Progression => Some(("realms-Progression", "regions-Progression")),
            Self::ClassicEra => Some(("extraClassicRealms", "extraClassicRegions")),
            Self::Anniversary => Some(("extraAnniversaryRealms", "extraAnniversaryRegions")),
            Self::Forever => None,
        }
    }
}
impl std::fmt::Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_vocabulary_round_trips_and_stays_compatible_with_saved_settings() {
        for game in Game::ALL {
            assert_eq!(Game::from_folder(game.folder()), Some(game));
            assert_eq!(Game::from_protocol(game.protocol()), Some(game));
            let json = serde_json::to_string(&game).unwrap();
            assert_eq!(json, format!("\"{}\"", game.protocol()));
            assert_eq!(serde_json::from_str::<Game>(&json).unwrap(), game);
        }
        assert_eq!(Game::from_folder("_ptr_"), None);
        assert!(Game::Forever.status_keys().is_none() && !Game::Forever.tsm_supported());
    }
}
