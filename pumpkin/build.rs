//! Build script: exposes the TrChat version this port tracks.
//!
//! `/trchat version` and the update checker (`updates:`) must report the
//! version of the Mod this port implements, not the crate's own `0.1.0` —
//! otherwise every check would claim a new release is available. The version
//! lives in the repository `gradle.properties` (`mod_version`), so it is read at
//! build time and handed to the code as `TRCHAT_VERSION`.

use std::path::Path;

fn main() {
    // The crate sits in `pumpkin/`, directly below the repository root.
    let properties = Path::new("../gradle.properties");
    println!("cargo:rerun-if-changed={}", properties.display());
    println!("cargo:rerun-if-changed=build.rs");

    let version = std::fs::read_to_string(properties)
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("mod_version=")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
        })
        // A checkout without the repository root (e.g. the crate on its own)
        // still has to build; `0.0.0` simply makes every release look newer.
        .unwrap_or_else(|| "0.0.0".to_string());

    println!("cargo:rustc-env=TRCHAT_VERSION={version}");
}
