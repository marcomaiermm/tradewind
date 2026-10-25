pub mod auction;
mod engine;
pub mod icons;
pub use crate::core::auction::{
    Catalog as AuctionCatalog, Demand as AuctionDemand, Favorite, Item as AuctionItem, Market,
    Sort as AuctionSort, SortDirection, SortField, Value as AuctionValue,
};

pub use crate::adapters::archives::Backup;
pub use crate::adapters::desktop::{APP_ICON_RGBA, APP_ICON_SIZE, APP_ID};
pub use crate::adapters::desktop::{DesktopEvent, SystemAppearance, TrayHandle};
pub use crate::adapters::desktop::{appearance, system_language};
use crate::adapters::files::{self, Paths};
pub use crate::adapters::files::{Appearance, Preferences, Settings};
pub use crate::core::desktop::{CloseAction, WindowPreferences};
pub use crate::core::{ExportKind, Faction, Game, Realm, RealmFilters, realm_name_and_faction};
use crate::i18n::{Language, Text};
use engine::Engine;
pub use engine::{Action, LogLine, Snapshot, SyncRow};
pub fn prepare_desktop_identity() -> Result<(), Text> {
    crate::adapters::desktop::prepare_identity().map_err(|error| Text::from_error(&error))
}

pub async fn start_desktop(
    sender: tokio::sync::mpsc::UnboundedSender<DesktopEvent>,
    language: Language,
) -> Result<TrayHandle, Text> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::adapters::desktop::start_tray(sender, language),
    )
    .await
    .map_err(|_| msg!(TrayUnavailable))?
    .map_err(|e| Text::from_error(&e))
}
pub async fn save_capture(
    path: std::path::PathBuf,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> Result<(), Text> {
    tokio::task::spawn_blocking(move || {
        crate::adapters::desktop::save_capture(&path, width, height, &rgba)
    })
    .await
    .map_err(|e| Text::from_error(&e.into()))?
    .map_err(|e| Text::from_error(&e))
}
/// The Battle.net launcher's version icons, as SVG documents.
pub async fn version_icons(root: std::path::PathBuf) -> std::collections::BTreeMap<Game, Vec<u8>> {
    tokio::task::spawn_blocking(move || crate::adapters::launcher::version_icons(&root))
        .await
        .unwrap_or_default()
}
pub async fn open_link(url: String) -> Result<(), Text> {
    tokio::task::spawn_blocking(move || crate::adapters::desktop::open(&url))
        .await
        .map_err(|e| Text::from_error(&e.into()))?
        .map_err(|e| Text::from_error(&e))
}
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Coordinator {
    engine: Arc<Mutex<Engine>>,
    icons: Arc<std::sync::Mutex<icons::Worker>>,
    stopping: Arc<AtomicBool>,
}
#[derive(Clone, Debug)]
pub struct Outcome {
    pub snapshot: Snapshot,
    pub settings: Settings,
    /// Increases with every settings write, because outcomes can arrive out of order.
    pub revision: u64,
    pub result: Result<Text, Text>,
}
impl Coordinator {
    pub fn new() -> anyhow::Result<(Self, Settings)> {
        let paths = Paths::new()?;
        let (mut settings, warning) = paths.load_or_recover()?;
        if settings.wow_path.as_os_str().is_empty()
            && let Some(path) = crate::adapters::wow::discover()
        {
            settings.wow_path = path;
        }
        let mut engine = Engine::new(paths, settings.clone())?;
        if let Some(warning) = warning {
            engine.log_error(&warning);
            engine.startup_warning = Some(warning);
        }
        Ok((
            Self {
                engine: Arc::new(Mutex::new(engine)),
                icons: Arc::default(),
                stopping: Arc::default(),
            },
            settings,
        ))
    }
    pub async fn inspect_at(&self, root: std::path::PathBuf) -> Outcome {
        self.dispatch(Action::InspectAt(root)).await
    }
    pub async fn load_icons(&self, request: icons::Request) -> icons::Loaded {
        let worker = self.icons.clone();
        let failure = icons::failed(&request);
        tokio::task::spawn_blocking(move || {
            worker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .load(request)
        })
        .await
        .unwrap_or(failure)
    }
    pub async fn tick(&self) -> Option<Outcome> {
        if self.stopping.load(Ordering::SeqCst) {
            return None;
        }
        let Ok(mut engine) = self.engine.try_lock() else {
            return None;
        };
        if self.stopping.load(Ordering::SeqCst) {
            return None;
        }
        let action = if engine.sync_due() {
            Action::Sync
        } else if engine.backup_check_due() {
            Action::ScheduledBackup
        } else {
            return None;
        };
        Some(Self::execute(&mut engine, action).await)
    }
    pub async fn dispatch(&self, action: Action) -> Outcome {
        // One queue serializes manual and scheduled mutations across coordinator clones.
        let mut engine = self.engine.lock().await;
        if self.stopping.load(Ordering::SeqCst) {
            return Self::outcome(&engine, Err(msg!(ClosingApp)));
        }
        Self::execute(&mut engine, action).await
    }

    /// Stop new work and wait for the current foreground or scheduled operation to finish.
    pub async fn shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _engine = self.engine.lock().await;
    }
    fn outcome(engine: &Engine, result: Result<Text, Text>) -> Outcome {
        Outcome {
            snapshot: engine.snapshot.clone(),
            settings: engine.settings.clone(),
            revision: engine.revision,
            result,
        }
    }
    async fn execute(engine: &mut Engine, action: Action) -> Outcome {
        engine.snapshot.selected_directory = None;
        engine.record_attempt(&action);
        let chains_sync = action.chains_sync();
        let notifies = action.notifies();
        let result = async {
            // OS-level lease also prevents two app instances from changing the same WoW files.
            let _lease = action
                .writes_wow_files()
                .then(|| files::MutationLease::acquire(&engine.settings.wow_path))
                .transpose()?;
            let message = engine.run(action).await?;
            if !(chains_sync && engine.snapshot.logged_in) {
                return Ok(message);
            }
            let sync = match files::MutationLease::acquire(&engine.settings.wow_path) {
                Ok(_lease) => engine.run(Action::Sync).await,
                Err(error) => Err(error),
            };
            Ok(match sync {
                Ok(sync) => msg!(SyncJoined, message = message, sync = sync),
                Err(e) => {
                    let error = Text::from_error(&e);
                    engine.log_error(&error);
                    msg!(SyncIncomplete, message = message, error = error)
                }
            })
        }
        .await
        .map_err(|e: anyhow::Error| Text::from_error(&e));
        if let Err(error) = &result {
            engine.log_error(error);
        }
        engine.refresh_account_state();
        if notifies && engine.settings.notifications {
            crate::adapters::desktop::notify(
                &result
                    .as_ref()
                    .unwrap_or_else(|e| e)
                    .render(engine.settings.language.resolve(system_language())),
            );
        }
        Self::outcome(engine, result)
    }
    #[cfg(test)]
    pub(crate) fn with_engine(engine: Engine) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            icons: Default::default(),
            stopping: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_waits_for_current_work_and_prevents_further_writes() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::in_dir(root.path());
        let settings = Settings {
            wow_path: root.path().into(),
            notifications: false,
            ..Default::default()
        };
        let coordinator = Coordinator::with_engine(Engine::new(paths, settings.clone()).unwrap());
        let in_progress = coordinator.engine.lock().await;
        let mut shutdown = std::pin::pin!(coordinator.shutdown());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut shutdown)
                .await
                .is_err()
        );
        assert!(coordinator.stopping.load(Ordering::SeqCst));
        assert!(coordinator.tick().await.is_none());
        let mut queued_save =
            std::pin::pin!(coordinator.dispatch(Action::Save(settings.preferences())));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut queued_save)
                .await
                .is_err()
        );
        drop(in_progress);
        shutdown.await;
        assert!(queued_save.await.result.is_err());
        assert!(
            coordinator
                .dispatch(Action::Bootstrap)
                .await
                .result
                .is_err()
        );
        assert!(!root.path().join("config").exists());
        assert!(!root.path().join("data").exists());
    }

    #[tokio::test]
    async fn coordinator_waits_for_queue_and_rejects_a_second_instance_before_writes() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("_retail_")).unwrap();
        let settings = Settings {
            wow_path: root.path().into(),
            notifications: false,
            ..Default::default()
        };
        let paths = Paths::in_dir(root.path());
        let coordinator = Coordinator::with_engine(Engine::new(paths, settings).unwrap());
        let guard = coordinator.engine.lock().await;
        let queued = coordinator.clone();
        let mut task = tokio::spawn(async move { queued.dispatch(Action::Bootstrap).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut task)
                .await
                .is_err()
        );
        assert!(coordinator.tick().await.is_none());
        drop(guard);
        assert!(task.await.unwrap().result.is_ok());
        let lease = files::MutationLease::acquire(root.path()).unwrap();
        let outcome = coordinator.dispatch(Action::Backup).await;
        assert!(
            outcome
                .result
                .unwrap_err()
                .render(Language::German)
                .contains("andere Tradewind-Instanz")
        );
        assert!(!root.path().join("data/backups").exists());
        drop(lease);
    }
}
