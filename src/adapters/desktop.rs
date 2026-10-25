use crate::adapters::files as storage;
use crate::i18n::{Key, Language};
use anyhow::{Context, Result};
use ksni::TrayMethods;
mod dialogs;
mod identity;
pub use dialogs::{choose_directory, choose_export};
pub use identity::prepare as prepare_identity;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc::UnboundedSender;

pub const APP_ID: &str = "tradewind";
pub const APP_ICON_SIZE: u32 = 256;
pub const APP_ICON_RGBA: &[u8] = include_bytes!("../../data/icons/app/icon-256.rgba");

pub fn set_autostart(enabled: bool) -> Result<()> {
    let config = directories::BaseDirs::new()
        .context(msg!(MissingXdgConfig))?
        .config_dir()
        .join("autostart/tradewind.desktop");
    if !enabled {
        if config.exists() {
            fs::remove_file(config)?;
        }
        return Ok(());
    }
    let executable = executable()?
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    storage::atomic_write(&config,format!("[Desktop Entry]\nType=Application\nName=Tradewind\nExec=\"{executable}\"\nIcon=tradewind\nTerminal=false\nCategories=Game;\n").as_bytes())
}

/// What a launcher should start: inside an AppImage, the image rather than its temporary mount.
fn executable() -> Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(image) if !image.is_empty() => Ok(image.into()),
        _ => Ok(std::env::current_exe()?),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DesktopEvent {
    Show,
    Sync,
    Quit,
}
pub struct TrayHandle {
    pub online: Arc<AtomicBool>,
    pub handle: ksni::Handle<Tray>,
}
impl std::fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrayHandle")
    }
}
pub struct Tray {
    language: Language,
    sender: UnboundedSender<DesktopEvent>,
    online: Arc<AtomicBool>,
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "tradewind".into()
    }
    fn title(&self) -> String {
        "Tradewind".into()
    }
    fn icon_name(&self) -> String {
        "tradewind".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Supply pixels as well as a theme name, so the tray also works before installation.
        [
            (
                32,
                include_bytes!("../../data/icons/app/icon-32.rgba").as_slice(),
            ),
            (
                48,
                include_bytes!("../../data/icons/app/icon-48.rgba").as_slice(),
            ),
        ]
        .into_iter()
        .map(|(size, rgba)| {
            let mut argb = rgba.to_vec();
            for pixel in argb.as_chunks_mut::<4>().0 {
                pixel.rotate_right(1);
            }
            ksni::Icon {
                width: size,
                height: size,
                data: argb,
            }
        })
        .collect()
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(DesktopEvent::Show);
    }
    fn watcher_online(&self) {
        self.online.store(true, Ordering::SeqCst);
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        self.online.store(false, Ordering::SeqCst);
        let _ = self.sender.send(DesktopEvent::Show);
        true
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.language.text(Key::OpenApp).into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(DesktopEvent::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: self.language.text(Key::SyncNow).into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(DesktopEvent::Sync);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: self.language.text(Key::Quit).into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(DesktopEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}
pub async fn start_tray(
    sender: UnboundedSender<DesktopEvent>,
    language: Language,
) -> Result<TrayHandle> {
    let online = Arc::new(AtomicBool::new(true));
    let handle = Tray {
        sender,
        language,
        online: online.clone(),
    }
    .spawn()
    .await?;
    Ok(TrayHandle { online, handle })
}
pub fn notify(message: &str) {
    let _ = Command::new("notify-send")
        .args([
            "--app-name=Tradewind",
            "--icon=tradewind",
            "Tradewind",
            message,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
pub fn open(url: &str) -> Result<()> {
    anyhow::ensure!(
        url.starts_with("https://tradeskillmaster.com/")
            || url.starts_with("https://support.tradeskillmaster.com/"),
        msg!(LinkForbidden)
    );
    anyhow::ensure!(
        Command::new("xdg-open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?
            .success(),
        msg!(LinkOpenFailed)
    );
    Ok(())
}

pub fn save_capture(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    anyhow::ensure!(
        rgba.len() == width as usize * height as usize * 4,
        msg!(InvalidScreenshot)
    );
    let mut output = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in rgba.as_chunks::<4>().0 {
        output.extend_from_slice(&pixel[..3]);
    }
    storage::atomic_write(path, &output)
}

pub fn system_language() -> Language {
    let get = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
    let base = get("LC_ALL")
        .or_else(|| get("LC_MESSAGES"))
        .or_else(|| get("LANG"))
        .unwrap_or_else(|| "C".into());
    crate::i18n::system_language(&base, get("LANGUAGE").as_deref())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemAppearance {
    pub dark: bool,
    pub reduced_motion: bool,
}
/// Current preferences from the XDG portal, then one item per change. Without a portal the
/// stream yields the defaults (light, full motion).
pub fn appearance() -> impl futures::Stream<Item = SystemAppearance> {
    use futures::StreamExt;
    mundy::Preferences::stream(mundy::Interest::ColorScheme | mundy::Interest::ReducedMotion).map(
        |preferences| SystemAppearance {
            dark: preferences.color_scheme == mundy::ColorScheme::Dark,
            reduced_motion: preferences.reduced_motion.is_reduce(),
        },
    )
}
impl TrayHandle {
    pub async fn set_language(&self, language: Language) {
        self.handle
            .update(move |tray| tray.language = language)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;

    #[test]
    fn tray_icon_uses_original_pixels_and_restores_after_watcher_loss() {
        assert_eq!(
            APP_ICON_RGBA.len(),
            (APP_ICON_SIZE * APP_ICON_SIZE * 4) as usize
        );
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let online = Arc::new(AtomicBool::new(true));
        let mut tray = Tray {
            language: Language::English,
            sender,
            online: online.clone(),
        };
        assert_eq!(tray.icon_name(), "tradewind");
        let pixmaps = tray.icon_pixmap();
        for (icon, rgba) in pixmaps.iter().zip([
            include_bytes!("../../data/icons/app/icon-32.rgba").as_slice(),
            include_bytes!("../../data/icons/app/icon-48.rgba").as_slice(),
        ]) {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
            for (argb, rgba) in icon
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .zip(rgba.as_chunks::<4>().0)
            {
                assert_eq!(argb, &[rgba[3], rgba[0], rgba[1], rgba[2]]);
            }
        }
        assert!(tray.watcher_offline(ksni::OfflineReason::No));
        assert!(!online.load(Ordering::SeqCst));
        assert!(matches!(receiver.try_recv().unwrap(), DesktopEvent::Show));
        tray.watcher_online();
        assert!(online.load(Ordering::SeqCst));
        tray.activate(0, 0);
        assert!(matches!(receiver.try_recv().unwrap(), DesktopEvent::Show));
    }
}
