//! Plugin configuration, stored as `config.json` in the plugin data folder.
//!
//! Mirrors the *configuration surface* of TrChat Bukkit v2 in a flat,
//! JSON-friendly shape:
//!
//! * `format` + `channels[].format` — legacy `&`-code templates using the
//!   `{player}` / `{message}` placeholders (later: `{target}`, `{world}`, …).
//! * `channels[].prefixes` — chat prefixes that route a line into a channel
//!   (`Bindings.Prefix`); the longest matching prefix wins, exactly like
//!   `ChannelManager.byPrefix` in the Bukkit plugin.
//! * `channels[].permission` — `Join-Permission`; an empty string allows
//!   everyone.
//! * `channels[].radius` — `DISTANCE` speak condition threshold in blocks
//!   (`0.0` = unlimited).
//! * `is_default` — the fallback channel used when no prefix matches.
//! * `msg` — private message (`/msg`) templates: `{player}`, `{target}`,
//!   `{message}`.
//! * `redis_enabled` + `redis_url` — cross-server relay (documented follow-up;
//!   the crate already declares `network.*` permissions).
//!
//! See `pumpkin/README.md` for the full mapping table against the Bukkit
//! `settings.yml` / `channels/*.yml` schema.

use pumpkin_plugin_api::Context;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock, RwLock};

/// A single chat channel: a set of message prefixes plus the format used for
/// the messages routed to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Channel id, as shown by `/channel` and used by `/channel <id> <message>`.
    pub id: String,
    /// Message prefixes that route a chat line into this channel (for example
    /// `["!all"]`). The longest matching prefix wins; an empty list means the
    /// channel can only be reached as the fallback channel or by command.
    pub prefixes: Vec<String>,
    /// Legacy-style format template with `&` color codes. Placeholders:
    /// `{player}`, `{message}`.
    pub format: String,
    /// `Join-Permission`: permission required to speak into (and, when set,
    /// listen to) this channel. Empty = no restriction.
    pub permission: String,
    /// Speak radius in blocks (`DISTANCE`); `0.0` = server-wide.
    pub radius: f64,
    /// Whether this channel is the fallback when no prefix matches.
    pub is_default: bool,
}

/// Private message (`/msg`) rendering templates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgConfig {
    /// Template shown to the sender. Placeholders: `{player}`, `{target}`,
    /// `{message}`.
    pub sender: String,
    /// Template shown to the receiver.
    pub receiver: String,
}

impl Default for MsgConfig {
    fn default() -> Self {
        Self {
            sender: "&7[&f{player} &7-> &f{target}&7] &f{message}".to_string(),
            receiver: "&7[&f{player} &7-> &f{target}&7] &f{message}".to_string(),
        }
    }
}

/// Client-side chat configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrChatConfig {
    /// Legacy-style format template with `&` color codes, used when no channel
    /// is configured at all. Placeholders: `{player}`, `{message}`.
    pub format: String,
    /// Channel definitions. Empty means "no channel system": the `format`
    /// template is used directly for every message.
    pub channels: Vec<ChannelConfig>,
    /// Private message templates (`/msg`, `/tell`, ...).
    pub msg: MsgConfig,
    /// Whether the Redis cross-server relay is enabled (experimental).
    pub redis_enabled: bool,
    /// Redis endpoint for the cross-server relay, `redis://host:port/db`.
    pub redis_url: String,
    /// `chat.blockedWords` — censored words. Empty = filtering disabled.
    #[serde(default)]
    pub blocked_words: Vec<String>,
    /// `chat.filterReplacement` — the character repeated to the matched word's
    /// length when censoring (`"*"` by default).
    #[serde(default)]
    pub filter_replacement: String,
    /// `chat.messageMaxLength` — maximum message length in UTF-16 code units.
    #[serde(default)]
    pub message_max_length: u32,
    /// `chat.cooldownMillis` — minimum delay between two accepted messages.
    #[serde(default)]
    pub cooldown_millis: u64,
    /// `chat.antiRepeatSimilarity` — similarity threshold in `[0, 1]` above
    /// which a message counts as a repeat (`0` disables the anti-repeat guard).
    #[serde(default)]
    pub anti_repeat_similarity: f64,
    /// `chat.antiRepeatPeriodMillis` — how long recent messages are kept for
    /// the anti-repeat comparison.
    #[serde(default)]
    pub anti_repeat_period_millis: u64,
}

impl Default for TrChatConfig {
    fn default() -> Self {
        Self {
            format: "&7<&f{player}&7> &f{message}".to_string(),
            channels: vec![
                ChannelConfig {
                    id: "normal".to_string(),
                    prefixes: vec![],
                    format: "&7<&f{player}&7> &f{message}".to_string(),
                    permission: String::new(),
                    radius: 0.0,
                    is_default: true,
                },
                ChannelConfig {
                    id: "staff".to_string(),
                    prefixes: vec!["!staff".to_string()],
                    format: "&8[&cSTAFF&8] &c{player}&8: &f{message}".to_string(),
                    permission: "trchat.channel.staff".to_string(),
                    radius: 0.0,
                    is_default: false,
                },
                ChannelConfig {
                    id: "local".to_string(),
                    prefixes: vec!["@local".to_string()],
                    format: "&8[&2LOCAL&8] &7{player}&8: &f{message}".to_string(),
                    permission: String::new(),
                    radius: 100.0,
                    is_default: false,
                },
            ],
            msg: MsgConfig::default(),
            redis_enabled: false,
            redis_url: "redis://127.0.0.1:6379/".to_string(),
            blocked_words: Vec::new(),
            filter_replacement: "*".to_string(),
            message_max_length: 256,
            cooldown_millis: 2000,
            anti_repeat_similarity: 0.85,
            anti_repeat_period_millis: 60_000,
        }
    }
}

/// The channel a chat line was routed to.
pub enum Route<'a> {
    /// No channel is configured: render with [`TrChatConfig::format`].
    Plain(&'a str),
    /// A channel matched (by prefix, or as the fallback channel) together with
    /// the message with its prefix stripped.
    Channel(&'a ChannelConfig, &'a str),
}

impl TrChatConfig {
    /// Routes a raw chat line to a channel.
    ///
    /// The longest matching prefix wins (Bukkit `ChannelManager.byPrefix`
    /// semantics); when nothing matches, the channel flagged `default` is used,
    /// falling back to the first configured channel.
    pub fn route<'a>(&'a self, message: &'a str) -> Route<'a> {
        let mut matched: Option<(&'a ChannelConfig, &'a str)> = None;
        for channel in &self.channels {
            for prefix in &channel.prefixes {
                if prefix.is_empty() || !message.starts_with(prefix.as_str()) {
                    continue;
                }
                if matched.is_none_or(|(_, current)| current.len() < prefix.len()) {
                    matched = Some((channel, prefix.as_str()));
                }
            }
        }
        if let Some((channel, prefix)) = matched {
            return Route::Channel(channel, message[prefix.len()..].trim_start());
        }
        match self.fallback_channel() {
            Some(channel) => Route::Channel(channel, message.trim_start()),
            None => Route::Plain(message.trim_start()),
        }
    }

    /// The channel flagged `default`; falls back to the first configured
    /// channel. `None` when there are no channels at all.
    pub fn fallback_channel(&self) -> Option<&ChannelConfig> {
        self.channels
            .iter()
            .find(|c| c.is_default)
            .or_else(|| self.channels.first())
    }

    /// Looks up a channel by its (case-insensitive) id.
    pub fn channel_by_id(&self, id: &str) -> Option<&ChannelConfig> {
        self.channels
            .iter()
            .find(|c| c.id.eq_ignore_ascii_case(id))
    }

    /// All configured channels, in declaration order.
    pub fn channels(&self) -> &[ChannelConfig] {
        &self.channels
    }
}

/// A shared, read-mostly handle to the plugin configuration.
///
/// The configuration is locked only while each chat line is being processed
/// (short and non-reentrant), which matches the "hot path" expectation of the
/// Bukkit plugin. `Arc` + interior mutability also lets the pending `/trchat
/// reload` command swap the whole config in one shot.
#[derive(Clone)]
pub struct SharedConfig(pub Arc<RwLock<TrChatConfig>>);

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
        Ok(Self(Arc::new(RwLock::new(config))))
    }

    /// Borrows the configuration for reading.
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, TrChatConfig> {
        self.0.read().unwrap_or_else(|e| e.into_inner())
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

// ---- process-wide configuration handle ----
//
// The event pipeline borrows the loaded config for the duration of one chat
// line, while commands (`/trchat reload`, ...) need access outside any event.
// `SharedConfig` is cheaply cloneable (`Arc`), so we keep one process-wide
// copy here, installed by `ChatManager::init` and replaceable by `reload`.

static GLOBAL_CONFIG: OnceLock<Arc<RwLock<TrChatConfig>>> = OnceLock::new();
static DATA_FOLDER: OnceLock<String> = OnceLock::new();

/// Installs the process-wide configuration handle (called once at plugin load).
pub fn init_global(config: &SharedConfig, data_folder: String) {
    let _ = GLOBAL_CONFIG.set(Arc::clone(&config.0));
    let _ = DATA_FOLDER.set(data_folder);
}

/// Borrows the process-wide configuration for reading.
pub fn global_config() -> std::sync::RwLockReadGuard<'static, TrChatConfig> {
    GLOBAL_CONFIG
        .get()
        .expect("TrChat configuration not initialized")
        .read()
        .unwrap_or_else(|e| e.into_inner())
}

/// Re-reads `config.json` from disk and swaps the process-wide configuration.
pub fn reload_global() -> Result<(), String> {
    let folder = DATA_FOLDER.get().ok_or("TrChat configuration not initialized")?;
    let path = format!("{folder}/config.json");
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let config: TrChatConfig =
        serde_json::from_str(&raw).map_err(|e| format!("failed to parse {path}: {e}"))?;
    let mut guard = GLOBAL_CONFIG
        .get()
        .ok_or("TrChat configuration not initialized")?
        .write()
        .unwrap_or_else(|e| e.into_inner());
    *guard = config;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> TrChatConfig {
        TrChatConfig::default()
    }

    #[test]
    fn longest_matching_prefix_wins() {
        let mut config = config();
        config.channels.push(ChannelConfig {
            id: "all-staff".to_string(),
            prefixes: vec!["!all!staff".to_string()],
            format: String::new(),
            permission: String::new(),
            radius: 0.0,
            is_default: false,
        });
        // `!all` (global) and `!all!staff` both match; the longer one wins.
        match config.route("!all!staff hello") {
            Route::Channel(channel, rest) => {
                assert_eq!(channel.id, "all-staff");
                assert_eq!(rest, "hello");
            }
            Route::Plain(_) => panic!("expected a channel match"),
        }
    }

    #[test]
    fn prefix_strips_and_trims() {
        let mut config = config();
        config.channels.push(ChannelConfig {
            id: "global".to_string(),
            prefixes: vec!["!global".to_string()],
            format: String::new(),
            permission: String::new(),
            radius: 0.0,
            is_default: false,
        });
        match config.route("!global   hi there") {
            Route::Channel(channel, rest) => {
                assert_eq!(channel.id, "global");
                assert_eq!(rest, "hi there");
            }
            Route::Plain(_) => panic!("expected the global channel"),
        }
    }

    #[test]
    fn unmatched_message_uses_the_default_channel() {
        match config().route("plain hello") {
            Route::Channel(channel, rest) => {
                assert_eq!(channel.id, "normal");
                assert_eq!(rest, "plain hello");
            }
            Route::Plain(_) => panic!("expected the default channel"),
        }
    }

    #[test]
    fn without_channels_the_format_is_used_directly() {
        let mut config = config();
        config.channels.clear();
        match config.route("plain hello") {
            Route::Plain(rest) => assert_eq!(rest, "plain hello"),
            Route::Channel(..) => panic!("expected the plain fallback"),
        }
    }

    #[test]
    fn empty_prefix_never_matches() {
        let mut config = config();
        config.channels[0].prefixes = vec![String::new()];
        match config.route("anything") {
            Route::Channel(channel, _) => assert_eq!(channel.id, "normal"),
            Route::Plain(_) => panic!("expected the default channel"),
        }
    }

    #[test]
    fn channel_by_id_is_case_insensitive() {
        let config = config();
        assert!(config.channel_by_id("NORMAL").is_some());
        assert!(config.channel_by_id("missing").is_none());
    }
}