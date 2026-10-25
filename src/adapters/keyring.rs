use anyhow::{Context, Result, ensure};
use std::{
    io::{self, Write},
    process::{Command, Stdio},
};
fn missing_tool(error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::NotFound {
        anyhow::anyhow!(msg!(MissingSecretTool))
    } else {
        error.into()
    }
}
pub fn lookup(email: &str) -> Result<String> {
    let output = Command::new("secret-tool")
        .args(["lookup", "application", "tradewind", "account", email])
        .stderr(Stdio::null())
        .output()
        .map_err(missing_tool)?;
    ensure!(
        output.status.success() && !output.stdout.is_empty(),
        msg!(MissingKeyringLogin)
    );
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub fn store(email: &str, token: &str) -> Result<()> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label=Tradewind",
            "application",
            "tradewind",
            "account",
            email,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(missing_tool)?;
    child
        .stdin
        .take()
        .context(msg!(MissingKeyringInput))?
        .write_all(token.as_bytes())?;
    ensure!(child.wait()?.success(), msg!(KeyringUnavailable));
    Ok(())
}
pub fn clear(email: &str) -> Result<()> {
    if email.is_empty() {
        return Ok(());
    }
    let status = Command::new("secret-tool")
        .args(["clear", "application", "tradewind", "account", email])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(missing_tool)?;
    ensure!(status.success(), msg!(KeyringRemoveFailed));
    Ok(())
}
