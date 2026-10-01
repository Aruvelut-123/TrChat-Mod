//! `updates:` — the Mod's GitHub release update checker.
//!
//! Ports `update/UpdateChecker.java`, `update/SemanticVersion.java`,
//! `update/ReleaseNotes.java` and `update/ReleaseNoteRenderer.java`
//! (fact spec: `docs/spec/data-redis-update.md` §5).
//!
//! Like the Mod, the checker only *notifies* and never downloads a file
//! (`TrChatConfig.java:129-132`): it GETs the latest GitHub release, compares it
//! against the version this port tracks, and tells the console and every online
//! `trchat.admin` holder about it once.
//!
//! Two sandbox differences, both recorded in the spec:
//!
//! * the Mod runs the GET on a dedicated background thread
//!   (`"TrChat Update Checker"`, `UpdateChecker.java:45-49`); a WASM guest is
//!   single threaded, so the request is issued from a repeating server task and
//!   the host's `wasi:http` client blocks that task until the response or the
//!   request timeout — hence `updates.intervalMinutes` is clamped to at least
//!   one minute and a CAS guard keeps only one request in flight;
//! * the stable WIT feedback channel carries plain components (no click/hover
//!   action), so `Updater-Link-Prefix` + `Updater-Link` are appended as text
//!   where the Mod attaches a clickable, hoverable link. `Updater-Link-Hover`
//!   is therefore resolved but unused.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Mutex, OnceLock};

use pumpkin_plugin_api::{
    events::player::PlayerJoinEvent,
    events::{EventData, EventHandler, EventPriority},
    logging::{log, LogLevel},
    player::Player,
    scheduler::SchedulerExt,
    text::TextComponent,
    Context, Server,
};

use crate::lang::Lang;

/// Host of the "latest release" endpoint (`UpdateChecker.java:31-33`).
///
/// The full endpoint is
/// `https://api.github.com/repos/Aruvelut-123/TrChat-Mod/releases/latest`.
pub const API_AUTHORITY: &str = "api.github.com";
/// Path of the "latest release" endpoint.
pub const API_PATH: &str = "/repos/Aruvelut-123/TrChat-Mod/releases/latest";
/// Page used when the release payload carries no `html_url` (`:34-35`).
pub const RELEASES_URL: &str = "https://github.com/Aruvelut-123/TrChat-Mod/releases";
/// `Accept` header of the API request (`:99`).
pub const ACCEPT_HEADER: &str = "application/vnd.github+json";
/// `User-Agent` prefix (`:100`); the tracked version is appended.
pub const USER_AGENT_PREFIX: &str = "TrChat-Mod/";
/// Request timeout in seconds (`:98`, `:41-44`).
pub const REQUEST_TIMEOUT_SECONDS: u64 = 30;
/// The first check runs one minute after load (`:63-70`).
pub const INITIAL_DELAY_MINUTES: u32 = 1;
/// `updates.intervalMinutes` bounds (`TrChatConfig.java:137-140`).
pub const MIN_INTERVAL_MINUTES: u32 = 1;
/// `updates.intervalMinutes` bounds (`TrChatConfig.java:137-140`).
pub const MAX_INTERVAL_MINUTES: u32 = 1440;
/// Game ticks in one minute (20 ticks/s), the WIT scheduler unit.
const TICKS_PER_MINUTE: u64 = 20 * 60;
/// Bytes read per `blocking-read` call while draining the response body.
const READ_CHUNK_BYTES: u64 = 8192;
/// The bare admin node (`TrChatPermissions.check(player, "trchat.admin")`);
/// lookups qualify it through [`crate::perms::node`].
const ADMIN_NODE: &str = "trchat.admin";

/// The TrChat version this port tracks.
///
/// Filled in by `build.rs` from `mod_version` in the repository
/// `gradle.properties`, so `/trchat version` and the update check report the
/// same version the Bukkit/NeoForge Mod does instead of the crate's own.
pub const CURRENT_VERSION: &str = env!("TRCHAT_VERSION");

/// The release parsed out of one GitHub payload (`UpdateChecker.check` `:107-112`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// `tag_name` without its leading `v` (`:116`).
    pub version: String,
    /// `html_url`, or [`RELEASES_URL`].
    pub url: String,
    /// `ReleaseNotes.normalize(body)` — empty when GitHub sent no notes.
    pub notes: Vec<String>,
}

/// How the latest release compares to the running version (`:114-137`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// `latest > current` — a newer release exists.
    Newer,
    /// `latest == current`.
    Current,
    /// `current > latest` — a build ahead of the newest release.
    Ahead,
}

/// The release discovered by the last successful check, if any
/// (`UpdateChecker.available`).
static AVAILABLE: Mutex<Option<ReleaseInfo>> = Mutex::new(None);

/// Players already told about [`AVAILABLE`] (`UpdateChecker.notified`).
///
/// The Mod keys this set on the player UUID; the stable WIT `uuid` type has no
/// string form, so the lowercased player name is used — the same substitution
/// the chat pipeline makes for its per-player state.
static NOTIFIED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// `UpdateChecker.checking` — only one request may be in flight (`:50`, `:93-95`).
static CHECKING: AtomicBool = AtomicBool::new(false);

/// `UpdateChecker.reportedCurrent` — the "up to date" line is logged once.
static REPORTED_CURRENT: AtomicBool = AtomicBool::new(false);

fn notified() -> &'static Mutex<HashSet<String>> {
    NOTIFIED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Starts the update checker: the repeating task plus the join notifier.
///
/// Mirrors `TrChatServerEvents.java:43-46, 83-85` — with `updates.enabled:
/// false` the Mod never even constructs the checker, so neither does this port.
pub fn start(context: &Context) -> Result<(), String> {
    let (enabled, interval_minutes) = {
        let config = crate::config::global_config().read();
        (
            config.settings.updates.enabled,
            config.settings.updates.interval_minutes,
        )
    };
    if !enabled {
        log(
            LogLevel::Info,
            "[TrChat] Update checker disabled (updates.enabled: false).",
        );
        return Ok(());
    }

    // `UpdateChecker.notifyPlayer` is called from the player-login hook
    // (`TrChatServerEvents.java:83-85`).
    context
        .register_event_handler::<PlayerJoinEvent, JoinNotifyHandler>(
            JoinNotifyHandler,
            EventPriority::Normal,
            true,
        )
        .map_err(|error| error.to_string())?;

    let interval = interval_minutes.clamp(MIN_INTERVAL_MINUTES, MAX_INTERVAL_MINUTES);
    context.schedule_repeating_task(
        u64::from(INITIAL_DELAY_MINUTES) * TICKS_PER_MINUTE,
        u64::from(interval) * TICKS_PER_MINUTE,
        |server| check(&server),
    );
    log(
        LogLevel::Info,
        &format!("[TrChat] Update checker started (every {interval} minute(s))."),
    );
    Ok(())
}

/// Notifies an admin who has just joined (`UpdateChecker.notifyPlayer`).
struct JoinNotifyHandler;

impl EventHandler<PlayerJoinEvent> for JoinNotifyHandler {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        notify_player(&event.player);
        event
    }
}

/// One check cycle, run by the repeating task.
///
/// The CAS guard mirrors `UpdateChecker.check` `:93-95`: a cycle that finds a
/// request already running returns without touching the network.
fn check(server: &Server) {
    if CHECKING.swap(true, AtomicOrdering::SeqCst) {
        return;
    }
    let result = fetch_latest(CURRENT_VERSION);
    CHECKING.store(false, AtomicOrdering::SeqCst);

    match result {
        Ok(body) => apply_payload(server, &body),
        // `UpdateChecker.java:140-141`.
        Err(error) => log(
            LogLevel::Warn,
            &format!("[TrChat] Unable to check TrChat updates: {error}"),
        ),
    }
}

/// Compares a fetched payload against the running version (`:107-137`).
fn apply_payload(server: &Server, body: &str) {
    let payload = match parse_payload(body) {
        Ok(payload) => payload,
        Err(error) => {
            log(
                LogLevel::Warn,
                &format!("[TrChat] Unable to check TrChat updates: {error}"),
            );
            return;
        }
    };

    let current = SemanticVersion::parse(CURRENT_VERSION);
    let latest = SemanticVersion::parse(&payload.tag_name);

    match verdict_of(&current, &latest) {
        Verdict::Newer => {
            let release = payload.into_release();
            // Only a *different* release re-notifies (`:118-123`).
            let changed = {
                let mut available = lock(&AVAILABLE);
                let changed = available.as_ref() != Some(&release);
                *available = Some(release.clone());
                changed
            };
            if changed {
                lock(notified()).clear();
                notify_available(server, &release);
            }
        }
        Verdict::Current | Verdict::Ahead => {
            *lock(&AVAILABLE) = None;
            if !REPORTED_CURRENT.swap(true, AtomicOrdering::SeqCst) {
                if verdict_of(&current, &latest) == Verdict::Ahead {
                    log(
                        LogLevel::Info,
                        &format!(
                            "[TrChat] TrChat {CURRENT_VERSION} is newer than the latest GitHub release {}.",
                            payload.tag_name
                        ),
                    );
                } else {
                    log(
                        LogLevel::Info,
                        &format!("[TrChat] TrChat {CURRENT_VERSION} is up to date."),
                    );
                }
            }
        }
    }
}

/// Console WARN + broadcast to every online admin (`notifyAvailable`, `:147-160`).
fn notify_available(server: &Server, release: &ReleaseInfo) {
    let notes = if release.notes.is_empty() {
        "No release notes provided.".to_string()
    } else {
        release.notes.join("\n")
    };
    log(
        LogLevel::Warn,
        &format!(
            "[TrChat] TrChat update available: {CURRENT_VERSION} -> {} ({})\n{notes}",
            release.version, release.url
        ),
    );
    for player in server.get_all_players() {
        notify_player(&player);
    }
}

/// Sends the update block to `player` once (`notifyPlayer`, `:72-90`).
fn notify_player(player: &Player) {
    let release = lock(&AVAILABLE).clone();
    let Some(release) = release else {
        return;
    };
    if !is_admin(player) {
        return;
    }
    {
        let mut notified = lock(notified());
        if !notified.insert(player.get_name().to_ascii_lowercase()) {
            return;
        }
    }

    let table = crate::lang::lang().read().unwrap_or_else(|e| e.into_inner());
    for line in notification_lines(&table, &release, CURRENT_VERSION, "") {
        let _ = player
            .send_system_message(TextComponent::from_legacy_string_with_code(&line, '&'), false);
    }
}

/// `TrChatPermissions.check(player, "trchat.admin")` — OP level 2 or the node.
///
/// The node is registered with `PermissionDefault::Op(Two)`, so a positive
/// answer is exactly "OP 2+, or explicitly granted". The YAML/Mod spelling is
/// bare, hence [`crate::perms::node`].
fn is_admin(player: &Player) -> bool {
    player.has_permission(&crate::perms::node(ADMIN_NODE))
}

/// The message block of `notifyPlayer` (`:79-88`) plus the Mod's `header`
/// (`:162-185`), as one string per feedback component.
pub fn notification_lines(
    table: &Lang,
    release: &ReleaseInfo,
    current_text: &str,
    locale: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    // `Updater-Available` is a two-line value and the Mod appends the link block
    // after a newline, so the header spans three rendered lines.
    for line in table
        .format("Updater-Available", locale, &[current_text, &release.version])
        .split('\n')
    {
        lines.push(line.to_string());
    }
    lines.push(format!(
        "{}{}",
        table.format("Updater-Link-Prefix", locale, &[]),
        table.format("Updater-Link", locale, &[])
    ));
    lines.push(table.format("Updater-Changelog", locale, &[]));
    if release.notes.is_empty() {
        lines.push(table.format("Updater-Changelog-Empty", locale, &[]));
    } else {
        for note in &release.notes {
            lines.push(render_note(note));
        }
    }
    lines.push(table.format("Status-Footer", locale, &[]));
    lines
}

/// One GitHub release payload (`tag_name` / `html_url` / `body`, `:107-112`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ReleasePayload {
    /// Required release tag; a payload without it is rejected like the Mod's
    /// failed `getAsString()` (`:108`).
    pub tag_name: String,
    /// Optional release page; `null` or absent falls back to [`RELEASES_URL`].
    #[serde(default)]
    pub html_url: Option<String>,
    /// Optional release notes; `null` or absent means "no notes" (`:110-112`).
    #[serde(default)]
    pub body: Option<String>,
}

impl ReleasePayload {
    /// Builds the record the notifier works with (`:115-117`).
    pub fn into_release(self) -> ReleaseInfo {
        ReleaseInfo {
            version: strip_tag_prefix(&self.tag_name).to_string(),
            url: self
                .html_url
                .unwrap_or_else(|| RELEASES_URL.to_string()),
            notes: normalize(self.body.as_deref().unwrap_or("")),
        }
    }
}

/// Parses the GitHub response body.
pub fn parse_payload(body: &str) -> Result<ReleasePayload, String> {
    serde_json::from_str(body).map_err(|error| format!("invalid GitHub response: {error}"))
}

/// `tag_name.replaceFirst("^[vV]", "")` (`:116`).
pub fn strip_tag_prefix(tag: &str) -> &str {
    tag.strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag)
}

/// The Mod's comparison result for a running version and a release tag.
pub fn verdict_of(current: &SemanticVersion, latest: &SemanticVersion) -> Verdict {
    match latest.cmp(current) {
        Ordering::Greater => Verdict::Newer,
        Ordering::Equal => Verdict::Current,
        Ordering::Less => Verdict::Ahead,
    }
}

/// Java's `\s` — `[ \t\n\x0B\f\r]`, the class the Mod's patterns use (the `regex`
/// crate's `\s` is Unicode-aware and would also match e.g. NBSP).
fn is_java_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// `SemanticVersion` (`update/SemanticVersion.java`) — the version ordering the
/// update check relies on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticVersion {
    numbers: Vec<i64>,
    pre_release: Vec<String>,
}

impl SemanticVersion {
    /// `SemanticVersion.parse` (`:17-39`); never fails.
    pub fn parse(value: &str) -> Self {
        let normalized = value.trim().to_lowercase();
        let normalized = normalized.strip_prefix('v').unwrap_or(&normalized);
        // Build metadata is dropped entirely (`:22`).
        let normalized = match normalized.find('+') {
            Some(index) => &normalized[..index],
            None => normalized,
        };
        let (numbers_part, pre_part) = match normalized.find('-') {
            Some(index) => (&normalized[..index], Some(&normalized[index + 1..])),
            None => (normalized, None),
        };

        let mut numbers: Vec<i64> = split_java(&numbers_part, '.')
            .into_iter()
            .map(leading_number)
            .collect();
        if numbers.is_empty() {
            numbers.push(0);
        }
        let pre_release = match pre_part {
            // A blank pre-release (`1.0.0-`) counts as none (`:35-37`).
            Some(part) if part.trim().is_empty() => Vec::new(),
            Some(part) => split_java(part, '.').into_iter().map(String::from).collect(),
            None => Vec::new(),
        };
        Self {
            numbers,
            pre_release,
        }
    }
}

/// `String.split(regex)` — like Java, trailing empty fields are dropped.
fn split_java(value: &str, separator: char) -> Vec<&str> {
    let mut parts: Vec<&str> = value.split(separator).collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// `Integer.parseInt(part.replaceFirst("^(\\d+).*$", "$1"))` with the
/// `NumberFormatException` fallback to `0` (`:25-30`).
fn leading_number(part: &str) -> i64 {
    let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

impl Ord for SemanticVersion {
    /// `SemanticVersion.compareTo` (`:41-79`).
    fn cmp(&self, other: &Self) -> Ordering {
        let size = self.numbers.len().max(other.numbers.len());
        for index in 0..size {
            let left = self.numbers.get(index).copied().unwrap_or(0);
            let right = other.numbers.get(index).copied().unwrap_or(0);
            match left.cmp(&right) {
                Ordering::Equal => {}
                other => return other,
            }
        }

        // A release outranks its own pre-releases (`:52-57`).
        match (self.pre_release.is_empty(), other.pre_release.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }

        // The shorter pre-release is smaller (`:58-61`).
        let size = self.pre_release.len().max(other.pre_release.len());
        for index in 0..size {
            let Some(left) = self.pre_release.get(index) else {
                return Ordering::Less;
            };
            let Some(right) = other.pre_release.get(index) else {
                return Ordering::Greater;
            };
            let left_numeric = all_digits(left);
            let right_numeric = all_digits(right);
            let compared = match (left_numeric, right_numeric) {
                (true, true) => numeric(left).cmp(&numeric(right)),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => left.cmp(right),
            };
            if compared != Ordering::Equal {
                return compared;
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `Character::isDigit` over the whole segment. Java's `allMatch` is vacuously
/// true for an empty segment (which then crashes on `Long.parseLong`); an empty
/// segment is treated as non-numeric here instead of panicking.
fn all_digits(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

/// `Long.parseLong`, saturating instead of overflowing.
fn numeric(value: &str) -> i64 {
    value.parse().unwrap_or(i64::MAX)
}

/// `ReleaseNotes.normalize` (`:18-36`) — blank lines trimmed off both ends,
/// every remaining line right-trimmed, line-leading whitespace preserved.
pub fn normalize(body: &str) -> Vec<String> {
    if body.trim().is_empty() {
        return Vec::new();
    }
    let unified = body.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = unified.split('\n').collect();
    let mut first = 0;
    let mut last = lines.len();
    while first < last && lines[first].trim().is_empty() {
        first += 1;
    }
    while last > first && lines[last - 1].trim().is_empty() {
        last -= 1;
    }
    lines[first..last]
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect()
}

/// `ReleaseNotes.LineType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    /// `## heading`
    LevelTwoHeading,
    /// `### heading`
    LevelThreeHeading,
    /// `- item`
    ListItem,
    /// Anything else.
    Text,
    /// Whitespace only.
    Blank,
}

/// `ReleaseNotes.FormattedLine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedLine {
    /// The line's kind.
    pub kind: LineType,
    /// The captured text (headings already lost their closing markers).
    pub text: String,
}

/// `ReleaseNotes.parseLine` (`:38-55`): blanks, then level-three headings, then
/// level-two headings, then list items, then plain text.
pub fn parse_line(line: &str) -> FormattedLine {
    if line.trim().is_empty() {
        return FormattedLine {
            kind: LineType::Blank,
            text: String::new(),
        };
    }
    // The Mod's `(?!#)` is implied: the marker must be followed by whitespace.
    if let Some(text) = match_marked(line, "###") {
        return FormattedLine {
            kind: LineType::LevelThreeHeading,
            text: heading_text(&text),
        };
    }
    if let Some(text) = match_marked(line, "##") {
        return FormattedLine {
            kind: LineType::LevelTwoHeading,
            text: heading_text(&text),
        };
    }
    if let Some(text) = match_marked(line, "-") {
        return FormattedLine {
            kind: LineType::ListItem,
            text,
        };
    }
    FormattedLine {
        kind: LineType::Text,
        text: line.to_string(),
    }
}

/// `^\s*<marker>\s+(.+?)\s*$` — the captured group, or `None`.
fn match_marked(line: &str, marker: &str) -> Option<String> {
    let rest = line.trim_start_matches(is_java_space);
    let rest = rest.strip_prefix(marker)?;
    let content = rest.trim_start_matches(is_java_space);
    if content.len() == rest.len() {
        // `\s+` needs at least one whitespace character after the marker.
        return None;
    }
    let trimmed = content.trim_end_matches(is_java_space);
    if trimmed.is_empty() {
        // The lazy group still matches a single whitespace character.
        Some(" ".to_string())
    } else {
        Some(trimmed.to_string())
    }
}

/// `ReleaseNotes.headingText` (`:57-59`) — drops a trailing ATX closing run.
fn heading_text(value: &str) -> String {
    let trimmed = value.trim_end_matches(is_java_space);
    let without_hashes = trimmed.trim_end_matches('#');
    let stripped = if without_hashes.len() < trimmed.len() {
        // `\s+#+\s*$`: the closing run only counts when whitespace precedes it.
        let before = without_hashes.trim_end_matches(is_java_space);
        if before.len() < without_hashes.len() {
            before
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    stripped.trim().to_string()
}

/// `ReleaseNoteRenderer.render` (`:11-24`) as one legacy-coloured line:
/// `##` → AQUA+BOLD, `###` → YELLOW+BOLD, `-` → DARK_GRAY bullet + GRAY text,
/// plain text GRAY, blank line empty.
pub fn render_note(line: &str) -> String {
    let formatted = parse_line(line);
    match formatted.kind {
        LineType::LevelTwoHeading => format!("&b&l{}", formatted.text),
        LineType::LevelThreeHeading => format!("&e&l{}", formatted.text),
        LineType::ListItem => format!("&8  \u{2022} &7{}", formatted.text),
        LineType::Text => format!("&7{}", formatted.text),
        LineType::Blank => String::new(),
    }
}

/// The Mod's HTTP GET (`:92-113`), issued through the host's `wasi:http` client.
///
/// The stable plugin API exposes no fetch helper, so the request is built by
/// hand from the `wasip2` (WASI 0.2) bindings; the host gates it on the
/// `http.outbound` permission (`wasm_host/mod.rs:359, 419`).
#[cfg(target_arch = "wasm32")]
fn fetch_latest(current: &str) -> Result<String, String> {
    use wasip2::http::outgoing_handler;
    use wasip2::http::types::{
        Headers, IncomingBody, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme,
    };
    use wasip2::io::poll::poll;
    use wasip2::io::streams::StreamError;

    let headers = Headers::new();
    headers
        .set("accept", &[ACCEPT_HEADER.as_bytes().to_vec()])
        .map_err(|error| format!("could not set the Accept header: {error:?}"))?;
    headers
        .set(
            "user-agent",
            &[format!("{USER_AGENT_PREFIX}{current}").into_bytes()],
        )
        .map_err(|error| format!("could not set the User-Agent header: {error:?}"))?;

    let request = OutgoingRequest::new(headers);
    request
        .set_method(&Method::Get)
        .map_err(|()| "could not set the request method".to_string())?;
    request
        .set_scheme(Some(&Scheme::Https))
        .map_err(|()| "could not set the request scheme".to_string())?;
    request
        .set_authority(Some(API_AUTHORITY))
        .map_err(|()| "could not set the request authority".to_string())?;
    request
        .set_path_with_query(Some(API_PATH))
        .map_err(|()| "could not set the request path".to_string())?;

    // A `GET` still has to close its (empty) body before it is sent.
    {
        let body = request
            .body()
            .map_err(|()| "could not open the request body".to_string())?;
        let stream = body
            .write()
            .map_err(|()| "could not open the request body stream".to_string())?;
        drop(stream);
        OutgoingBody::finish(body, None)
            .map_err(|error| format!("could not finish the request body: {error:?}"))?;
    }

    let timeout = REQUEST_TIMEOUT_SECONDS * 1_000_000_000;
    let options = RequestOptions::new();
    options
        .set_connect_timeout(Some(timeout))
        .map_err(|()| "could not set the connect timeout".to_string())?;
    options
        .set_first_byte_timeout(Some(timeout))
        .map_err(|()| "could not set the first-byte timeout".to_string())?;
    options
        .set_between_bytes_timeout(Some(timeout))
        .map_err(|()| "could not set the between-bytes timeout".to_string())?;

    let future = outgoing_handler::handle(request, Some(options))
        .map_err(|error| format!("GitHub request rejected: {error:?}"))?;
    let response = loop {
        match future.get() {
            Some(Ok(Ok(response))) => break response,
            Some(Ok(Err(error))) => return Err(format!("GitHub request failed: {error:?}")),
            Some(Err(())) => return Err("the GitHub request was already consumed".to_string()),
            None => {
                let pollable = future.subscribe();
                let _ = poll(&[&pollable]);
            }
        }
    };

    let status = response.status();
    if !(200..300).contains(&status) {
        // `UpdateChecker.java:104-106`.
        return Err(format!("GitHub API returned HTTP {status}"));
    }

    let incoming = response
        .consume()
        .map_err(|()| "could not consume the GitHub response".to_string())?;
    let stream = incoming
        .stream()
        .map_err(|()| "could not open the GitHub response stream".to_string())?;
    let mut bytes = Vec::new();
    loop {
        match stream.blocking_read(READ_CHUNK_BYTES) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(StreamError::Closed) => break,
            Err(error) => return Err(format!("could not read the GitHub response: {error:?}")),
        }
    }
    drop(stream);
    drop(IncomingBody::finish(incoming));

    String::from_utf8(bytes).map_err(|error| format!("the GitHub response is not UTF-8: {error}"))
}

/// Native builds have no WASI HTTP client: the checker reports the same WARN the
/// Mod logs when a request fails, so unit tests and `cargo check` stay offline.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_latest(_current: &str) -> Result<String, String> {
    Err("the WASI HTTP client is only available in the wasm32-wasip2 build".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(value: &str) -> SemanticVersion {
        SemanticVersion::parse(value)
    }

    /// §5.2 parsing: lowercase, one leading `v`, build metadata dropped, the
    /// first `-` splits the pre-release, and a non-numeric segment reads as `0`.
    #[test]
    fn semantic_version_parsing_follows_the_mod() {
        assert_eq!(version("2.5.4").numbers, [2, 5, 4]);
        assert_eq!(version("  v2.5.4  ").numbers, [2, 5, 4]);
        // Locale.ROOT lowercasing handles an upper-case `V`.
        assert_eq!(version("V1.0.0").numbers, [1, 0, 0]);
        // Build metadata never participates in the comparison.
        assert_eq!(version("1.2.3+build.7").numbers, [1, 2, 3]);
        assert_eq!(version("1.2.3+build.7").pre_release, Vec::<String>::new());
        // `^(\d+).*$` takes the leading digits and a failure reads as 0.
        assert_eq!(version("1.2.x").numbers, [1, 2, 0]);
        assert_eq!(version("1.2.3a").numbers, [1, 2, 3]);
        // Java's split drops trailing empty fields.
        assert_eq!(version("1.").numbers, [1]);
        assert_eq!(version("").numbers, [0]);
        // A pre-release survives, and a blank one counts as none.
        assert_eq!(version("1.0.0-alpha.1").pre_release, ["alpha", "1"]);
        assert_eq!(version("1.0.0-").pre_release, Vec::<String>::new());
        assert_eq!(version("1.0.0- ").pre_release, Vec::<String>::new());
    }

    /// §5.2 comparison: missing numeric segments pad with `0`, a release beats
    /// its pre-releases, a shorter pre-release is smaller, numeric segments
    /// compare numerically and lose against alphanumeric ones.
    #[test]
    fn semantic_version_ordering_matches_the_spec() {
        assert!(version("2.5.4") < version("2.5.5"));
        assert!(version("2.5.4") > version("2.5.3"));
        assert_eq!(version("2.5.4"), version("v2.5.4"));
        assert!(version("2.5.4.1") > version("2.5.4"), "a fourth segment wins");
        assert!(version("2.5") < version("2.5.0.1"), "missing segments pad with 0");

        assert!(version("1.0.0") > version("1.0.0-rc.1"), "release beats pre-release");
        assert!(version("1.0-a") < version("1.0-a.1"), "shorter pre-release is smaller");
        assert!(version("1.0-2") < version("1.0-10"), "numeric segments compare as numbers");
        assert!(version("1.0-2") < version("1.0-alpha"), "numeric loses to alphanumeric");
        assert!(version("1.0-Alpha") == version("1.0-alpha"), "case-insensitive");
    }

    /// §5.3 `normalize`: CRLF/CR unified, blank edges dropped, trailing
    /// whitespace stripped, line-leading whitespace kept.
    #[test]
    fn normalize_matches_the_mod() {
        assert!(normalize("").is_empty());
        assert!(normalize("   \n\t\n").is_empty());
        assert_eq!(normalize("a\r\nb\rc"), ["a", "b", "c"]);
        assert_eq!(normalize("\n\n  ## Title  \n- item\n\n"), ["  ## Title", "- item"]);
    }

    /// §5.4 `parseLine` + `ReleaseNoteRenderer`: heading levels, list bullets,
    /// plain text and blanks.
    #[test]
    fn release_notes_render_like_the_mod() {
        assert_eq!(render_note("## Highlights"), "&b&lHighlights");
        assert_eq!(render_note("### Fixes"), "&e&lFixes");
        assert_eq!(render_note("- a fix"), "&8  \u{2022} &7a fix");
        assert_eq!(render_note("plain text"), "&7plain text");
        assert_eq!(render_note("   "), "");
        // Closing ATX markers need whitespace before them to be dropped.
        assert_eq!(render_note("## Title ##"), "&b&lTitle");
        assert_eq!(render_note("## Title##"), "&b&lTitle##");
        // Four hashes are neither a level-two nor a level-three heading.
        assert_eq!(render_note("#### Deep"), "&7#### Deep");
        // A heading with no text keeps the empty heading kind.
        assert_eq!(parse_line("##").kind, LineType::Text);
        assert_eq!(parse_line("-").kind, LineType::Text);
        assert_eq!(parse_line("## ").kind, LineType::LevelTwoHeading);
    }

    /// §5.1 payload parsing: `html_url` and `body` fall back, `tag_name` is
    /// required.
    #[test]
    fn payload_parsing_follows_the_mod() {
        let payload = parse_payload(
            r###"{"tag_name":"v2.5.4","html_url":"https://example.test/2.5.4","body":"## Notes\n- one"}"###,
        )
        .expect("valid payload");
        let release = payload.into_release();
        assert_eq!(release.version, "2.5.4", "the leading v is dropped");
        assert_eq!(release.url, "https://example.test/2.5.4");
        assert_eq!(release.notes, ["## Notes", "- one"]);

        // A `null` (or absent) body means "no release notes".
        let payload = parse_payload(r#"{"tag_name":"2.0.0","body":null}"#).expect("valid payload");
        let release = payload.into_release();
        assert_eq!(release.url, RELEASES_URL, "html_url falls back");
        assert!(release.notes.is_empty());

        // `tag_name` is mandatory, exactly like the Mod's `getAsString()`.
        assert!(parse_payload(r#"{"body":"x"}"#).is_err());
        assert!(parse_payload("not json").is_err());
    }

    /// §5.1 comparison: newer / equal / ahead.
    #[test]
    fn verdict_reports_newer_current_and_ahead() {
        assert_eq!(verdict_of(&version("2.5.4.1"), &version("v2.5.4")), Verdict::Ahead);
        assert_eq!(verdict_of(&version("2.5.4"), &version("v2.5.4")), Verdict::Current);
        assert_eq!(verdict_of(&version("2.5.4"), &version("v2.6.0")), Verdict::Newer);
        assert_eq!(
            verdict_of(&version("2.5.4"), &version("v2.6.0-rc.1")),
            Verdict::Newer,
            "a pre-release of a newer version still counts as newer"
        );
    }

    /// `tag_name.replaceFirst("^[vV]", "")` removes exactly one marker.
    #[test]
    fn tag_prefix_stripping_matches_the_mod() {
        assert_eq!(strip_tag_prefix("v2.5.4"), "2.5.4");
        assert_eq!(strip_tag_prefix("V2.5.4"), "2.5.4");
        assert_eq!(strip_tag_prefix("2.5.4"), "2.5.4");
        assert_eq!(strip_tag_prefix("vv2.5.4"), "v2.5.4");
    }

    /// The request is assembled from the host/path halves, so together they must
    /// keep spelling the documented endpoint (`UpdateChecker.java:31-33`).
    #[test]
    fn api_endpoint_constants_stay_in_sync() {
        assert_eq!(
            format!("https://{API_AUTHORITY}{API_PATH}"),
            "https://api.github.com/repos/Aruvelut-123/TrChat-Mod/releases/latest"
        );
    }

    /// The request knobs mirror the Mod's client (`UpdateChecker.java:41-44,
    /// 98-100`). Only the wasm32 transport consumes them, so this also keeps them
    /// from drifting unnoticed in native builds.
    #[test]
    fn request_constants_match_the_mod() {
        assert_eq!(ACCEPT_HEADER, "application/vnd.github+json");
        assert_eq!(USER_AGENT_PREFIX, "TrChat-Mod/");
        assert_eq!(REQUEST_TIMEOUT_SECONDS, 30);
        assert!(READ_CHUNK_BYTES > 0);
    }

    /// The notification block: three header lines, the changelog heading, the
    /// notes (or the empty-notes line) and the status footer.
    #[test]
    fn notification_lines_follow_the_mod_order() {
        let table = Lang::init("", "en_US");
        let release = ReleaseInfo {
            version: "2.6.0".to_string(),
            url: RELEASES_URL.to_string(),
            notes: vec!["## Notes".to_string(), "- a fix".to_string()],
        };
        let lines = notification_lines(&table, &release, "2.5.4", "");

        assert!(lines[0].contains("Update found"), "header line 1: {}", lines[0]);
        assert!(
            lines[1].contains("2.5.4") && lines[1].contains("2.6.0"),
            "header line 2 carries both versions: {}",
            lines[1]
        );
        assert!(
            lines[2].contains("Download") && lines[2].contains("GitHub Releases"),
            "link line: {}",
            lines[2]
        );
        assert_eq!(lines[3], table.format("Updater-Changelog", "", &[]));
        assert_eq!(lines[4], "&b&lNotes");
        assert_eq!(lines[5], "&8  \u{2022} &7a fix");
        assert_eq!(lines[6], table.format("Status-Footer", "", &[]));
        assert_eq!(lines.len(), 7);

        // No notes at all → the Mod's dedicated line.
        let empty = ReleaseInfo {
            notes: Vec::new(),
            ..release
        };
        let lines = notification_lines(&table, &empty, "2.5.4", "");
        assert_eq!(lines[4], table.format("Updater-Changelog-Empty", "", &[]));
        assert_eq!(lines.len(), 6);
    }

    /// The tracked version comes from `gradle.properties`, not from the crate.
    #[test]
    fn tracked_version_is_the_mod_version() {
        let parsed = SemanticVersion::parse(CURRENT_VERSION);
        assert!(
            parsed.numbers.len() >= 3,
            "TRCHAT_VERSION must look like a version, got {CURRENT_VERSION:?}"
        );
        assert!(
            verdict_of(&parsed, &parsed) == Verdict::Current,
            "the tracked version must compare equal to itself"
        );
    }
}
