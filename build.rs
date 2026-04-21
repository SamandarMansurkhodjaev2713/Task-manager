//! Build script that threads build-time metadata into the binary.
//!
//! We read the `GIT_SHA` and `RUSTC_VERSION` environment variables that
//! the container build (or a local shell) provides and re-emit them via
//! `cargo:rustc-env=...` so that `option_env!(...)` inside the crate can
//! see them reliably.  Without this indirection the values can get lost
//! between layered `ENV` declarations and Cargo's env filtering.
//!
//! If a variable is not set (e.g. during `cargo check` in a local shell
//! without git plumbing) we leave it unset and let `option_env!(...)`
//! fall back to the literal string `"unknown"` in `src/presentation/http`.

use std::env;
use std::process::Command;

fn main() {
    let git_sha = env::var("GIT_SHA").ok().or_else(resolve_git_sha_via_git);
    if let Some(sha) = git_sha {
        if !sha.trim().is_empty() {
            println!("cargo:rustc-env=GIT_SHA={}", sha.trim());
        }
    }

    let rustc_version = env::var("RUSTC_VERSION")
        .ok()
        .or_else(resolve_rustc_version);
    if let Some(version) = rustc_version {
        if !version.trim().is_empty() {
            println!("cargo:rustc-env=RUSTC_VERSION={}", version.trim());
        }
    }

    // Rerun triggers: env vars and the actual file that gets processed.
    // Changes to the HEAD reference (local commits) should re-embed a
    // fresh SHA automatically.
    println!("cargo:rerun-if-env-changed=GIT_SHA");
    println!("cargo:rerun-if-env-changed=RUSTC_VERSION");
    if std::path::Path::new(".git/HEAD").exists() {
        println!("cargo:rerun-if-changed=.git/HEAD");
    }
}

fn resolve_git_sha_via_git() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    Some(sha.trim().to_owned())
}

fn resolve_rustc_version() -> Option<String> {
    let output = Command::new("rustc").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    Some(version.trim().to_owned())
}
