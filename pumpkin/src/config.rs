//! Plugin configuration, stored as `config.json` in the plugin data folder.

use pumpkin_plugin_api::Context;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// Client-side chat format. Placeholders:
///
/// * `{player}` — the sender's display name
/// * `{message}` — the raw chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrChatConfig {
    /// Legacy-style format template with `&` color codes, e.g.
    /// `&7<&f{player}&7> &f{message}`.
    pub format: String,
    /// Experimental: forward chat over Redis to link multiple servers.
    pub redis_enabled: bool,
    /// Redis connection URL used when `redis_enabled` is true.
    pub redis_url: String,
}

impl Default for TrChatConfig {
    fn default() -> Self {
        Self {
            format: "&7<&f{player}&7> &f{message}".to_string(),
            redis_enabled: false,
            redis_url: "redis://127.0.0.1:6379/".to_string(),
        }
    }
}

/// A shared handle to the loaded configuration.
pub struct SharedConfig(pub RwLock<TrChatConfig>);

impl SharedConfig {
    /// Loads `config.json` from the plugin data folder.
    ///
    /// If the file does not exist yet, the default configuration is written to
    /// disk first (so admins can see and edit it), then returned. A missing or
    /// unreadable file is never silently ignored — either the default is created
    /// or the load fails loudly.
    pub fn load(context: &Context) -> Result<Self, String> {
        let folder = context.get_data_folder();
        let path = format!("{folder}/config.json");
        let config = match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw)
                .map_err(|e| format!("failed to parse {path}: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let default = TrChatConfig::default();
                Self::write_default(&path, &default)?;
                default
            }
            Err(e) => return Err(format!("failed to read {path}: {e}")),
        };
        Ok(Self(RwLock::new(config)))
    }

    /// Writes the default configuration to `config.json` (creating the data
    /// folder if needed). Called on first startup so the file exists on disk.
    fn write_default(path: &str, config: &TrChatConfig) -> Result<(), String> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("failed to serialize config: {e}"))?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create data folder {}: {e}", parent.display()))?;
        }
        std::fs::write(path, json).map_err(|e| format!("failed to write {path}: {e}"))
    }
}