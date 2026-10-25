//! Window preferences and tray fallback rules, independent of the window system.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPreferences {
    pub close_to_tray: bool,
    pub minimize_to_tray: bool,
    pub start_in_tray: bool,
}

impl Default for WindowPreferences {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            minimize_to_tray: false,
            start_in_tray: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    HideToTray,
    Quit,
}

impl WindowPreferences {
    pub fn on_close(self, tray_available: bool) -> CloseAction {
        if self.close_to_tray && tray_available {
            CloseAction::HideToTray
        } else {
            CloseAction::Quit
        }
    }

    pub fn hide_minimized(self, minimized: Option<bool>, tray_available: bool) -> bool {
        self.minimize_to_tray && minimized == Some(true) && tray_available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_and_minimize_are_independent_and_require_a_tray() {
        for close_to_tray in [false, true] {
            for minimize_to_tray in [false, true] {
                for tray in [false, true] {
                    let preferences = WindowPreferences {
                        close_to_tray,
                        minimize_to_tray,
                        start_in_tray: false,
                    };
                    assert_eq!(
                        preferences.on_close(tray),
                        if close_to_tray && tray {
                            CloseAction::HideToTray
                        } else {
                            CloseAction::Quit
                        }
                    );
                    for state in [None, Some(false), Some(true)] {
                        assert_eq!(
                            preferences.hide_minimized(state, tray),
                            minimize_to_tray && tray && state == Some(true)
                        );
                    }
                }
            }
        }
    }
}
