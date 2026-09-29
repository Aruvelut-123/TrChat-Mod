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

use pumpkin_plugin_api::text::TextComponent;
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
///
/// §4.4/§4.5 — a component part may also carry a hover and exactly one click
/// action. The click actions are consulted in the spec's priority order
/// (`suggest` > `command` > `url` > `copy` > `file`) by
/// [`PrefixPart::click_action`].
///
/// The component renderer still emits plain legacy text, so these fields are
/// parsed and validated now but not yet attached to the outgoing component;
/// [`PrefixPart::click_action`] is the accessor that wiring will call.
#[derive(Debug, Clone, Default)]
pub struct PrefixPart {
    pub condition: String,
    pub text: String,
    /// `hover` — hover text (multi-line `|-` supported upstream).
    #[allow(dead_code)]
    pub hover: String,
    /// `suggest` — click inserts this command into the chat box.
    #[allow(dead_code)]
    pub suggest: String,
    /// `command` — click runs this command.
    #[allow(dead_code)]
    pub command: String,
    /// `url` — click opens this link (trimmed, cut at the first space).
    #[allow(dead_code)]
    pub url: String,
    /// `copy` — click copies this text to the clipboard.
    #[allow(dead_code)]
    pub copy: String,
    /// `file` — click opens this local path.
    #[allow(dead_code)]
    pub file: String,
    /// `insertion` — shift-click inserted text.
    #[allow(dead_code)]
    pub insertion: String,
    /// `font` — resource font (`ResourceLocation`).
    #[allow(dead_code)]
    pub font: String,
}

/// The click action a component part contributes, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAction {
    Suggest(String),
    RunCommand(String),
    OpenUrl(String),
    CopyToClipboard(String),
    OpenFile(String),
}

impl ClickAction {
    /// Applies this action to a component, returning the rebound handle.
    ///
    /// Every `TextComponent` mutator consumes the handle, so the caller threads
    /// the component through instead of mutating in place (§4.5).
    pub fn apply(&self, component: TextComponent) -> TextComponent {
        match self {
            ClickAction::Suggest(c) => component.click_suggest_command(c),
            ClickAction::RunCommand(c) => component.click_run_command(c),
            ClickAction::OpenUrl(u) => component.click_open_url(u),
            ClickAction::CopyToClipboard(t) => component.click_copy_to_clipboard(t),
            ClickAction::OpenFile(p) => component.click_open_file(p),
        }
    }
}

impl PrefixPart {
    /// §4.5 — the first non-empty click action in the Mod's priority order:
    /// `suggest` > `command` > `url` > `copy` > `file`. A `url` is trimmed and
    /// cut at the first space, and must parse as a `URI` or it is dropped.
    pub fn click_action(&self) -> Option<ClickAction> {
        if !self.suggest.is_empty() {
            return Some(ClickAction::Suggest(self.suggest.clone()));
        }
        if !self.command.is_empty() {
            return Some(ClickAction::RunCommand(self.command.clone()));
        }
        if !self.url.is_empty() {
            return valid_url(&self.url).map(ClickAction::OpenUrl);
        }
        if !self.copy.is_empty() {
            return Some(ClickAction::CopyToClipboard(self.copy.clone()));
        }
        if !self.file.is_empty() {
            return Some(ClickAction::OpenFile(self.file.clone()));
        }
        None
    }

    /// Whether this part should be rendered at all: the legacy renderer keeps
    /// unconditional parts and parts whose condition it cannot evaluate.
    #[allow(dead_code)]
    pub fn is_rendered(&self) -> bool {
        self.condition.is_empty() || self.condition == "~"
    }
}

/// §4.5 — `url`: `trim()`, truncate at the first space, then require that the
/// result parses as a `URI` (`new URI(url)` succeeding upstream). Anything
/// else yields no click event at all.
#[allow(dead_code)]
pub fn valid_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let candidate = trimmed.split(' ').next().unwrap_or("").trim();
    if candidate.is_empty() {
        return None;
    }
    // A URI needs a scheme and no illegal characters; the sandbox has no URI
    // parser, so accept only well-formed absolute URLs.
    let (scheme, rest) = candidate.split_once(':')?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return None;
    }
    if candidate.chars().any(|c| c.is_whitespace() || c == '"' || c == '<' || c == '>') {
        return None;
    }
    Some(candidate.to_string())
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
    /// `datasource.yml` — where moderation/ignore state persists (Mod-side
    /// `PlayerDataStore`); parsed for the future state-store wiring.
    #[allow(dead_code)] // consumed by the state-store follow-up
    pub datasource: DataSourceConfig,
    /// `function.yml` — command controller rules + built-in/custom chat
    /// functions; parsed for the future function-executor wiring.
    #[allow(dead_code)] // consumed by the chat-functions follow-up
    pub function: FunctionConfig,
    /// `filter.yml` — the chat filter profile (Local words, punctuation
    /// skipping, white list, replacement) consumed by the chat pipeline.
    pub filter: FilterConfig,
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
    /// The `filter.yml` chat-filter profile (consumed by the pipeline).
    pub fn filter_config(&self) -> &FilterConfig {
        &self.filter
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

    /// §2.2 `ChannelManager.byCommand` — the first channel (in id order) whose
    /// `Bindings.Command` contains `command`, compared case-insensitively.
    pub fn channel_by_command(&self, command: &str) -> Option<&ChannelConfig> {
        self.channels
            .iter()
            .find(|c| c.bindings.command.iter().any(|a| a.eq_ignore_ascii_case(command)))
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

    // datasource.yml / function.yml — parsed for future wiring (state store,
    // command controller, chat functions); defaults are seeded above.
    let mut datasource = DataSourceConfig::default();
    let raw = fs::read_to_string(root.join("datasource.yml"))
        .map_err(|e| format!("read datasource.yml: {e}"))?;
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) {
        datasource = parse_datasource(&value);
    }
    let mut function = FunctionConfig::default();
    let raw = fs::read_to_string(root.join("function.yml"))
        .map_err(|e| format!("read function.yml: {e}"))?;
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) {
        function = parse_function(&value);
    }
    let mut filter = FilterConfig::default();
    let raw =
        fs::read_to_string(root.join("filter.yml")).map_err(|e| format!("read filter.yml: {e}"))?;
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) {
        filter = parse_filter(&value);
    }

    Ok(TrChatConfig {
        settings,
        channels,
        msg,
        datasource,
        function,
        filter,
    })
}

fn write_default(path: &Path, content: &str, label: &str) -> Result<(), String> {
    if !path.exists() {
        fs::write(path, content).map_err(|e| format!("write {label}: {e}"))?;
    }
    Ok(())
}

// ---- datasource.yml (Mod `PlayerDataStore`) ---- //

/// Connection settings of a network database section (MySQL/MariaDB/PostgreSQL).
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the state-store follow-up
pub struct NetworkDatabase {
    pub host: String,
    pub port: i64,
    pub database: String,
    pub user: String,
    pub password: String,
    pub parameters: String,
}

/// Advanced/custom JDBC section.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the state-store follow-up
pub struct JdbcDatabase {
    pub driver: String,
    pub url: String,
    pub user: String,
    pub password: String,
    pub table_prefix: String,
}

/// `datasource.yml` — which store moderation/ignore state survives in.
/// Parsed for the future state-store wiring; the WASM sandbox has no JDBC
/// driver, so the values are kept as data only.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the state-store follow-up
pub struct DataSourceConfig {
    /// `Type` — SQLite / MySQL / MariaDB / PostgreSQL / JDBC.
    pub data_type: String,
    /// `SQLite.File` (relative to the data folder when not absolute).
    pub sqlite_file: String,
    pub mysql: NetworkDatabase,
    pub mariadb: NetworkDatabase,
    pub postgresql: NetworkDatabase,
    pub jdbc: JdbcDatabase,
}

impl DataSourceConfig {
    /// The configured `Type`, lowercased (Mod treats it case-insensitively).
    #[allow(dead_code)] // consumed by the state-store follow-up
    pub fn kind(&self) -> String {
        self.data_type.to_ascii_lowercase()
    }
}

fn parse_network_database(v: Option<&serde_yaml::Value>) -> NetworkDatabase {
    let Some(v) = v else {
        return NetworkDatabase::default();
    };
    let get = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let get_i = |key: &str| v.get(key).and_then(|x| x.as_i64()).unwrap_or(0);
    NetworkDatabase {
        host: get("Host"),
        port: get_i("Port"),
        database: get("Database"),
        user: get("User"),
        password: get("Password"),
        parameters: get("Parameters"),
    }
}

fn parse_datasource(v: &serde_yaml::Value) -> DataSourceConfig {
    let get = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    DataSourceConfig {
        data_type: get("Type"),
        sqlite_file: v
            .get("SQLite")
            .and_then(|s| s.get("File"))
            .and_then(|f| f.as_str())
            .unwrap_or_default()
            .to_string(),
        mysql: parse_network_database(v.get("MySQL")),
        mariadb: parse_network_database(v.get("MariaDB")),
        postgresql: parse_network_database(v.get("PostgreSQL")),
        jdbc: {
            let j = v.get("JDBC");
            JdbcDatabase {
                driver: j
                    .and_then(|x| x.get("Driver"))
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                url: j
                    .and_then(|x| x.get("Url"))
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                user: j
                    .and_then(|x| x.get("User"))
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                password: j
                    .and_then(|x| x.get("Password"))
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                table_prefix: j
                    .and_then(|x| x.get("Table-Prefix"))
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
            }
        },
    }
}

// ---- function.yml (Mod `ChatFunctionService` / `CommandController`) ---- //

/// One `General.Command-Controller.List` rule (a command pattern to intercept).
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the chat-functions follow-up
pub struct CommandRule {
    /// The raw source entry (kept for docs/debugging).
    pub source: String,
    /// The pattern before the first `{…}` property block.
    pub pattern: String,
    /// `{exact: true}` → match the whole input, not just the command label.
    pub exact: bool,
    /// `{condition: …}` — not evaluated by this port yet.
    pub condition: String,
    /// `{cooldown: N}` in seconds, converted to milliseconds.
    pub cooldown_millis: i64,
}

/// `General.Command-Controller`.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the chat-functions follow-up
pub struct CommandControllerConfig {
    pub enabled: bool,
    pub rules: Vec<CommandRule>,
}

/// A built-in general function (`Mention`, `Mention-All`, `Item-Show`, …).
/// `Mention` / `Mention-All` are consumed by the chat pipeline
/// (`functions::process`); the remaining fields belong to the follow-ups.
#[derive(Debug, Clone, Default)]
pub struct GeneralFunctionConfig {
    /// Section name under `General`, e.g. `Mention`.
    pub name: String,
    pub enabled: bool,
    pub permission: String,
    pub cooldown_millis: i64,
    pub notify: bool,
    pub self_mention: bool,
    /// `Pattern` — only meaningful for `Mention`.
    pub pattern: String,
    pub keys: Vec<String>,
    #[allow(dead_code)] // `actions` — consumed by the runActions follow-up
    pub actions: Vec<String>,
    /// `Origin-Name` / `Compatible` / `UI` — Item-Show only.
    pub origin_name: bool,
    pub compatible: bool,
    pub ui: bool,
}

/// The clickable display of a custom function.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the chat-functions follow-up
pub struct FunctionDisplay {
    pub text: String,
    pub hover: String,
    pub suggest: String,
    pub command: String,
    pub url: String,
    pub copy: String,
}

/// A `Custom.<name>` entry (regex function).
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the chat-functions follow-up
pub struct CustomFunctionConfig {
    pub name: String,
    pub condition: String,
    pub priority: i64,
    pub pattern: String,
    pub text_filter: String,
    pub permission: String,
    pub cooldown_millis: i64,
    pub actions: Vec<String>,
    pub display: FunctionDisplay,
}

/// `function.yml` — parsed for the future function-executor wiring.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // consumed by the chat-functions follow-up
pub struct FunctionConfig {
    pub command_controller: CommandControllerConfig,
    /// Built-in `General` functions other than `Command-Controller`, in
    /// declaration order.
    pub general: Vec<GeneralFunctionConfig>,
    /// `Custom` entries, sorted by priority descending (Mod behavior).
    pub custom: Vec<CustomFunctionConfig>,
}

/// `filter.yml` — the chat filter profile (the Mod's `FilterService.Settings`).
#[derive(Debug, Clone, Default)]
pub struct FilterConfig {
    /// `Enable.Chat` — whether chat messages are filtered (default `true`).
    pub chat_enabled: bool,
    /// `Enable.Sign` — whether sign text is filtered (default `true`).
    #[allow(dead_code)] // sign/anvil filtering is out of scope for chat-only Pumpkin
    pub sign_enabled: bool,
    /// `Enable.Anvil` — whether anvil renames are filtered (default `true`).
    #[allow(dead_code)] // sign/anvil filtering is out of scope for chat-only Pumpkin
    pub anvil_enabled: bool,
    /// `Cloud-Thesaurus.Enabled` — remote thesaurus refresh (default `true`).
    #[allow(dead_code)] // network fetch is out of scope for the WASM sandbox
    pub cloud_enabled: bool,
    /// `Cloud-Thesaurus.Urls` — thesaurus endpoints (default empty).
    #[allow(dead_code)] // network fetch is out of scope for the WASM sandbox
    pub cloud_urls: Vec<String>,
    /// `Cloud-Thesaurus.Ignored` — words never added from the cloud, lowercased.
    #[allow(dead_code)] // network fetch is out of scope for the WASM sandbox
    pub cloud_ignored: Vec<String>,
    /// `Local` — the local sensitive word list.
    pub local_words: Vec<String>,
    /// `Ignored-Punctuations` — characters skipped while matching, lowercased.
    pub ignored_punctuations: Vec<char>,
    /// `WhiteList` — phrases protected from replacement.
    pub white_list: Vec<String>,
    /// `Replacement` — the censoring char (default `*`).
    pub replacement: char,
}

/// Parses `filter.yml` with the Mod's defaults (`FilterService.Settings.from`).
fn parse_filter(v: &serde_yaml::Value) -> FilterConfig {
    let enable = v
        .get("Enable")
        .and_then(|x| x.as_mapping())
        .cloned()
        .unwrap_or_default();
    let cloud = v
        .get("Cloud-Thesaurus")
        .and_then(|x| x.as_mapping())
        .cloned()
        .unwrap_or_default();
    let bool_of = |m: &serde_yaml::Mapping, key: &str, fallback: bool| -> bool {
        m.get(key).and_then(|x| x.as_bool()).unwrap_or(fallback)
    };
    let strings_of = |m: &serde_yaml::Mapping, key: &str| -> Vec<String> {
        match m.get(key) {
            Some(serde_yaml::Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|x| x.as_str())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        }
    };
    let strings_of_root = |key: &str| -> Vec<String> {
        match v.get(key) {
            Some(serde_yaml::Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|x| x.as_str())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        }
    };
    let replacement = match v.get("Replacement").and_then(|x| x.as_str()) {
        Some(s) if !s.is_empty() => s.chars().next().unwrap_or('*'),
        _ => '*',
    };
    // The Mod folds every character of every punctuation string into the set.
    let punctuation = match v.get("Ignored-Punctuations") {
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|x| x.as_str())
            .flat_map(|s| s.chars())
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect(),
        _ => Vec::new(),
    };
    FilterConfig {
        chat_enabled: bool_of(&enable, "Chat", true),
        sign_enabled: bool_of(&enable, "Sign", true),
        anvil_enabled: bool_of(&enable, "Anvil", true),
        cloud_enabled: bool_of(&cloud, "Enabled", true),
        cloud_urls: strings_of(&cloud, "Urls"),
        cloud_ignored: strings_of(&cloud, "Ignored")
            .iter()
            .map(|s| s.to_lowercase())
            .collect(),
        local_words: strings_of_root("Local"),
        ignored_punctuations: punctuation,
        white_list: strings_of_root("WhiteList"),
        replacement,
    }
}

/// Parses `{key: value}` property blocks from a command-rule source string
/// (Mod `CommandController.PROPERTY`).
fn command_properties(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        let inner = &after[..end];
        if let Some(colon) = inner.find(':') {
            let key = inner[..colon].trim().to_ascii_lowercase();
            let value = inner[colon + 1..].trim().to_string();
            out.push((key, value));
        }
        rest = &after[end + 1..];
    }
    out
}

/// `30s` / `5m` / `2h` / `1d` / plain number → milliseconds (Mod `durationMillis`).
fn duration_millis(value: &str) -> i64 {
    let value = value.trim();
    if value.is_empty() {
        return 0;
    }
    let digits = value
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    let number: i64 = digits.parse().unwrap_or(0);
    let unit = value[digits.len()..].to_ascii_lowercase();
    match unit.as_str() {
        "s" => number * 1_000,
        "m" => number * 60_000,
        "h" => number * 3_600_000,
        "d" => number * 86_400_000,
        // "" and anything unrecognised → plain milliseconds (Mod default arm).
        _ => number,
    }
}

/// `{cooldown: N}` — a plain number in *seconds* (Mod `secondsMillis`).
fn seconds_millis(value: &str) -> i64 {
    value
        .trim()
        .parse::<f64>()
        .map(|f| (f * 1000.0).round() as i64)
        .unwrap_or(0)
}

fn yaml_str(v: &serde_yaml::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

fn yaml_bool(v: &serde_yaml::Value, key: &str, fallback: bool) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(fallback)
}

fn yaml_strings(v: &serde_yaml::Value, key: &str) -> Vec<String> {
    match v.get(key) {
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.to_string())
            .collect(),
        Some(serde_yaml::Value::String(s)) => {
            vec![s.to_string()]
        }
        _ => Vec::new(),
    }
}

fn parse_general_function(name: &str, v: &serde_yaml::Value) -> GeneralFunctionConfig {
    let mut actions = yaml_strings(v, "Action");
    actions.extend(yaml_strings(v, "Actions"));
    GeneralFunctionConfig {
        name: name.to_string(),
        enabled: yaml_bool(v, "Enabled", true),
        permission: yaml_str(v, "Permission"),
        // `Permission: 'none'` in defaults; treat as empty.
        cooldown_millis: duration_millis(&yaml_str(v, "Cooldown")),
        notify: yaml_bool(v, "Notify", true),
        self_mention: yaml_bool(v, "Self-Mention", false),
        pattern: yaml_str(v, "Pattern"),
        keys: yaml_strings(v, "Keys"),
        actions,
        origin_name: yaml_bool(v, "Origin-Name", false),
        compatible: yaml_bool(v, "Compatible", false),
        ui: yaml_bool(v, "UI", false),
    }
}

fn parse_function(v: &serde_yaml::Value) -> FunctionConfig {
    let general = v.get("General").unwrap_or(&serde_yaml::Value::Null);
    let controller = general
        .get("Command-Controller")
        .unwrap_or(&serde_yaml::Value::Null);
    let mut rules = Vec::new();
    if let Some(serde_yaml::Value::Sequence(list)) = controller.get("List") {
        for item in list {
            if let Some(source) = item.as_str() {
                if source.is_empty() {
                    continue;
                }
                let end = source.find('{').unwrap_or(source.len());
                let pattern = source[..end].trim().to_string();
                let mut exact = false;
                let mut condition = String::new();
                let mut cooldown = 0i64;
                for (key, value) in command_properties(source) {
                    match key.as_str() {
                        "exact" => exact = value.eq_ignore_ascii_case("true"),
                        "condition" => condition = value,
                        "cooldown" => cooldown = seconds_millis(&value),
                        _ => {}
                    }
                }
                rules.push(CommandRule {
                    source: source.to_string(),
                    pattern,
                    exact,
                    condition,
                    cooldown_millis: cooldown,
                });
            }
        }
    }

    let mut general_functions = Vec::new();
    if let Some(map) = general.as_mapping() {
        for (key, value) in map {
            let Some(name) = key.as_str() else {
                continue;
            };
            if name == "Command-Controller" {
                continue;
            }
            general_functions.push(parse_general_function(name, value));
        }
    }

    let mut custom = Vec::new();
    if let Some(map) = v.get("Custom").and_then(|c| c.as_mapping()) {
        for (key, value) in map {
            let Some(name) = key.as_str() else {
                continue;
            };
            let display = value.get("display").unwrap_or(&serde_yaml::Value::Null);
            let mut actions = yaml_strings(value, "action");
            actions.extend(yaml_strings(value, "actions"));
            actions.extend(yaml_strings(value, "Action"));
            actions.extend(yaml_strings(value, "Actions"));
            let text_filter = yaml_str(value, "text-filter");
            custom.push(CustomFunctionConfig {
                name: name.to_string(),
                condition: yaml_str(value, "condition"),
                priority: value.get("priority").and_then(|p| p.as_i64()).unwrap_or(0),
                pattern: yaml_str(value, "pattern"),
                text_filter,
                permission: yaml_str(value, "permission"),
                cooldown_millis: duration_millis(&yaml_str(value, "cooldown")),
                actions,
                display: FunctionDisplay {
                    text: yaml_str(display, "text"),
                    hover: display
                        .get("hover")
                        .and_then(|h| {
                            h.as_str().map(|s| s.to_string()).or_else(|| {
                                h.as_sequence().map(|seq| {
                                    seq.iter()
                                        .filter_map(|x| x.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                            })
                        })
                        .unwrap_or_default(),
                    suggest: yaml_str(display, "suggest"),
                    command: yaml_str(display, "command"),
                    url: yaml_str(display, "url"),
                    copy: yaml_str(display, "copy"),
                },
            });
        }
        custom.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    FunctionConfig {
        command_controller: CommandControllerConfig {
            enabled: yaml_bool(controller, "Enabled", true),
            rules,
        },
        general: general_functions,
        custom,
    }
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
                // A bare string entry carries no hover/click fields.
                parts.push(PrefixPart {
                    text: text.clone(),
                    ..Default::default()
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
        // §4.4/§4.5 — the optional hover and click/insertion/font fields.
        hover: map_str(m, "hover").to_string(),
        suggest: map_str(m, "suggest").to_string(),
        command: map_str(m, "command").to_string(),
        url: map_str(m, "url").to_string(),
        copy: map_str(m, "copy").to_string(),
        file: map_str(m, "file").to_string(),
        insertion: map_str(m, "insertion").to_string(),
        font: map_str(m, "font").to_string(),
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
    let layer = select_layer(layers)?;
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

/// Picks the tier the legacy renderer uses: the first unconditional tier, or
/// the first tier at all when none is unconditional.
fn select_layer(layers: &[FormatLayer]) -> Option<&FormatLayer> {
    layers
        .iter()
        .find(|l| l.condition.is_empty() || l.condition == "~")
        .or_else(|| layers.first())
}

/// The renderable prefix parts of the tier the legacy renderer picks, in YAML
/// order. `chat.rs` walks these to attach each part's hover/click event, which
/// the flattened [`legacy_template`] string cannot carry.
pub fn selected_prefix_parts(layers: &[FormatLayer]) -> Vec<PrefixPart> {
    select_layer(layers)
        .map(|layer| layer.prefix.iter().filter(|p| p.is_rendered()).cloned().collect())
        .unwrap_or_default()
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

    /// §2.2 `ChannelManager.byCommand` — the factory bindings resolve to the
    /// right channel, matching is case-insensitive, and unbound aliases miss.
    #[test]
    fn channel_by_command_resolves_factory_bindings() {
        let dir = temp_dir("by_command");
        let config = load_from_folder(&dir).expect("defaults must load");

        // Global is bound to global/all/shout.
        for alias in ["global", "all", "shout"] {
            let found = config
                .channel_by_command(alias)
                .unwrap_or_else(|| panic!("{alias} should resolve"));
            assert_eq!(found.id, "Global", "alias {alias}");
        }
        // Case-insensitive (Java `equalsIgnoreCase`).
        assert_eq!(config.channel_by_command("GLOBAL").unwrap().id, "Global");
        assert_eq!(config.channel_by_command("Shout").unwrap().id, "Global");

        // Staff and Private are likewise bound.
        assert_eq!(config.channel_by_command("staff").unwrap().id, "Staff");
        assert_eq!(config.channel_by_command("tell").unwrap().id, "Private");

        // Normal has no `Command` binding, and unknown aliases miss.
        assert!(config.channel_by_command("normal").is_none());
        assert!(config.channel_by_command("nope").is_none());
        // An empty alias never matches a configured binding.
        assert!(config.channel_by_command("").is_none());
    }

    /// Every channel alias is distinct across channels, so `byCommand` never
    /// depends on iteration order for the factory set.
    #[test]
    fn factory_command_aliases_are_unique() {
        let dir = temp_dir("aliases_unique");
        let config = load_from_folder(&dir).expect("defaults must load");
        let mut seen: Vec<&str> = Vec::new();
        for channel in config.channels() {
            for alias in &channel.bindings.command {
                assert!(
                    !seen.iter().any(|s| s.eq_ignore_ascii_case(alias)),
                    "alias {alias} bound to more than one channel"
                );
                seen.push(alias);
            }
        }
        assert!(!seen.is_empty(), "factory config should bind some aliases");
    }

    /// §4.5 — click actions are consulted in the Mod's priority order:
    /// `suggest` > `command` > `url` > `copy` > `file`.
    #[test]
    fn click_action_follows_spec_priority() {
        let all = PrefixPart {
            suggest: "/s".into(),
            command: "/c".into(),
            url: "https://e.com/".into(),
            copy: "cp".into(),
            file: "/f".into(),
            ..Default::default()
        };
        assert_eq!(all.click_action(), Some(ClickAction::Suggest("/s".into())));

        let no_suggest = PrefixPart {
            suggest: String::new(),
            ..all.clone()
        };
        assert_eq!(
            no_suggest.click_action(),
            Some(ClickAction::RunCommand("/c".into()))
        );

        let only_url = PrefixPart {
            command: String::new(),
            url: "https://e.com/".into(),
            ..no_suggest
        };
        assert_eq!(
            only_url.click_action(),
            Some(ClickAction::OpenUrl("https://e.com/".into()))
        );

        // A part with no action yields none, and `console`/`text` alone is fine.
        assert_eq!(PrefixPart::default().click_action(), None);
    }

    /// §4.5 — `url` is trimmed, cut at the first space, and must be a valid URI
    /// or the part contributes no click event at all.
    #[test]
    fn url_is_trimmed_cut_and_validated() {
        assert_eq!(valid_url("  https://a.com/x  "), Some("https://a.com/x".into()));
        assert_eq!(
            valid_url("https://a.com/x with spaces"),
            Some("https://a.com/x".into())
        );
        // No scheme / empty / illegal characters → dropped.
        assert_eq!(valid_url("not a url"), None);
        assert_eq!(valid_url(""), None);
        assert_eq!(valid_url("   "), None);
        assert_eq!(valid_url("https:"), None);
        assert_eq!(valid_url("sch eme://x"), None);

        // A part whose only action is an invalid url has no click action.
        let bad = PrefixPart {
            url: "not a url".into(),
            ..Default::default()
        };
        assert_eq!(bad.click_action(), None);
    }

    /// §4.4/§4.5 — `selected_prefix_parts` feeds the click/hover wiring in
    /// `chat.rs`, so it must pick the same tier and order as the flattened
    /// template and skip conditional parts.
    #[test]
    fn selected_prefix_parts_matches_the_flattened_tier() {
        let layers = vec![
            FormatLayer {
                condition: "perm \"trchat.staff\"".into(),
                priority: 10,
                prefix: vec![PrefixPart {
                    text: "&c[Staff]".into(),
                    ..Default::default()
                }],
                msg_default_color: "f".into(),
                special_char_color: String::new(),
            },
            FormatLayer {
                condition: "~".into(),
                priority: 0,
                prefix: vec![
                    PrefixPart {
                        text: "&8[&fSite&8] ".into(),
                        hover: "Click".into(),
                        url: "https://example.com/".into(),
                        ..Default::default()
                    },
                    PrefixPart {
                        // A conditional part cannot be evaluated, so it is
                        // dropped from the renderable set.
                        condition: "player op".into(),
                        text: "&7[OP]".into(),
                        ..Default::default()
                    },
                ],
                msg_default_color: "7".into(),
                special_char_color: String::new(),
            },
        ];

        let parts = selected_prefix_parts(&layers);
        assert_eq!(parts.len(), 1, "only the unconditional, renderable part");
        assert_eq!(parts[0].text, "&8[&fSite&8] ");
        assert_eq!(parts[0].hover, "Click");
        assert_eq!(
            parts[0].click_action(),
            Some(ClickAction::OpenUrl("https://example.com/".into())),
            "the part keeps its click action for the renderer to attach"
        );

        // An empty list renders no parts and no clickable prefix.
        assert!(selected_prefix_parts(&[]).is_empty());
    }

    /// §4.4/§4.5 — the component-part fields survive parsing from YAML.
    #[test]
    fn component_part_fields_are_parsed() {
        let yaml = r#"
text: "&8[&fSite&8]"
hover: "Click me"
url: "https://example.com/"
insertion: "inserted"
font: "minecraft:default"
"#;
        let m = match serde_yaml::from_str::<serde_yaml::Value>(yaml).unwrap() {
            serde_yaml::Value::Mapping(m) => m,
            other => panic!("expected a mapping, got {other:?}"),
        };
        let part = part_from_map(&m);
        assert_eq!(part.text, "&8[&fSite&8]");
        assert_eq!(part.hover, "Click me");
        assert_eq!(part.url, "https://example.com/");
        assert_eq!(part.insertion, "inserted");
        assert_eq!(part.font, "minecraft:default");
        assert_eq!(
            part.click_action(),
            Some(ClickAction::OpenUrl("https://example.com/".into()))
        );
        assert!(part.is_rendered(), "an unconditional part must render");
    }

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

    #[test]
    fn datasource_defaults_parse() {
        let value: serde_yaml::Value = serde_yaml::from_str(defaults::DATASOURCE).unwrap();
        let ds = parse_datasource(&value);
        assert_eq!(ds.kind(), "sqlite");
        assert_eq!(ds.sqlite_file, "data.db");
        assert_eq!(ds.mysql.host, "127.0.0.1");
        assert_eq!(ds.mysql.port, 3306);
        assert_eq!(ds.mysql.database, "trchat");
        assert_eq!(
            ds.mysql.parameters,
            "useUnicode=true&characterEncoding=utf8&useSSL=false&serverTimezone=UTC"
        );
        assert_eq!(ds.postgresql.port, 5432);
        assert_eq!(ds.mariadb.user, "root");
        assert_eq!(ds.jdbc.table_prefix, "trchat_");
        assert!(ds.jdbc.driver.is_empty()); // JDBC auto-discovery

        // Type switches between sections without touching the others.
        let mut switched = ds.clone();
        switched.data_type = "MySQL".to_string();
        assert_eq!(switched.kind(), "mysql");
        assert_eq!(switched.sqlite_file, "data.db");
    }

    #[test]
    fn function_defaults_parse() {
        let value: serde_yaml::Value = serde_yaml::from_str(defaults::FUNCTION).unwrap();
        let f = parse_function(&value);

        // Command controller: 4 rules, exact + condition + cooldown props.
        let cc = &f.command_controller;
        assert!(cc.enabled);
        assert_eq!(cc.rules.len(), 4);
        let arasple = &cc.rules[0];
        assert_eq!(arasple.pattern, "arasple");
        assert!(arasple.exact);
        assert_eq!(arasple.condition, "perm \"trchat.admin\"");
        assert_eq!(arasple.cooldown_millis, 0);
        let ver = &cc.rules[1];
        assert!(!ver.exact);
        assert_eq!(ver.pattern, "ver(sion)?(s)?");
        let shout = &cc.rules[3];
        assert_eq!(shout.pattern, "shout");
        assert_eq!(shout.cooldown_millis, 3000); // `{cooldown: 3}` seconds

        // General functions keep declaration order.
        let names: Vec<&str> = f.general.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Mention",
                "Mention-All",
                "Item-Show",
                "Inventory-Show",
                "EnderChest-Show"
            ]
        );
        let mentionall = f.general.iter().find(|g| g.name == "Mention-All").unwrap();
        assert!(mentionall.enabled);
        assert_eq!(mentionall.permission, "trchat.function.mentionall");
        assert_eq!(mentionall.cooldown_millis, 300_000); // '5m'
        assert_eq!(
            mentionall.keys,
            vec!["@all", "@everyone", "@everybody", "@所有人", "@全体成员"]
        );

        // Custom functions sorted by priority descending.
        assert_eq!(f.custom.first().unwrap().name, "shareUrl");
        assert_eq!(f.custom.first().unwrap().priority, 100);
        assert_eq!(f.custom.first().unwrap().display.text, "&8[&f&l网站&8]");
        assert_eq!(
            f.custom.first().unwrap().display.hover.contains("点击进入"),
            true
        );
        assert_eq!(f.custom.first().unwrap().display.url, "{0}");
        let glow_email = f.custom.iter().find(|c| c.name == "glowEmail").unwrap();
        assert_eq!(glow_email.cooldown_millis, 5000); // '5s'
        assert_eq!(glow_email.display.copy, "{0}");
        // hidePhoneNumber has no priority → sorts after the priority-100 ones.
        let hide_phone = f
            .custom
            .iter()
            .find(|c| c.name == "hidePhoneNumber")
            .unwrap();
        assert_eq!(hide_phone.priority, 0);
        assert_eq!(hide_phone.display.text, "&8[&c&m-&8]");
    }

    #[test]
    fn properties_parsed_from_rule_source() {
        let props = command_properties("arasple{exact: true}{condition: perm \"trchat.admin\"}");
        assert_eq!(
            props,
            vec![
                ("exact".to_string(), "true".to_string()),
                ("condition".to_string(), "perm \"trchat.admin\"".to_string()),
            ]
        );
        assert_eq!(duration_millis("30s"), 30_000);
        assert_eq!(duration_millis("5m"), 300_000);
        assert_eq!(duration_millis("2h"), 7_200_000);
        assert_eq!(duration_millis("1d"), 86_400_000);
        assert_eq!(duration_millis(""), 0);
        assert_eq!(seconds_millis("3"), 3000);
    }

    #[test]
    fn filter_defaults_parse() {
        let value: serde_yaml::Value = serde_yaml::from_str(defaults::FILTER).unwrap();
        let f = parse_filter(&value);
        assert!(f.chat_enabled && f.sign_enabled && f.anvil_enabled);
        assert!(f.cloud_enabled);
        assert_eq!(f.cloud_urls.len(), 1);
        assert_eq!(f.cloud_ignored, vec!["nt"]);
        assert_eq!(f.local_words, vec!["NMSL", "fuck", "shit"]);
        assert_eq!(f.white_list, vec!["has been"]);
        assert_eq!(f.replacement, '*');
        // Punctuation: every char of every entry is folded into the set,
        // including full-width punctuation and multi-char entries like `——`.
        for c in ['!', '。', '！', '　', '—', '…', '`', '\\'] {
            assert!(f.ignored_punctuations.contains(&c), "missing {c:?}");
        }
    }

    #[test]
    fn filter_custom_overrides_and_defaults() {
        let value: serde_yaml::Value = serde_yaml::from_str(
            r#"
Enable:
  Chat: false
Cloud-Thesaurus:
  Enabled: false
  Ignored: ['ABC']
Local: ['HeLLo']
Ignored-Punctuations: ['A', '。']
WhiteList: []
Replacement: ''
"#,
        )
        .unwrap();
        let f = parse_filter(&value);
        assert!(!f.chat_enabled);
        assert!(f.sign_enabled); // absent → Mod default true
        assert!(f.anvil_enabled);
        assert!(!f.cloud_enabled);
        assert_eq!(f.cloud_ignored, vec!["abc"]); // lowercased like the Mod
        assert_eq!(f.local_words, vec!["HeLLo"]); // preserved verbatim
        assert_eq!(f.ignored_punctuations, vec!['a', '。']);
        assert_eq!(f.replacement, '*'); // blank → '*'
    }
}
