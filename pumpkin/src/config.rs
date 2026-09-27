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
    /// Loads `config.json` from the plugin data folder, falling back to defaults.
    pub fn load(context: &Context) -> Result<Self, String> {
        let folder = context.get_data_folder();
        let path = format!("{folder}/config.json");
        let config = match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw)
                .map_err(|e| format!("failed to parse {path}: {e}"))?,
            Err(_) => TrChatConfig::default(),
        };
        Ok(Self(RwLock::new(config)))
    }
}