//! Desktop metadata for direct, uninstalled launches on Wayland.
use super::{APP_ID, storage};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

const MANAGED_ENTRY: &str = "X-Tradewind-Portable=true";
const ICON: &[u8] = include_bytes!("../../../data/icons/app/icon-256.png");

pub fn prepare() -> Result<()> {
    let dirs = directories::BaseDirs::new().context(msg!(MissingXdgDirs))?;
    let search = std::env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let installed: Vec<_> = std::env::split_paths(&search)
        .filter(|path| path.is_absolute())
        .collect();
    prepare_at(dirs.data_dir(), &super::executable()?, &installed)
}

fn prepare_at(data_dir: &Path, executable: &Path, installed: &[PathBuf]) -> Result<()> {
    let filename = format!("{APP_ID}.desktop");
    let entry = data_dir.join("applications").join(&filename);
    // Installed and user-customized launchers take precedence over our portable fallback.
    if fs::symlink_metadata(&entry).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Ok(());
    }
    match fs::read_to_string(&entry) {
        Ok(current) if !current.lines().any(|line| line == MANAGED_ENTRY) => return Ok(()),
        Ok(_) => (),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if installed
                .iter()
                .any(|root| root.join("applications").join(&filename).is_file())
            {
                return Ok(());
            }
        }
        Err(error) => return Err(error.into()),
    }
    let icon = data_dir.join(APP_ID).join("icons/app.png");
    let executable = desktop_path(executable)?;
    let icon_value = desktop_path(&icon)?;
    ensure!(!executable.contains('='), msg!(InvalidDesktopPath));
    // Exec has argument quoting followed by desktop-string escaping (two distinct layers).
    let argument = executable
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    let quoted = desktop_string(&format!("\"{argument}\""));
    let entry_text = include_str!("../../../data/tradewind.desktop")
        .replace("\nExec=tradewind\n", &format!("\nExec={quoted}\n"))
        .replace(
            "\nIcon=tradewind\n",
            &format!("\nIcon={}\n", desktop_string(icon_value)),
        );
    let entry_text = format!("{entry_text}NoDisplay=true\n{MANAGED_ENTRY}\n");
    write_if_changed(&icon, ICON)?;
    write_if_changed(&entry, entry_text.as_bytes())
}

fn desktop_path(path: &Path) -> Result<&str> {
    let value = path.to_str().context(msg!(InvalidDesktopPath))?;
    ensure!(
        path.is_absolute() && !value.chars().any(char::is_control),
        msg!(InvalidDesktopPath)
    );
    Ok(value)
}

fn desktop_string(value: &str) -> String {
    value.replace('\\', "\\\\")
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<()> {
    if !fs::read(path).is_ok_and(|current| current == bytes) {
        storage::atomic_write(path, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn direct_launch_registers_hidden_identity_and_reuses_unchanged_files() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let executable = root.path().join("My build/tradewind");
        prepare_at(&data, &executable, &[]).unwrap();
        let entry = data.join("applications/tradewind.desktop");
        let contents = fs::read_to_string(&entry).unwrap();
        assert!(contents.contains("NoDisplay=true\n"));
        assert!(contents.contains("StartupWMClass=tradewind\n"));
        assert!(contents.contains(&format!("Exec=\"{}\"\n", executable.display())));
        assert_eq!(
            fs::read(data.join("tradewind/icons/app.png")).unwrap(),
            ICON
        );
        let before = fs::metadata(&entry).unwrap().ino();
        prepare_at(&data, &executable, &[]).unwrap();
        assert_eq!(fs::metadata(&entry).unwrap().ino(), before);
        let moved = root.path().join("Moved build/tradewind");
        prepare_at(&data, &moved, &[]).unwrap();
        assert!(
            fs::read_to_string(entry)
                .unwrap()
                .contains(&format!("Exec=\"{}\"\n", moved.display()))
        );
    }

    #[test]
    fn installed_and_custom_desktop_files_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("user");
        let system = root.path().join("system");
        let installed = system.join("applications/tradewind.desktop");
        storage::atomic_write(
            &installed,
            include_bytes!("../../../data/tradewind.desktop"),
        )
        .unwrap();
        prepare_at(&user, Path::new("/example/tradewind"), &[system]).unwrap();
        assert!(!user.exists());
        let custom = user.join("applications/tradewind.desktop");
        let contents = b"[Desktop Entry]\nName=Custom TSM\nIcon=custom-icon\n";
        storage::atomic_write(&custom, contents).unwrap();
        prepare_at(&user, Path::new("/example/tradewind"), &[]).unwrap();
        assert_eq!(fs::read(&custom).unwrap(), contents);
        assert!(!user.join("tradewind").exists());
    }

    #[test]
    fn desktop_exec_escapes_paths_and_rejects_newline_injection() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        prepare_at(&data, Path::new("/tmp/a $b`c\"d\\e%f/tradewind"), &[]).unwrap();
        let contents = fs::read_to_string(data.join("applications/tradewind.desktop")).unwrap();
        assert!(contents.contains(r#"Exec="/tmp/a \\$b\\`c\\"d\\\\e%%f/tradewind""#));
        let untouched = root.path().join("untouched");
        assert!(
            prepare_at(
                &untouched,
                Path::new("/tmp/build\nHidden=true/tradewind"),
                &[]
            )
            .is_err()
        );
        assert!(!untouched.exists());
    }
}
