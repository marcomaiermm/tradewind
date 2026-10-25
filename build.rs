//! Appends the git commit to the version shown in Settings and `--version`.
//! Source tarballs have no `.git`, so their builds report the plain version. Checking for it
//! first keeps git from picking up an enclosing repository, e.g. a distribution package build.
use std::{path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    // Approximates the "-dirty" marker. Edits under src/ or to Cargo.toml rerun this script,
    // edits elsewhere (docs, packaging) do not.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let version = env!("CARGO_PKG_VERSION");
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let commit = Path::new(".git")
        .exists()
        .then(|| git(&["rev-parse", "--short=8", "HEAD"]))
        .flatten();
    let full = match commit {
        Some(commit) if !commit.is_empty() => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|s| !s.is_empty());
            format!("{version} ({commit}{})", if dirty { "-dirty" } else { "" })
        }
        _ => version.to_owned(),
    };
    println!("cargo:rustc-env=TRADEWIND_VERSION={full}");
}
