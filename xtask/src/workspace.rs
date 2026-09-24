//! Facts about the Cargo workspace.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::TaskResult;

/// The workspace root (the parent of this crate's directory).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// `cargo metadata` for the workspace members only.
pub fn metadata() -> Result<Value, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root())
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("cargo metadata: {error}"))
}

/// The version every workspace crate shares.
pub fn version() -> Result<String, String> {
    let metadata = metadata()?;
    metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| package["name"] == "mujina-app")
        .and_then(|package| package["version"].as_str())
        .map(str::to_string)
        .ok_or_else(|| "mujina-app not found in cargo metadata".to_string())
}

pub fn version_check(tag: Option<&str>) -> TaskResult {
    let tag = match tag {
        Some(tag) => tag.to_string(),
        None => std::env::var("GITHUB_REF_NAME")
            .map_err(|_| "no tag given and GITHUB_REF_NAME is not set".to_string())?,
    };
    let version = version()?;
    if tag == format!("v{version}") {
        println!("tag {tag} matches version {version}");
        Ok(())
    } else {
        Err(format!(
            "tag {tag} does not match Cargo.toml version {version} (expected v{version})"
        ))
    }
}
