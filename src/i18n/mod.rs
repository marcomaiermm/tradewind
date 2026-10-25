//! Pure, typed English/German messages. No process-global locale or protocol translation.
mod catalog;
pub use catalog::Key;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}
impl Language {
    pub fn from_locale(locale: &str) -> Self {
        if locale
            .split(['_', '-', '.', '@'])
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case("de"))
        {
            Self::German
        } else {
            Self::English
        }
    }
    pub fn text(self, key: Key) -> &'static str {
        key.template(self, false)
    }
    pub fn number(self, value: u64) -> String {
        let digits = value.to_string();
        let mut out = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(if self == Self::German { '.' } else { ',' });
            }
            out.push(ch);
        }
        out
    }
    pub fn timestamp(self, timestamp: i64, full: bool) -> String {
        chrono::DateTime::from_timestamp(timestamp, 0)
            .filter(|_| timestamp > 0)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format(match (self, full) {
                        (Self::German, true) => "%d.%m.%Y · %H:%M",
                        (Self::German, false) => "%d.%m. · %H:%M",
                        (Self::English, true) => "%Y-%m-%d · %H:%M",
                        (Self::English, false) => "%b %d · %H:%M",
                    })
                    .to_string()
            })
            .unwrap_or_else(|| self.text(Key::NoData).into())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePreference {
    #[default]
    System,
    English,
    German,
}
impl LanguagePreference {
    pub fn resolve(self, system: Language) -> Language {
        match self {
            Self::System => system,
            Self::English => Language::English,
            Self::German => Language::German,
        }
    }
    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::System => language.text(Key::System),
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }
}

/// Remains structured through application outcomes, errors and activity entries so language
/// changes also translate existing messages. Literal arguments (paths, names, server data)
/// are never interpreted as templates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    Message(Key, Vec<(&'static str, Text)>),
    Literal(String),
    Number(u64),
}
impl Text {
    pub fn render(&self, language: Language) -> String {
        match self {
            Self::Literal(value) => value.clone(),
            Self::Number(value) => language.number(*value),
            Self::Message(key, args) => {
                let plural = args.iter().any(|(name, value)| {
                    *name == "count" && matches!(value, Self::Number(n) if *n != 1)
                });
                let mut tail = key.template(language, plural);
                let mut result = String::new();
                while let Some((head, remaining)) = tail.split_once('{') {
                    result.push_str(head);
                    let Some((name, rest)) = remaining.split_once('}') else {
                        break;
                    };
                    if let Some((_, value)) = args.iter().find(|(key, _)| *key == name) {
                        result.push_str(&value.render(language));
                    } else {
                        debug_assert!(false, "Missing message argument {name} for {key:?}");
                        result.push('?');
                    }
                    tail = rest;
                }
                result.push_str(tail);
                result
            }
        }
    }
    /// Connection, DNS, TLS or timeout failure, possibly nested in a transport message.
    pub fn is_network(&self) -> bool {
        use Key::*;
        match self {
            Self::Message(key, args) => {
                matches!(
                    key,
                    TransportTimeout
                        | ConnectionFailed
                        | RequestFailed
                        | ConnectionRefused
                        | ConnectionReset
                        | ConnectionEnded
                        | Unreachable
                        | NetworkBlocked
                        | ConnectionTimeout
                        | CertificateName
                        | CertificateExpired
                        | CertificateUnverified
                        | DnsFailed
                        | Http2Failed
                        | TlsFailed
                ) || args.iter().any(|(_, arg)| arg.is_network())
            }
            _ => false,
        }
    }
    pub fn from_error(error: &anyhow::Error) -> Self {
        // Keep the outermost classified message. Never append raw transport errors or URLs.
        error
            .downcast_ref::<Self>()
            .cloned()
            .unwrap_or_else(|| Self::Literal(error.to_string()))
    }
}
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Language::English))
    }
}
impl std::error::Error for Text {}
impl From<Key> for Text {
    fn from(key: Key) -> Self {
        Self::Message(key, vec![])
    }
}
impl From<String> for Text {
    fn from(value: String) -> Self {
        Self::Literal(value)
    }
}
impl From<&str> for Text {
    fn from(value: &str) -> Self {
        Self::Literal(value.into())
    }
}
impl From<&String> for Text {
    fn from(value: &String) -> Self {
        Self::Literal(value.clone())
    }
}
impl From<&Text> for Text {
    fn from(value: &Text) -> Self {
        value.clone()
    }
}
impl From<usize> for Text {
    fn from(value: usize) -> Self {
        Self::Number(value as u64)
    }
}
impl From<u64> for Text {
    fn from(value: u64) -> Self {
        Self::Number(value)
    }
}

#[macro_export]
macro_rules! msg {
    ($key:ident $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::Text::Message($crate::i18n::Key::$key, vec![$((stringify!($name), $crate::i18n::Text::from($value))),*])
    };
}

/// Resolve Linux message locale at the desktop boundary. LANGUAGE is a preference
/// list, and C/POSIX intentionally disables it. Unknown locales fall back to English.
pub fn system_language(base: &str, preferences: Option<&str>) -> Language {
    if base == "C" || base == "POSIX" || base.starts_with("C.") {
        return Language::English;
    }
    if let Some(preferences) = preferences {
        for locale in preferences.split(':') {
            let tag = locale
                .split(['_', '-', '.', '@'])
                .next()
                .unwrap_or_default();
            if tag.eq_ignore_ascii_case("de") || tag.eq_ignore_ascii_case("en") {
                return Language::from_locale(locale);
            }
        }
    }
    Language::from_locale(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    fn placeholders(template: &str) -> BTreeSet<&str> {
        template
            .split('{')
            .skip(1)
            .map(|s| s.split_once('}').expect("closed placeholder").0)
            .collect()
    }
    #[test]
    fn all_catalog_entries_have_matching_placeholders_and_plural_forms() {
        for key in Key::ALL {
            let expected = placeholders(key.template(Language::English, false));
            for language in [Language::English, Language::German] {
                for many in [false, true] {
                    let template = key.template(language, many);
                    assert!(!template.is_empty(), "{key:?}");
                    assert_eq!(placeholders(template), expected, "{key:?}");
                }
            }
        }
    }
    #[test]
    fn network_errors_are_detected_through_transport_wrapping() {
        let tls = msg!(
            TransportError,
            operation = "TSM API",
            host = "example.invalid",
            reason = msg!(CertificateUnverified)
        );
        assert!(tls.is_network());
        assert!(!msg!(LoginRejected).is_network());
        assert!(!Text::from("timed out").is_network());
    }
    #[test]
    fn messages_retranslate_nested_errors_plural_counts_and_literal_arguments() {
        let error = anyhow::anyhow!(msg!(CertificateUnverified)).context(msg!(
            TransportError,
            operation = "TSM API",
            host = "example.invalid",
            reason = msg!(CertificateUnverified)
        ));
        let text = Text::from_error(&error);
        assert_eq!(
            text.render(Language::English),
            "TSM API (example.invalid): The TLS certificate could not be verified."
        );
        assert_eq!(
            text.render(Language::German),
            "TSM API (example.invalid): Das TLS-Zertifikat konnte nicht überprüft werden."
        );
        for (count, en, de) in [
            (0_usize, "0 matches", "0 Treffer"),
            (1, "1 match", "1 Treffer"),
            (2, "2 matches", "2 Treffer"),
            (1234, "1,234 matches", "1.234 Treffer"),
        ] {
            assert_eq!(msg!(Matches, count = count).render(Language::English), en);
            assert_eq!(msg!(Matches, count = count).render(Language::German), de);
        }
        let name = "Étoile-{count}-Alliance";
        assert_eq!(
            msg!(RealmAlreadyAdded, name = name).render(Language::English),
            "Étoile-{count}-Alliance has already been added."
        );
        assert_eq!(
            msg!(RealmAlreadyAdded, name = name).render(Language::German),
            "Étoile-{count}-Alliance ist bereits hinzugefügt."
        );
    }
    #[test]
    fn locale_selection_is_deterministic_with_english_fallback() {
        for (base, prefs, expected) in [
            ("de_DE.UTF-8", None, Language::German),
            ("de-AT", None, Language::German),
            ("en_GB.UTF-8", None, Language::English),
            ("fr_FR.UTF-8", None, Language::English),
            ("en_US.UTF-8", Some("fr:de:en"), Language::German),
            ("C", Some("de"), Language::English),
            ("C.UTF-8", Some("de"), Language::English),
            ("POSIX", Some("de"), Language::English),
        ] {
            assert_eq!(system_language(base, prefs), expected);
        }
        assert_eq!(
            LanguagePreference::German.resolve(Language::English),
            Language::German
        );
        assert_eq!(
            LanguagePreference::English.resolve(Language::German),
            Language::English
        );
        assert_eq!(Language::German.timestamp(0, true), "Noch keine Daten");
        let timestamp = 1_700_000_000;
        assert!(
            Language::German
                .timestamp(timestamp, true)
                .contains(".2023")
        );
        assert!(
            Language::English
                .timestamp(timestamp, true)
                .starts_with("2023-")
        );
    }
}
