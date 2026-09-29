//! Configuration layer — the NeoForge Mod config surface ported to Pumpkin.
//!
//! The old single `config.json` is replaced by the Mod's YAML layout. On first
//! startup the plugin writes the bundled defaults (byte-identical copies of
//! `src/main/resources/defaults/`) into the plugin data folder, then loads:
//!
//! * `settings.yml`      — server, chat guards, logging, updates, Redis
//! * `channels/*.yml`    — one channel definition per file
//! * `lang/*.yml`        — locale message tables (consumed by [`crate::lang`])
//! * `datasource.yml`    — data source (SQLite / MySQL / …)
//! * `filter.yml`        — blocked-word filter
//! * `function.yml`      — chat functions (mention, item, …)
//! * `special-chars.yml` — special-character table
//!
//! Fully typed and wired into the runtime today: `settings.yml` plus
//! `channels/*.yml` (+ `lang/*.yml` through [`crate::lang`]). The remaining
//! files are written and YAML-validated but not consumed yet — the honest
//! same status the previous JSON build had for Redis, a documented follow-up.
//!
//! Key naming matches the Mod (`camelCase` in `settings.yml`, `PascalCase`
//! sections in channel files), so a single `config/trchat/` folder can be
//! shared between the NeoForge/Fabric Mod and this experimental plugin.

use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard};

use pumpkin_plugin_api::Context;
use serde::Deserialize;

/// Default configuration files bundled into the plugin binary.
///
/// These are compiled-in copies of the Mod's `src/main/resources/defaults/`
/// so the WASM binary (which cannot enumerate host resources) can seed a fresh
/// data folder on first startup.
pub mod defaults {
    pub const SETTINGS: &str = include_str!("defaults/settings.yml");
    pub const DATASOURCE: &str = include_str!("defaults/datasource.yml");
    pub const FILTER: &str = include_str!("defaults/filter.yml");
    pub const FUNCTION: &str = include_str!("defaults/function.yml");
    pub const SPECIAL_CHARS: &str = include_str!("defaults/special-chars.yml");
    /// Channel defaults, keyed by file stem (`Normal`, `Global`, …).
    /// `Example.yml` and `Schema.yml` are shipped for reference but are never
    /// registered as channels (same rule as the upstream loader).
    pub const CHANNELS: &[(&str, &str)] = &[
        ("Normal", include_str!("defaults/channels/Normal.yml")),
        ("Global", include_str!("defaults/channels/Global.yml")),
        ("Staff", include_str!("defaults/channels/Staff.yml")),
        ("Private", include_str!("defaults/channels/Private.yml")),
        ("Schema", include_str!("defaults/channels/Schema.yml")),
        ("Example", include_str!("defaults/channels/Example.yml")),
    ];
    /// Lang defaults, keyed by file stem (`en_US`, `zh_CN`, `es_ES`).
    pub const LANGS: &[(&str, &str)] = &[
        ("en_US", include_str!("defaults/lang/en_US.yml")),
        ("zh_CN", include_str!("defaults/lang/zh_CN.yml")),
        ("es_ES", include_str!("defaults/lang/es_ES.yml")),
    ];
}

/// A snapshot of `settings.yml`, mirroring the Mod's `TrChatConfig` builder
/// keys byte-for-byte (camelCase in YAML). Unknown keys are tolerated so newer
/// Mod configs do not fail the parse.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub chat: ChatSection,
    pub logging: LoggingSection,
    pub updates: UpdatesSection,
    pub redis: RedisSection,
}

/// `chat:` — the guards and display knobs used by the chat pipeline.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ChatSection {
    #[serde(rename = "serverId")]
    pub server_id: u32,
    #[serde(rename = "serverName")]
    pub server_name: String,
    #[serde(rename = "defaultLanguage")]
    pub default_language: String,
    #[serde(rename = "globalPrefix")]
    pub global_prefix: String,
    #[serde(rename = "messageMaxLength")]
    pub message_max_length: u32,
    #[serde(rename = "cooldownMillis")]
    pub cooldown_millis: u64,
    #[serde(rename = "antiRepeatSimilarity")]
    pub anti_repeat_similarity: f64,
    #[serde(rename = "antiRepeatMaxPerPeriod")]
    pub anti_repeat_max_per_period: u32,
    #[serde(rename = "antiRepeatPeriodMillis")]
    pub anti_repeat_period_millis: u64,
    #[serde(rename = "antiRepeatCompareAll")]
    pub anti_repeat_compare_all: bool,
    #[serde(rename = "antiHighFrequencyMaxPerPeriod")]
    pub anti_high_frequency_max_per_period: u32,
    #[serde(rename = "antiHighFrequencyPeriodMillis")]
    pub anti_high_frequency_period_millis: u64,
    #[serde(rename = "antiDuplicatePhraseMaxRepeat")]
    pub anti_duplicate_phrase_max_repeat: u32,
    #[serde(rename = "antiDuplicatePhraseWhitelist")]
    pub anti_duplicate_phrase_whitelist: Vec<String>,
    #[serde(rename = "blockedWords")]
    pub blocked_words: Vec<String>,
    #[serde(rename = "filterReplacement")]
    pub filter_replacement: String,
    #[serde(rename = "disabledWorlds")]
    pub disabled_worlds: Vec<String>,
}

/// `logging:` — daily plain-text chat logs under `logs/`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LoggingSection {
    #[serde(rename = "normalMessageFormat")]
    pub normal_message_format: String,
    #[serde(rename = "privateMessageFormat")]
    pub private_message_format: String,
    #[serde(rename = "retentionDays")]
    pub retention_days: u32,
}

/// `updates:` — notifies only, never downloads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdatesSection {
    pub enabled: bool,
    #[serde(rename = "intervalMinutes")]
    pub interval_minutes: u32,
}

/// `redis:` — cross-server transport (runtime is a documented follow-up).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RedisSection {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: u16,
    #[serde(rename = "connectTimeoutMillis")]
    pub connect_timeout_millis: u32,
    #[serde(rename = "socketTimeoutMillis")]
    pub socket_timeout_millis: u32,
    #[serde(rename = "reconnectDelayMillis")]
    pub reconnect_delay_millis: u32,
    pub channel: String,
}

/// One parsed channel: the `Options`/`Bindings`/`Formats`/`Sender`/
/// `Receiver`/`Console` sections of a `channels/<Id>.yml` file.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Channel id — the YAML file stem (`Normal`, `Global`, `Staff`, …).
    pub id: String,
    pub options: ChannelOptions,
    pub bindings: ChannelBindings,
    /// Format tiers parsed for the future component renderer; the legacy
    /// string renderer consumes the flattened [`ChannelConfig::template`].
    #[allow(dead_code)] // consumed by the upcoming component renderer
    pub formats: Vec<FormatLayer>,
    /// Private-message tiers (never broadcast to the channel).
    pub sender: Vec<FormatLayer>,
    pub receiver: Vec<FormatLayer>,
    /// Console tiers — the upstream renders a console-only variant; this port
    /// keeps the parsed data for that follow-up.
    #[allow(dead_code)] // consumed by the upcoming console renderer
    pub console: Vec<FormatLayer>,
    /// Legacy flattened public template (see [`legacy_template`]) consumed by
    /// the current string renderer in `chat.rs`.
    pub template: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelOptions {
    pub join_permission: String,
    /// Parsed and kept for the permission layer follow-up (channel listeners,
    /// per-world channel visibility). Not consumed by the pipeline yet.
    #[allow(dead_code)]
    pub listen_permission: String,
    /// Condition DSL (`perm "…"`, `player op`, …) — parsed and kept for the
    /// condition evaluator follow-up; the legacy renderer skips it.
    #[allow(dead_code)]
    pub speak_condition: String,
    #[allow(dead_code)] // consumed by the upcoming channel join/listen logic
    pub always_listen: bool,
    pub auto_join: bool,
    pub private: bool,
    /// `ALL` | `SELF` | `SINGLE_WORLD` | `DISTANCE;<blocks>`.
    pub target: String,
    /// Parsed and kept for the Redis transport follow-up.
    #[allow(dead_code)]
    pub proxy: bool,
    #[allow(dead_code)]
    pub force_proxy: bool,
    #[allow(dead_code)]
    pub double_transfer: bool,
    #[allow(dead_code)]
    pub ports: Vec<u16>,
    /// Functions disabled in this channel (`Mention`, …) — parsed and kept
    /// for the chat-functions follow-up (function.yml).
    #[allow(dead_code)]
    pub disabled_functions: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelBindings {
    /// Prefixes that route chat into this channel; the longest match wins.
    pub prefix: Vec<String>,
    /// Channel commands (`msg`, `global`, `staff`, …) — parsed and kept for
    /// the dynamic command-registration follow-up; the fixed command set in
    /// `commands.rs` covers the bundled channels today.
    #[allow(dead_code)]
    pub command: Vec<String>,
}

/// One entry of a `Formats` / `Sender` / `Receiver` / `Console` list.
#[derive(Debug, Clone, Default)]
pub struct FormatLayer {
    /// `~` or empty = unconditional tier; other strings are conditions this
    /// port cannot evaluate yet (`perm "trchat.global"`, `player op`, …).
    pub condition: String,
    /// Tier priority — kept for the upcoming condition/priority resolver;
    /// the legacy renderer picks the first unconditional tier instead.
    #[allow(dead_code)]
    pub priority: i64,
    /// Ordered prefix components (group name → text), e.g. `server`, `player`.
    pub prefix: Vec<PrefixPart>,
    /// `msg.default-color` — message text color (`7`, `&f`, …).
    pub msg_default_color: String,
    /// `msg.special-char-color` — color applied to configured special
    /// characters (resource-pack glyphs); empty = wrap disabled.
    pub special_char_color: String,
}

/// A single prefix component; `text` is the legacy plain-text payload.
#[derive(Debug, Clone, Default)]
pub struct PrefixPart {
    pub condition: String,
    pub text: String,
}

impl ChannelConfig {
    /// Effective permission required to speak in this channel. The Mod uses
    /// `Options.Join-Permission` for join *and* use; a channel without one
    /// (e.g. `Normal`) is public.
    pub fn permission(&self) -> &str {
        &self.options.join_permission
    }

    /// Whether this is the default (auto-join) channel.
    #[allow(dead_code)] // used by channel-management commands follow-up
    pub fn is_default(&self) -> bool {
        self.options.auto_join
    }

    /// Broadcast reach from `Options.Target`; `0.0` = unlimited.
    pub fn radius(&self) -> f64 {
        let t = self.options.target.trim();
        if let Some(rest) = t.strip_prefix("DISTANCE;") {
            return rest
                .trim_end_matches(';')
                .trim()
                .parse::<f64>()
                .unwrap_or(0.0);
        }
        0.0
    }

    /// The tier the legacy renderer actually uses: the first unconditional
    /// tier, else the first tier as a fallback (same selection as
    /// [`legacy_template`]).
    pub fn render_layer(&self) -> Option<&FormatLayer> {
        self.formats
            .iter()
            .find(|l| l.condition.is_empty() || l.condition == "~")
            .or_else(|| self.formats.first())
    }
}

/// The full configuration snapshot loaded from the YAML files.
#[derive(Debug, Clone, Default)]
pub struct TrChatConfig {
    pub settings: Settings,
    /// All registered channels, in declaration order.
    pub channels: Vec<ChannelConfig>,
    /// Private-message templates (legacy-flattened from the `Private` channel)
    /// consumed by the `/msg` command.
    pub msg: PrivateMessageFormats,
}

#[derive(Debug, Clone, Default)]
pub struct PrivateMessageFormats {
    pub sender: String,
    pub receiver: String,
}

/// Routing result of [`TrChatConfig::route`].
#[derive(Debug, Clone)]
pub enum Route<'a> {
    /// The message matched a channel (by prefix, or the default channel).
    Channel(&'a ChannelConfig, String),
    /// No channel is configured at all — render with the plain fallback.
    Plain(String),
}

impl TrChatConfig {
    // ---- flat accessors kept for the chat pipeline / commands ---- //

    pub fn message_max_length(&self) -> u32 {
        self.settings.chat.message_max_length
    }
    pub fn cooldown_millis(&self) -> u64 {
        self.settings.chat.cooldown_millis
    }
    pub fn anti_repeat_similarity(&self) -> f64 {
        self.settings.chat.anti_repeat_similarity
    }
    pub fn anti_repeat_period_millis(&self) -> u64 {
        self.settings.chat.anti_repeat_period_millis
    }
    pub fn blocked_words(&self) -> &[String] {
        &self.settings.chat.blocked_words
    }
    pub fn filter_replacement(&self) -> &str {
        &self.settings.chat.filter_replacement
    }
    pub fn default_language(&self) -> &str {
        &self.settings.chat.default_language
    }
    pub fn server_name(&self) -> &str {
        &self.settings.chat.server_name
    }
    /// The `chat.globalPrefix` setting, mirrored from the Mod for config
    /// completeness; the Global channel file already declares the `!all`
    /// prefix in its `Bindings`, so routing does not read this field.
    #[allow(dead_code)]
    pub fn global_prefix(&self) -> &str {
        &self.settings.chat.global_prefix
    }
    pub fn channels(&self) -> &[ChannelConfig] {
        &self.channels
    }

    pub fn channel_by_id(&self, id: &str) -> Option<&ChannelConfig> {
        self.channels.iter().find(|c| c.id.eq_ignore_ascii_case(id))
    }

    /// The default channel: the one with `Options.Auto-Join: true`, falling
    /// back to the first registered channel (matching the upstream rule that
    /// every message lands somewhere).
    pub fn default_channel(&self) -> Option<&ChannelConfig> {
        self.channels
            .iter()
            .find(|c| c.options.auto_join)
            .or_else(|| self.channels.first())
    }

    /// Fallback template when no channel is configured (legacy `Plain` route).
    pub fn plain_template(&self) -> String {
        "&f{player}: {message}".to_string()
    }

    /// Routes a chat message: longest matching prefix wins, unprefixed
    /// messages fall through to the default (auto-join) channel.
    pub fn route(&self, message: &str) -> Route<'_> {
        let trimmed = message.trim_start();
        // Tracks the matched prefix length so the longest prefix wins even
        // when a shorter prefix matched first.
        let mut best: Option<(&ChannelConfig, usize, &str)> = None;
        for channel in &self.channels {
            for prefix in &channel.bindings.prefix {
                if prefix.is_empty() {
                    continue; // an empty prefix can never match
                }
                if let Some(rest) = trimmed.strip_prefix(prefix.as_str()) {
                    if best.is_none_or(|(_, len, _)| prefix.len() > len) {
                        best = Some((channel, prefix.len(), rest));
                    }
                }
            }
        }
        if let Some((channel, _, rest)) = best {
            return Route::Channel(channel, rest.trim_start().to_string());
        }
        if let Some(channel) = self.default_channel() {
            return Route::Channel(channel, trimmed.to_string());
        }
        Route::Plain(trimmed.to_string())
    }
}

/// Thread-safe handle to the current configuration; cheap to clone and passed
/// into the chat event handler.
#[derive(Clone)]
pub struct SharedConfig(Arc<RwLock<TrChatConfig>>);

impl SharedConfig {
    pub fn read(&self) -> RwLockReadGuard<'_, TrChatConfig> {
        self.0.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Loads the data-folder YAML files (seeding defaults on first run) and
    /// returns the shared handle.
    pub fn load(context: &Context) -> Result<Self, String> {
        let folder = context.get_data_folder();
        let config = load_from_folder(&folder)?;
        let default_language = config.default_language().to_string();
        crate::lang::lang_init(&folder, &default_language);
        crate::special::reload(Path::new(&folder))?;
        Ok(Self(Arc::new(RwLock::new(config))))
    }

    /// Re-reads every YAML file from disk and swaps the snapshot in place.
    pub fn reload(&self, folder: &str) -> Result<(), String> {
        let config = load_from_folder(folder)?;
        let default_language = config.default_language().to_string();
        crate::lang::lang_init(folder, &default_language);
        crate::special::reload(Path::new(folder))?;
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = config;
        Ok(())
    }
}

// ---- process-wide config handle (command surface) ----

static GLOBAL_CONFIG: OnceLock<SharedConfig> = OnceLock::new();
static DATA_FOLDER: OnceLock<String> = OnceLock::new();

/// Seeds the process-wide handle used by the command surface.
pub fn init_global(config: &SharedConfig, data_folder: String) {
    let _ = GLOBAL_CONFIG.set(config.clone());
    let _ = DATA_FOLDER.set(data_folder);
}

/// Access to the process-wide config handle (commands, `/trchat reload`).
pub fn global_config() -> &'static SharedConfig {
    GLOBAL_CONFIG.get_or_init(|| SharedConfig(Arc::new(RwLock::new(TrChatConfig::default()))))
}

/// `/trchat reload` — re-reads the YAML files from the data folder.
pub fn reload_global() -> Result<(), String> {
    let folder = DATA_FOLDER
        .get()
        .ok_or_else(|| "trchat config is not initialized yet".to_string())?;
    global_config().reload(folder)
}

// ---- loaders ----

/// Full loader: seeds the data folder with bundled defaults (first run),
/// then parses `settings.yml` and `channels/*.yml`.
pub fn load_from_folder(folder: &str) -> Result<TrChatConfig, String> {
    let root = Path::new(folder);
    let channels_dir = root.join("channels");
    let lang_dir = root.join("lang");
    fs::create_dir_all(&channels_dir).map_err(|e| format!("create channels dir: {e}"))?;
    fs::create_dir_all(&lang_dir).map_err(|e| format!("create lang dir: {e}"))?;

    write_default(
        &root.join("settings.yml"),
        defaults::SETTINGS,
        "settings.yml",
    )?;
    write_default(
        &root.join("datasource.yml"),
        defaults::DATASOURCE,
        "datasource.yml",
    )?;
    write_default(&root.join("filter.yml"), defaults::FILTER, "filter.yml")?;
    write_default(
        &root.join("function.yml"),
        defaults::FUNCTION,
        "function.yml",
    )?;
    write_default(
        &root.join("special-chars.yml"),
        defaults::SPECIAL_CHARS,
        "special-chars.yml",
    )?;
    for (name, content) in defaults::CHANNELS {
        write_default(
            &channels_dir.join(format!("{name}.yml")),
            content,
            "channels/*.yml",
        )?;
    }
    for (name, content) in defaults::LANGS {
        write_default(&lang_dir.join(format!("{name}.yml")), content, "lang/*.yml")?;
    }

    // settings.yml
    let raw = fs::read_to_string(root.join("settings.yml"))
        .map_err(|e| format!("read settings.yml: {e}"))?;
    let settings: Settings =
        serde_yaml::from_str(&raw).map_err(|e| format!("parse settings.yml: {e}"))?;

    // channels/*.yml, in deterministic order; Example/Schema never load.
    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in fs::read_dir(&channels_dir).map_err(|e| format!("read channels dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read channels entry: {e}"))?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.ends_with(".yml") && !file.ends_with(".yaml") {
            continue;
        }
        let id = file
            .rsplit_once('.')
            .map(|(stem, _)| stem.to_string())
            .unwrap_or_else(|| file.clone());
        if id.eq_ignore_ascii_case("Example") || id.eq_ignore_ascii_case("Schema") {
            continue; // reference files — never registered as channels
        }
        let content =
            fs::read_to_string(entry.path()).map_err(|e| format!("read channels/{file}: {e}"))?;
        entries.push((id, content));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let channels = parse_channels(&entries);
    let msg = private_formats(&channels);

    Ok(TrChatConfig {
        settings,
        channels,
        msg,
    })
}

fn write_default(path: &Path, content: &str, label: &str) -> Result<(), String> {
    if !path.exists() {
        fs::write(path, content).map_err(|e| format!("write {label}: {e}"))?;
    }
    Ok(())
}

fn parse_channels(files: &[(String, String)]) -> Vec<ChannelConfig> {
    let mut out = Vec::new();
    for (id, raw) in files {
        let doc: serde_yaml::Value = match serde_yaml::from_str(raw) {
            Ok(doc) => doc,
            Err(e) => {
                eprintln!("[trchat] channels/{id}.yml skipped (parse error): {e}");
                continue;
            }
        };
        let options = parse_options(doc.get("Options"));
        let bindings = parse_bindings(doc.get("Bindings"));
        let formats = parse_layers(doc.get("Formats"));
        let sender = parse_layers(doc.get("Sender"));
        let receiver = parse_layers(doc.get("Receiver"));
        let console = parse_layers(doc.get("Console"));
        let template =
            legacy_template(&formats).unwrap_or_else(|| "&f{player}: {message}".to_string());
        out.push(ChannelConfig {
            id: id.clone(),
            options,
            bindings,
            formats,
            sender,
            receiver,
            console,
            template,
        });
    }
    out
}

fn parse_options(v: Option<&serde_yaml::Value>) -> ChannelOptions {
    fn s(v: Option<&serde_yaml::Value>, key: &str) -> String {
        v.and_then(|d| d.get(key))
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    }
    fn b(v: Option<&serde_yaml::Value>, key: &str) -> bool {
        v.and_then(|d| d.get(key))
            .and_then(|x| x.as_bool())
            .unwrap_or(false)
    }
    fn list(v: Option<&serde_yaml::Value>, key: &str) -> Vec<String> {
        match v.and_then(|d| d.get(key)) {
            Some(serde_yaml::Value::String(s)) => vec![s.clone()],
            Some(serde_yaml::Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        }
    }
    fn ports(v: Option<&serde_yaml::Value>, key: &str) -> Vec<u16> {
        match v.and_then(|d| d.get(key)) {
            Some(serde_yaml::Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|x| x.as_u64().map(|n| n as u16))
                .collect(),
            _ => Vec::new(),
        }
    }
    ChannelOptions {
        join_permission: s(v, "Join-Permission"),
        listen_permission: s(v, "Listen-Permission"),
        speak_condition: s(v, "Speak-Condition"),
        always_listen: b(v, "Always-Listen"),
        auto_join: b(v, "Auto-Join"),
        private: b(v, "Private"),
        target: s(v, "Target").if_empty("ALL"),
        proxy: b(v, "Proxy"),
        force_proxy: b(v, "Force-Proxy"),
        double_transfer: b(v, "Double-Transfer"),
        ports: ports(v, "Ports"),
        disabled_functions: list(v, "Disabled-Functions"),
    }
}

trait IfEmpty {
    fn if_empty(self, default: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, default: &str) -> String {
        if self.is_empty() {
            default.to_string()
        } else {
            self
        }
    }
}

fn parse_bindings(v: Option<&serde_yaml::Value>) -> ChannelBindings {
    fn list(v: Option<&serde_yaml::Value>, key: &str) -> Vec<String> {
        match v.and_then(|d| d.get(key)) {
            Some(serde_yaml::Value::String(s)) => vec![s.clone()],
            Some(serde_yaml::Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        }
    }
    ChannelBindings {
        prefix: list(v, "Prefix"),
        command: list(v, "Command"),
    }
}

/// Parses a `Formats` / `Sender` / `Receiver` / `Console` list of tiers.
fn parse_layers(v: Option<&serde_yaml::Value>) -> Vec<FormatLayer> {
    let Some(serde_yaml::Value::Sequence(seq)) = v else {
        return Vec::new();
    };
    let mut layers = Vec::with_capacity(seq.len());
    for tier in seq {
        let msg = tier.get("msg");
        let special = msg.and_then(|m| m.get("special-char"));
        layers.push(FormatLayer {
            condition: tier
                .get("condition")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string(),
            priority: tier.get("priority").and_then(|p| p.as_i64()).unwrap_or(0),
            prefix: parse_prefix(tier.get("prefix")),
            msg_default_color: msg
                .and_then(|m| m.get("default-color"))
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string(),
            special_char_color: if special
                .and_then(|s| s.get("enabled"))
                .and_then(|e| e.as_bool())
                .unwrap_or(false)
            {
                special
                    .and_then(|s| s.get("special-char-color"))
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_string()
            } else {
                String::new()
            },
        });
    }
    layers
}

/// Parses a `prefix:` mapping into ordered parts. Component groups with a
/// list of variants (`player:`) yield every variant in order; the legacy
/// renderer later keeps only the unconditional ones.
fn parse_prefix(v: Option<&serde_yaml::Value>) -> Vec<PrefixPart> {
    let Some(serde_yaml::Value::Mapping(map)) = v else {
        return Vec::new();
    };
    // Deterministic order: known groups first in the Mod's layout order, then
    // unknown groups sorted by name.
    const ORDER: &[&str] = &[
        "server",
        "world",
        "channel",
        "part-before-player",
        "spacer",
        "player",
        "part-before-msg",
        "separator",
        "main",
        "sender",
        "receiver",
        "console",
        "staff",
    ];
    let mut entries: Vec<(usize, &serde_yaml::Value)> = map
        .iter()
        .map(|(k, v)| {
            let name = k.as_str().unwrap_or("");
            let order = ORDER.iter().position(|n| *n == name).unwrap_or(usize::MAX);
            (order, v)
        })
        .collect();
    entries.sort_by_key(|(order, _)| *order);

    let mut parts = Vec::new();
    for (_, v) in entries {
        match v {
            serde_yaml::Value::String(text) if !text.is_empty() => {
                parts.push(PrefixPart {
                    condition: String::new(),
                    text: text.clone(),
                });
            }
            serde_yaml::Value::Mapping(m) => {
                parts.push(part_from_map(m));
            }
            serde_yaml::Value::Sequence(seq) => {
                parts.extend(seq.iter().filter_map(|v| match v {
                    serde_yaml::Value::Mapping(m) => Some(part_from_map(m)),
                    _ => None,
                }));
            }
            _ => {}
        }
    }
    parts
}

fn part_from_map(m: &serde_yaml::Mapping) -> PrefixPart {
    PrefixPart {
        text: map_str(m, "text").to_string(),
        condition: map_str(m, "condition").to_string(),
    }
}

fn map_str<'a>(m: &'a serde_yaml::Mapping, key: &str) -> &'a str {
    m.get(&serde_yaml::Value::String(key.to_string()))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// Flattens the unconditional tier of a format list into one legacy template
/// (`&` codes + `{player}` / `{message}` placeholders), which is what the
/// current string renderer understands. Hover / click / condition features
/// are deliberately lost here and documented as a format-parser follow-up.
fn legacy_template(layers: &[FormatLayer]) -> Option<String> {
    let layer = layers
        .iter()
        .find(|l| l.condition.is_empty() || l.condition == "~")
        .or_else(|| layers.first())?;
    let mut out = String::new();
    for part in &layer.prefix {
        if !part.condition.is_empty() && part.condition != "~" {
            continue; // cannot evaluate conditions — keep catch-all variants
        }
        out.push_str(&part.text);
    }
    out.push_str(&color_code(&layer.msg_default_color));
    out.push_str("{message}");
    Some(normalize_placeholders(&out))
}

/// `7` / `f` / `&7` / `&f` → `&7` / `&f`; anything else (or empty) → `""`.
pub(crate) fn color_code(color: &str) -> String {
    let c = color.strip_prefix('&').unwrap_or(color).trim();
    if c.chars().count() == 1 {
        format!("&{c}")
    } else {
        String::new()
    }
}

/// Maps the Mod's placeholder tokens to the renderer's `{…}` names and drops
/// unresolved `%…%` tokens (PAPI etc. that a console renderer cannot fill).
fn normalize_placeholders(s: &str) -> String {
    let s = s
        .replace("%player_name%", "{player}")
        .replace("%display_name%", "{player}")
        .replace("%player%", "{player}")
        .replace("%message%", "{message}")
        .replace("%trchat_toplayer%", "{target}")
        .replace("%trchat_player%", "{player}")
        .replace("%server_name%", "{server}")
        .replace("%player_world%", "{world}");
    strip_unknown_placeholders(&s)
}

fn strip_unknown_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut token = String::new();
        let mut closed = false;
        for n in chars.by_ref() {
            if n == '%' {
                closed = true;
                break;
            }
            token.push(n);
        }
        if !closed || token.is_empty() {
            // a bare '%' or an unterminated token — keep as-is
            out.push('%');
            out.push_str(&token);
        }
        // else: known %…% was already normalized; unknown tokens are dropped
    }
    out
}

/// Derives the private-message templates from the first `Private` channel.
fn private_formats(channels: &[ChannelConfig]) -> PrivateMessageFormats {
    let Some(pm) = channels.iter().find(|c| c.options.private) else {
        let plain = "{player}: {message}".to_string();
        return PrivateMessageFormats {
            sender: plain.clone(),
            receiver: plain,
        };
    };
    PrivateMessageFormats {
        sender: legacy_template(&pm.sender).unwrap_or_else(|| "{player}: {message}".to_string()),
        receiver: legacy_template(&pm.receiver)
            .unwrap_or_else(|| "{player}: {message}".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::defaults;
    use super::*;

    /// Every bundled default must be parseable YAML so a fresh data folder
    /// never ships a broken file.
    #[test]
    fn bundled_defaults_are_valid_yaml() {
        for (name, content) in [
            ("settings", defaults::SETTINGS),
            ("datasource", defaults::DATASOURCE),
            ("filter", defaults::FILTER),
            ("function", defaults::FUNCTION),
            ("special-chars", defaults::SPECIAL_CHARS),
        ] {
            serde_yaml::from_str::<serde_yaml::Value>(content)
                .unwrap_or_else(|e| panic!("{name} is not valid YAML: {e}"));
        }
        for (name, content) in defaults::CHANNELS {
            serde_yaml::from_str::<serde_yaml::Value>(content)
                .unwrap_or_else(|e| panic!("channel {name} is not valid YAML: {e}"));
        }
        for (name, content) in defaults::LANGS {
            serde_yaml::from_str::<serde_yaml::Value>(content)
                .unwrap_or_else(|e| panic!("lang {name} is not valid YAML: {e}"));
        }
    }

    /// `load_from_folder` seeds a fresh folder and parses the Mod defaults.
    #[test]
    fn loads_bundled_defaults_with_the_mod_layout() {
        let dir = temp_dir("loads_defaults");
        let config = load_from_folder(&dir).expect("defaults must load");

        // settings.yml keys
        assert_eq!(config.message_max_length(), 256);
        assert_eq!(config.cooldown_millis(), 2000);
        assert!((config.anti_repeat_similarity() - 0.85).abs() < 1e-9);
        assert_eq!(config.default_language(), "zh_CN");
        assert_eq!(config.server_name(), "A Minecraft Server");
        assert_eq!(config.global_prefix(), "!all");
        assert_eq!(config.filter_replacement(), "*");
        assert!(config.blocked_words().is_empty());

        // channels/*.yml: Normal, Global, Staff, Private; Example/Schema skipped
        let ids: Vec<&str> = config.channels().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["Global", "Normal", "Private", "Staff"]);

        // Normal carries the default route and full legacy template
        let normal = config.channel_by_id("Normal").expect("Normal exists");
        assert!(normal.is_default());
        assert!(normal.template.contains("{player}"));
        assert!(normal.template.contains("{message}"));

        // Global binds the !all prefix
        let global = config.channel_by_id("Global").expect("Global exists");
        assert_eq!(global.bindings.prefix, vec!["!all".to_string()]);
        assert!(global.options.proxy);
        assert!(global.template.contains("{server}"));

        // Private carries the /msg templates
        let private = config.channel_by_id("Private").expect("Private exists");
        assert!(private.options.private);
        assert!(config.msg.sender.contains("{target}"));
        assert!(config.msg.sender.contains("{message}"));
    }

    #[test]
    fn longest_matching_prefix_wins() {
        let config = sample_config(|c| {
            c.channels = vec![
                test_channel("Test", vec!["!", "!!"], Default::default()),
                test_channel("Other", vec!["#"], Default::default()),
            ];
            c.channels[0].options.auto_join = true;
        });
        match config.route("!!hello") {
            Route::Channel(ch, rest) => {
                assert_eq!(ch.id, "Test");
                assert_eq!(rest, "hello");
            }
            _ => panic!("expected a channel route"),
        }
    }

    #[test]
    fn prefix_strips_and_trims() {
        let config = sample_config(|c| {
            c.channels = vec![test_channel("Cmd", vec!["!cmd"], Default::default())];
            c.channels[0].options.auto_join = true;
        });
        match config.route("!cmd   hi there") {
            Route::Channel(ch, rest) => {
                assert_eq!(ch.id, "Cmd");
                assert_eq!(rest, "hi there");
            }
            _ => panic!("expected a channel route"),
        }
    }

    #[test]
    fn unmatched_message_uses_the_default_channel() {
        let config = sample_config(|c| {
            c.channels = vec![
                test_channel("Normal", vec![], Default::default()),
                test_channel("Global", vec!["!all"], Default::default()),
            ];
            c.channels[0].options.auto_join = true;
        });
        match config.route("plain hello") {
            Route::Channel(ch, rest) => {
                assert_eq!(ch.id, "Normal");
                assert_eq!(rest, "plain hello");
            }
            _ => panic!("expected the default channel"),
        }
    }

    #[test]
    fn without_channels_the_message_is_plain() {
        let config = sample_config(|c| c.channels.clear());
        match config.route("hello") {
            Route::Plain(msg) => assert_eq!(msg, "hello"),
            _ => panic!("expected a plain route"),
        }
    }

    #[test]
    fn empty_prefix_never_matches() {
        let config = sample_config(|c| {
            c.channels = vec![
                test_channel("Weird", vec![""], Default::default()),
                test_channel("Normal", vec![], Default::default()),
            ];
            c.channels[1].options.auto_join = true;
        });
        match config.route("hello") {
            Route::Channel(ch, _) => assert_eq!(ch.id, "Normal"),
            _ => panic!("empty prefix must not match; default channel wins"),
        }
    }

    #[test]
    fn channel_by_id_is_case_insensitive() {
        let config = sample_config(|c| {
            c.channels = vec![test_channel("Normal", vec![], Default::default())];
        });
        assert!(config.channel_by_id("normal").is_some());
        assert!(config.channel_by_id("NORMAL").is_some());
        assert!(config.channel_by_id("Missing").is_none());
    }

    #[test]
    fn radius_parses_from_target() {
        let mut ch = test_channel("A", vec![], Default::default());
        ch.options.target = "DISTANCE;30".to_string();
        assert_eq!(ch.radius(), 30.0);
        ch.options.target = "ALL".to_string();
        assert_eq!(ch.radius(), 0.0);
        ch.options.target = "DISTANCE;".to_string();
        assert_eq!(ch.radius(), 0.0);
    }

    fn sample_config(edit: impl FnOnce(&mut TrChatConfig)) -> TrChatConfig {
        let mut config = TrChatConfig::default();
        edit(&mut config);
        config
    }

    fn test_channel(id: &str, prefixes: Vec<&str>, options: ChannelOptions) -> ChannelConfig {
        ChannelConfig {
            id: id.to_string(),
            options,
            bindings: ChannelBindings {
                prefix: prefixes.into_iter().map(str::to_string).collect(),
                command: Vec::new(),
            },
            formats: Vec::new(),
            sender: Vec::new(),
            receiver: Vec::new(),
            console: Vec::new(),
            template: "&f{player}: {message}".to_string(),
        }
    }

    fn temp_dir(tag: &str) -> String {
        let dir =
            std::env::temp_dir().join(format!("trchat-pumpkin-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); // clean from a previous run
        dir.to_string_lossy().into_owned()
    }
}
