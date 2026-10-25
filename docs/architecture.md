# Architecture

Tradewind is a modular monolith in **one Rust crate** with a light hexagonal
split. There is no workspace of sub-crates and no generic plugin, repository or
event-bus layer.

| Module | Responsibility | Boundary |
|---|---|---|
| `core` | Game versions (`Game`), backup identity, realm/addon rules, time and cursor decisions, accounting model, export kinds, auction filters and sorting | No Iced, HTTP, filesystem or system clock. Time values are passed in |
| `application` | Actions, outcomes, snapshots, use cases and the central `Coordinator` | All business decisions outside the UI |
| `adapters` | TSM/OAuth/HTTP, WoW/Lua, uploads, archives, XDG storage, keyring, desktop, CASC icons | External formats, IO, atomic writes, archive validation |
| `i18n` | Typed English/German messages and pure formatting | No global language, no IO |
| `ui` | Iced state, forms, navigation, rendering | Sends actions, renders outcomes. Never touches network, Lua, archives or files directly |

## Coordinator

`Coordinator` is the only entry point for business actions. A Tokio mutex
serializes manual and scheduled work across clones. Timers do not enqueue more
work while the queue is busy. A non-blocking file lock at
`<WoW>/.tradewind.lock` additionally guards against a second client instance.
The lock file is never deleted (deleting it while held would allow two owners
on different inodes) and symlinked lock files are rejected.

`Coordinator::shutdown` blocks new actions and timer work, then waits for the
same mutex, so a running sync, backup or restore always finishes before the
process exits. **Quit** in the tray menu always takes this path. The window's
close button does too when close-to-tray is off or no tray is available.

Price sync runs every ten minutes while signed in with sync enabled. The local
backup check has its own schedule and does not require a login. Failed attempts
also advance the schedule to avoid hot retry loops. WoW itself does not honour
the lock, so restore and addon installation additionally check for running WoW
processes.

## Desktop integration

`ui::desktop` uses Iced's daemon runtime: moving to the tray closes only the
window surface while application state and coordinator stay alive. The tray
opens a new surface with the same state. This avoids `Mode::Hidden`, whose
visibility toggle is not implemented on Wayland. Without a reachable tray the
window is never hidden. If the tray disappears, the window reopens.

Start-in-tray waits at most five seconds for tray registration and falls back to
a normal window. Minimize detection is queried from the real Iced window. On
Wayland it returns `None` and the option is disabled with an explanation.

Tray events and the desktop's dark/reduced-motion preferences (XDG portal via
`mundy`) arrive as Iced subscriptions. Iced's own theme detection is not used:
it follows winit's window theme, which is empty on Wayland.

The settings page edits a `Preferences` draft and saves it on every change.
Only the saved `Settings` drive behaviour. Each save bumps a revision so an
older outcome (e.g. a scheduled sync that started before the save) cannot
overwrite newer settings.

Before the first window opens, `application::prepare_desktop_identity` writes a
hidden XDG desktop entry with an absolute icon path for the shared app ID, so
Wayland compositors can resolve the icon without installation. Installed or
user-provided entries (anywhere in `XDG_DATA_DIRS`) take precedence and are
never touched.

## File safety

`adapters/files.rs` writes atomically: temp file in the target directory,
`sync_all`, rename, directory sync. Targets inside WoW are checked to belong to
the selected installation. Parent traversal, symlinked targets and paths
outside the installation are rejected.

`adapters/archives.rs` is the single archive boundary. It rejects traversal,
links and special files, duplicate entries, wildcard patterns and invalid file
names, and limits size and entry count. Restore validates every Lua file and
the TSM main table **before** creating the safety backup and replacing anything.

Addon installation keeps existing SavedVariables in `backups/keep`, stages the
new addon, preserves existing AppData and rolls back on rename failures. A
multi-file restore is not a filesystem transaction: on a later write error some
files may already be replaced, while the previous data stays in the safety
backup. Safety backups are never deleted automatically.

## Versions

`APP_VERSION` comes from Cargo. `adapters::tsm::PROTOCOL_VERSION = 41402` is the
legacy wire contract and `APP_INFO` compatibility version. Releasing this app
does not change it.

## Internationalization

`src/i18n/catalog.rs` is the single source of UI strings. Every typed key
declares English and German together. Static labels use
`language.text(Key::…)`. Messages with data use `msg!(Key, name = value)` and
are rendered with `render(language)` at display time, so they can be
re-rendered after a language switch. Arguments are substituted in one pass
(braces in realm names or paths are never re-expanded). Counts use explicit
singular/plural forms.

*System* resolves the first non-empty locale from `LC_ALL`, `LC_MESSAGES` and
`LANG` (`LANGUAGE` may list preferences). `C`/`POSIX` and unsupported languages
fall back to English. Stored values, protocol names, realm IDs and CSV columns
are never localized, and the technical log is always English.

## Colors

Game version colors (Midnight `#7B5BC8`, MoP Classic `#3D9B78`, Classic Era
`#C69B57`, TBC Anniversary `#8BAF3C`, WoW: Forever `#016787`) are UI accents inspired by the logos, not
official brand colors. Faction colors come from `PLAYER_FACTION_COLORS` in the
WoW UI source (Alliance `#4954E8`, Horde `#E50C11`). Item quality colors follow
the standard WoW palette. Text colors are adjusted per theme to at least 4.5:1
contrast. The values live in `src/ui/identity.rs` and `src/ui/theme.rs`.

## Testing

`core` edge cases and the fixtures in [`tests/fixtures`](../tests/fixtures)
cover the reconstructed formats. Coordinator tests check serialization,
instance locking and offline backups. UI tests render real Iced/tiny-skia
frames offscreen (no compositor needed) to check layout, clipping and contrast.
Fixtures are not a substitute for live compatibility checks.

Optional, read-only probes against a real installation:

```sh
# Inspect an installation (read-only)
cargo run --release -- --inspect "/path/to/World of Warcraft"
# Render all views to PPM images without a desktop session
TSM_CAPTURE_DIR=/tmp/tsm-captures cargo test render_native_views_without_compositor -- --ignored
# Parse the local AuctionDB snapshot / decode local item icons
TSM_AUCTION_WOW_PATH="/path/to/World of Warcraft" cargo test local_auction_snapshot -- --ignored --nocapture
TSM_AUCTION_WOW_PATH="/path/to/World of Warcraft" TSM_ICON_PROBE_DIR=/tmp/tsm-icons \
  cargo test local_icons_from_installed_wow -- --ignored --nocapture
```
