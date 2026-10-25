//! File dialogs: the XDG desktop portal first (works on every desktop), kdialog as a fallback.
use crate::i18n::{Key, Language};
use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    io::ErrorKind,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU32, Ordering},
};
use zbus::zvariant::{OwnedValue, Value};

const PORTAL: &str = "org.freedesktop.portal.Desktop";

pub async fn choose_directory(initial: &Path, language: Language) -> Result<PathBuf> {
    let title = language.text(Key::DirectoryDialog);
    let options = HashMap::from([
        ("directory", Value::from(true)),
        ("current_folder", folder(initial)),
    ]);
    let path = match portal("OpenFile", title, options).await? {
        Some(choice) => choice,
        None => kdialog(&[
            "--getexistingdirectory".as_ref(),
            initial.as_os_str(),
            "--title".as_ref(),
            title.as_ref(),
        ])?,
    };
    path.context(msg!(DirectoryCanceled))
}

pub async fn choose_export(name: &str, language: Language) -> Result<PathBuf> {
    let home = directories::UserDirs::new().context(msg!(MissingHome))?;
    let documents = home.document_dir().unwrap_or(home.home_dir());
    let title = language.text(Key::ExportDialog);
    let csv = language.text(Key::CsvFiles);
    let options = HashMap::from([
        ("current_name", Value::from(name)),
        ("current_folder", folder(documents)),
        ("filters", filters(csv)),
    ]);
    let path = match portal("SaveFile", title, options).await? {
        Some(choice) => choice,
        None => kdialog(&[
            "--getsavefilename".as_ref(),
            documents.join(name).as_os_str(),
            format!("*.csv|{csv}").as_ref(),
            "--title".as_ref(),
            title.as_ref(),
        ])?,
    };
    let path = path.context(msg!(ExportCanceled))?;
    ensure!(!path.as_os_str().is_empty(), msg!(MissingExportPath));
    // Portal backends and kdialog both confirm replacing an existing file.
    Ok(path)
}

/// `None` when no portal FileChooser answers, otherwise the choice (`None` when canceled).
async fn portal(
    method: &str,
    title: &str,
    options: HashMap<&str, Value<'_>>,
) -> Result<Option<Option<PathBuf>>> {
    let Ok(mut responses) = request(method, title, options).await else {
        return Ok(None);
    };
    let response = responses.next().await.context(msg!(FileDialogFailed))?;
    let (code, mut results): (u32, HashMap<String, OwnedValue>) = response.body().deserialize()?;
    match code {
        0 => (),
        1 => return Ok(Some(None)),
        _ => bail!(msg!(FileDialogFailed)),
    }
    let uris = Vec::<String>::try_from(results.remove("uris").context(msg!(FileDialogFailed))?)?;
    let path = uris.first().and_then(|uri| file_path(uri));
    Ok(Some(Some(path.context(msg!(FileDialogFailed))?)))
}

/// Subscribes to the request's Response signal before asking, so a fast answer is not missed.
async fn request(
    method: &str,
    title: &str,
    mut options: HashMap<&str, Value<'_>>,
) -> zbus::Result<zbus::proxy::SignalStream<'static>> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let token = format!("tradewind{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let connection = zbus::Connection::session().await?;
    let sender = connection
        .unique_name()
        .ok_or_else(|| zbus::Error::Failure("no unique bus name".into()))?
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request =
        zbus::Proxy::new(&connection, PORTAL, path, "org.freedesktop.portal.Request").await?;
    let responses = request.receive_signal("Response").await?;
    options.insert("handle_token", Value::from(token.as_str()));
    let chooser = zbus::Proxy::new(
        &connection,
        PORTAL,
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .await?;
    // Assumes the portal honors handle_token (xdg-desktop-portal 0.9 and newer).
    chooser.call_method(method, &("", title, options)).await?;
    Ok(responses)
}

/// One glob filter, `a(sa(us))` on the bus.
fn filters(label: &str) -> Value<'_> {
    Value::from(vec![(label, vec![(0u32, "*.csv")])])
}

/// Portal folders are NUL-terminated byte strings.
fn folder(path: &Path) -> Value<'static> {
    let mut bytes = path.as_os_str().as_bytes().to_vec();
    bytes.push(0);
    Value::from(bytes)
}

fn file_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let bytes: Vec<u8> = percent_encoding::percent_decode_str(encoded).collect();
    Some(OsString::from_vec(bytes).into())
}

fn kdialog(args: &[&OsStr]) -> Result<Option<PathBuf>> {
    let output = match Command::new("kdialog")
        .args(args)
        .stderr(Stdio::null())
        .output()
    {
        Err(error) if error.kind() == ErrorKind::NotFound => bail!(msg!(NoFileDialog)),
        result => result?,
    };
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(
        String::from_utf8(output.stdout)?.trim(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portal_uris_decode_to_local_paths() {
        assert_eq!(
            file_path("file:///home/a%20b/%C3%BC.csv"),
            Some(PathBuf::from("/home/a b/ü.csv"))
        );
        assert_eq!(file_path("https://example.com/x"), None);
        assert_eq!(folder(Path::new("/tmp")).value_signature(), "ay");
        assert_eq!(filters("CSV").value_signature(), "a(sa(us))");
    }
}
