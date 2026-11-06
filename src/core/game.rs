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
    /// Stable name in settings, market and favorite keys.
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
    /// Realm names end in `-Alliance`/`-Horde` (all versions but retail).
    pub fn faction_realms(self) -> bool {
        self != Self::Retail
    }
    /// The realm API's `gameVersion` as game and AppData region-key prefix. `None` for
    /// versions without a key (e.g. Season of Mastery).
    pub fn from_version(version: &str) -> Option<(Self, &'static str)> {
        Some(match version {
            "Retail" => (Self::Retail, ""),
            "Wrath" => (Self::Progression, "BCC-"),
            "Classic Era" => (Self::ClassicEra, "Classic-"),
            "Classic Era - Hardcore" => (Self::ClassicEra, "HC-"),
            "Season of Discovery" => (Self::ClassicEra, "SoD-"),
            "Classic Era - Fresh" => (Self::Anniversary, "Fresh-"),
            "Forever" => (Self::Forever, "Forever-"),
            _ => return None,
        })
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
        assert_eq!(Game::from_version("Retail"), Some((Game::Retail, "")));
        assert_eq!(
            Game::from_version("Wrath"),
            Some((Game::Progression, "BCC-"))
        );
        assert_eq!(
            Game::from_version("Classic Era - Hardcore"),
            Some((Game::ClassicEra, "HC-"))
        );
        assert_eq!(
            Game::from_version("Season of Discovery"),
            Some((Game::ClassicEra, "SoD-"))
        );
        assert_eq!(
            Game::from_version("Classic Era - Fresh"),
            Some((Game::Anniversary, "Fresh-"))
        );
        assert_eq!(
            Game::from_version("Forever"),
            Some((Game::Forever, "Forever-"))
        );
        assert_eq!(Game::from_version("Season of Mastery"), None);
    }
}
