//! Cross-server chat over Redis — the port's `RedisBridge` plus the Redis half
//! of `ChatService` (`redis/RedisBridge.java`, `redis/RedisSettings.java`,
//! `protocol/RedisEnvelopeCodec.java`, `ChatService.java:972-1180`).
//!
//! One published payload is one JSON object, `{"data":["<type>", "…"]}`, with
//! every field a string. A Pumpkin server and a Bukkit/NeoForge server share
//! that envelope byte for byte, so both see each other's chat on the same
//! channel.
//!
//! ## Deviations from the Mod
//!
//! * The Mod runs a daemon thread (`"TrChat Redis subscriber"`) that blocks on
//!   `SUBSCRIBE` and hands each payload to `handleRedisMessage`. A
//!   `wasm32-wasip2` component is single threaded, so [`start`] subscribes and
//!   then *polls* the socket from a repeating server task. The socket is
//!   switched to non-blocking mode, so an idle poll costs no tick time at all;
//!   when the target refuses that flag, a 1 ms receive timeout bounds the wait
//!   instead.
//! * The Mod's `socketTimeoutMillis: 0` means "block forever". A blocking read
//!   that never returned would hang the server tick, so a non-positive value
//!   falls back to [`DEFAULT_SOCKET_TIMEOUT`] for the phases that do block
//!   (connect handshake, `PUBLISH`).
//! * The Mod keys its ignore list on the player UUID. Every other ignore check
//!   in this port keys on the lowercased name, so [`ignored`] resolves the
//!   broadcast's sender name (field 7), falling back to the name the sender's
//!   own `UpdateNames` snapshot recorded for the UUID in field 1.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use pumpkin_plugin_api::{
    events::player::{PlayerJoinEvent, PlayerLeaveEvent},
    events::{EventData, EventHandler, EventPriority},
    player::Player,
    scheduler::SchedulerExt,
    text::TextComponent,
    Context, Server,
};

use crate::config::RedisSection;
use crate::playerdata::SessionPlayers;
use crate::resp::{self, RespConnection, Value};

/// `RedisBridge`'s first action: the plugin only talks to Redis when
/// `redis.enabled` is set (`ChatService.reconnectRedis`, `:493`).
pub const MESSAGE_CHANNEL_DEFAULT: &str = "trchat-message";

/// A non-positive `socketTimeoutMillis` (the Mod's "wait forever") would hang
/// the tick that runs the network call, so the port substitutes this bound.
const DEFAULT_SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// `redis.connectTimeoutMillis` when the key is absent (`TrChatConfig.java:368`).
const DEFAULT_CONNECT_TIMEOUT_MILLIS: u64 = 3000;

/// `redis.reconnectDelayMillis` when the key is absent (`TrChatConfig.java:370`).
const DEFAULT_RECONNECT_DELAY_MILLIS: u64 = 3000;

/// The receive timeout used when the target refuses non-blocking sockets: a
/// poll then waits at most this long before handing the tick back.
const POLL_TIMEOUT: Duration = Duration::from_millis(1);

/// `ChatService.REMOTE_PLAYER_TTL` — a `UpdateNames` snapshot older than this is
/// no longer offered to `/msg`.
const REMOTE_PLAYER_TTL: Duration = Duration::from_secs(90);

/// `ChatService.tick` publishes the player list every 200 ticks.
const PLAYER_NAMES_INTERVAL_TICKS: u32 = 200;

/// `TrChatProtocol.NIL_UUID` — the sender of a console broadcast and the UUID of
/// the empty player-list snapshot.
const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// One TrChat message — `protocol/TrChatMessage.java`, the `data` vector of the
/// envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    data: Vec<String>,
}

impl Message {
    /// Builds a message from its fields; field 0 is the action (`TrChatMessage.of`).
    pub fn of(fields: &[&str]) -> Self {
        assert!(
            !fields.is_empty(),
            "TrChat message data must not be empty"
        );
        Self {
            data: fields.iter().map(|field| (*field).to_string()).collect(),
        }
    }

    /// Field 0 — the action name `handleRedisMessage` switches on.
    pub fn action(&self) -> &str {
        self.data.first().map(String::as_str).unwrap_or("")
    }

    /// The raw fields.
    pub fn data(&self) -> &[String] {
        &self.data
    }
}

/// `TrChatProtocol.forwardPrivate` — the private message a remote server
/// delivers, wrapped in `ForwardMessage` so the receiver unwraps it.
pub fn forward_private(
    target: &str,
    sender: &str,
    receiver_component: &str,
    fallback: &str,
    message_component: &str,
) -> Message {
    Message::of(&[
        "ForwardMessage",
        "SendPrivateRaw",
        target,
        sender,
        receiver_component,
        fallback,
        message_component,
    ])
}

/// `TrChatProtocol.emptyPlayerNames` — the nil-UUID snapshot that clears a
/// server's previous player list (`ChatService.java:1122-1129`).
pub fn empty_player_names(server_id: &str) -> Message {
    Message::of(&["UpdateNames", server_id, "", "#", NIL_UUID])
}

/// `RedisEnvelopeCodec.encode` — `{"data":["…"]}` with the Mod's escape set.
pub fn encode(message: &Message) -> String {
    let mut out = String::with_capacity(32 + message.data.len() * 16);
    out.push_str("{\"data\":[");
    for (index, field) in message.data.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        append_string(&mut out, field);
    }
    out.push_str("]}");
    out
}

/// `RedisEnvelopeCodec.appendString`.
fn append_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// `RedisEnvelopeCodec.decode` — the `data` array of a published envelope.
///
/// Like the Mod's parser the object must carry a non-empty `data` field, the
/// members of which are strings; a single scalar `data` value is accepted too
/// (`Parser.readEnvelope`). Unknown keys are ignored.
pub fn decode(payload: &str) -> Result<Message, String> {
    let root: serde_json::Value = serde_json::from_str(payload)
        .map_err(|error| format!("Invalid TrChat envelope: {error}"))?;
    let Some(object) = root.as_object() else {
        return Err("Invalid TrChat envelope: expected a JSON object".to_string());
    };
    let Some(data) = object.get("data") else {
        return Err("Missing or empty data field".to_string());
    };
    let data = match data {
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "Invalid TrChat envelope: data entries must be strings".to_string())
            })
            .collect::<Result<Vec<String>, String>>()?,
        serde_json::Value::String(text) => vec![text.clone()],
        _ => return Err("Missing or empty data field".to_string()),
    };
    if data.is_empty() {
        return Err("Missing or empty data field".to_string());
    }
    Ok(Message { data })
}

/// `TrChatProtocol.parseUuid` — trims, accepts the 32-character undashed form
/// and the canonical 36-character form, and returns `None` otherwise.
///
/// The port keeps UUIDs as the canonical dashed string rather than a `uuid`
/// handle: the wire format is a string, and a string stays testable off-server.
pub fn parse_uuid(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let compact: String = match trimmed.len() {
        36 => {
            let bytes = trimmed.as_bytes();
            if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
                return None;
            }
            trimmed.chars().filter(|character| *character != '-').collect()
        }
        32 => trimmed.to_string(),
        _ => return None,
    };
    if !compact.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    let compact = compact.to_ascii_lowercase();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &compact[0..8],
        &compact[8..12],
        &compact[12..16],
        &compact[16..20],
        &compact[20..32]
    ))
}

/// `ChatService.splitProtocolList` — a `,`-joined field, where an empty field is
/// no entry at all.
fn split_list(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(str::to_string).collect()
    }
}

/// `RedisBridge`'s settings, resolved from `redis:` in `settings.yml`
/// (`RedisSettings.fromConfig`).
struct Settings {
    host: String,
    port: u16,
    username: String,
    password: String,
    database: u16,
    connect_timeout: Duration,
    /// The blocking read timeout, never zero (see [`DEFAULT_SOCKET_TIMEOUT`]).
    socket_timeout: Duration,
    reconnect_delay: Duration,
    channel: String,
}

impl Settings {
    fn from_section(section: &RedisSection) -> Self {
        Self {
            host: if section.host.trim().is_empty() {
                "127.0.0.1".to_string()
            } else {
                section.host.trim().to_string()
            },
            port: if section.port == 0 { 6379 } else { section.port },
            username: section.username.trim().to_string(),
            password: section.password.clone(),
            database: section.database,
            connect_timeout: Duration::from_millis(if section.connect_timeout_millis == 0 {
                DEFAULT_CONNECT_TIMEOUT_MILLIS
            } else {
                u64::from(section.connect_timeout_millis)
            }),
            socket_timeout: if section.socket_timeout_millis == 0 {
                DEFAULT_SOCKET_TIMEOUT
            } else {
                Duration::from_millis(u64::from(section.socket_timeout_millis))
            },
            reconnect_delay: Duration::from_millis(if section.reconnect_delay_millis == 0 {
                DEFAULT_RECONNECT_DELAY_MILLIS
            } else {
                u64::from(section.reconnect_delay_millis)
            }),
            channel: if section.channel.trim().is_empty() {
                MESSAGE_CHANNEL_DEFAULT.to_string()
            } else {
                section.channel.trim().to_string()
            },
        }
    }
}

/// The `host:port` the plugin is configured to talk to, for the status report.
pub fn endpoint() -> String {
    let settings = settings();
    format!("{}:{}", settings.host, settings.port)
}

/// The configured channel name, for the status report.
pub fn channel() -> String {
    settings().channel
}

fn settings() -> Settings {
    let section = crate::config::global_config().read().settings.redis.clone();
    Settings::from_section(&section)
}

fn server_id() -> u32 {
    crate::config::global_config().read().settings.chat.server_id
}

fn enabled() -> bool {
    crate::config::global_config().read().settings.redis.enabled
}

/// The live bridge; `None` while Redis is disabled or was never started.
static BRIDGE: Mutex<Option<Bridge>> = Mutex::new(None);

fn bridge() -> MutexGuard<'static, Option<Bridge>> {
    BRIDGE.lock().unwrap_or_else(|error| error.into_inner())
}

/// A subscription connection.
///
/// The socket reads without blocking when the target allows it, so an idle poll
/// costs nothing; otherwise [`POLL_TIMEOUT`] bounds every read.
struct Subscriber {
    connection: RespConnection,
}

impl Subscriber {
    /// Connects, authenticates, selects and subscribes — the Mod's
    /// `subscriptionLoop` prologue (`RedisBridge.java:82-91`).
    fn open(settings: &Settings) -> io::Result<Self> {
        let mut connection = open_connection(settings)?;
        connection.write_command(&["SUBSCRIBE", &settings.channel])?;
        let acknowledgement = connection.poll()?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "Redis did not answer the SUBSCRIBE in time",
            )
        })?;
        // The Mod requires a list acknowledgement (`RedisBridge.java:86-88`).
        if acknowledgement.as_array().is_none() {
            return Err(io::Error::other(format!(
                "Unexpected SUBSCRIBE response: {acknowledgement}"
            )));
        }
        // Non-blocking from here on: a poll must never hold the server tick.
        if connection.set_nonblocking(true).is_err() {
            connection.set_read_timeout(Some(POLL_TIMEOUT))?;
        }
        Ok(Self { connection })
    }
}

/// Connects and runs the `AUTH`/`SELECT` prologue both connections share
/// (`RespConnection.java:22-40`).
fn open_connection(settings: &Settings) -> io::Result<RespConnection> {
    let mut connection =
        RespConnection::connect(&settings.host, settings.port, settings.connect_timeout)?;
    connection.set_read_timeout(Some(settings.socket_timeout))?;
    connection.set_write_timeout(Some(settings.socket_timeout))?;
    if !settings.password.is_empty() {
        let response = if settings.username.is_empty() {
            connection.command(&["AUTH", &settings.password])?
        } else {
            connection.command(&["AUTH", &settings.username, &settings.password])?
        };
        resp::require_ok(&response, "AUTH")?;
    }
    if settings.database != 0 {
        let database = settings.database.to_string();
        let response = connection.command(&["SELECT", &database])?;
        resp::require_ok(&response, "SELECT")?;
    }
    Ok(connection)
}

/// The Mod's `RedisBridge` state.
struct Bridge {
    settings: Settings,
    /// `RedisBridge.publisher` — created on the first publish, dropped when it
    /// fails so the next publish dials a fresh one.
    publisher: Option<RespConnection>,
    subscriber: Option<Subscriber>,
    /// `RedisBridge.subscribed`.
    subscribed: bool,
    /// Earliest instant the next connect attempt may run
    /// (`settings.reconnectDelayMillis`).
    next_attempt: Instant,
}

impl Bridge {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            publisher: None,
            subscriber: None,
            subscribed: false,
            next_attempt: Instant::now(),
        }
    }

    /// Dials the subscriber unless one is live or the reconnect delay has not
    /// elapsed — the Mod's `subscriptionLoop` plus its `Thread.sleep`.
    fn ensure_subscriber(&mut self) {
        if self.subscriber.is_some() || Instant::now() < self.next_attempt {
            return;
        }
        match Subscriber::open(&self.settings) {
            Ok(subscriber) => {
                self.subscriber = Some(subscriber);
                self.subscribed = true;
                crate::diag::info(format!(
                    "Connected to Redis at {}:{} on channel '{}'",
                    self.settings.host, self.settings.port, self.settings.channel
                ));
            }
            Err(error) => {
                self.subscribed = false;
                self.next_attempt = Instant::now() + self.settings.reconnect_delay;
                crate::diag::warn(format!("Redis subscription lost: {error}"));
            }
        }
    }

    /// Drops the subscription and arms the reconnect delay.
    fn lose_subscriber(&mut self, error: &dyn std::fmt::Display) {
        crate::diag::warn(format!("Redis subscription lost: {error}"));
        self.subscriber = None;
        self.subscribed = false;
        self.next_attempt = Instant::now() + self.settings.reconnect_delay;
    }

    /// Drains every payload published since the last tick.
    fn pump(&mut self) -> Vec<String> {
        self.ensure_subscriber();
        let channel = self.settings.channel.clone();
        let mut payloads = Vec::new();
        let mut failure = None;
        if let Some(subscriber) = self.subscriber.as_mut() {
            loop {
                match subscriber.connection.poll() {
                    Ok(Some(value)) => {
                        if let Some(payload) = message_payload(&value, &channel) {
                            payloads.push(payload);
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
        }
        if let Some(error) = failure {
            self.lose_subscriber(&error);
        }
        payloads
    }

    /// `RedisBridge.publish` — success means Redis reported at least one
    /// subscriber, exactly as the Mod treats a zero count as a failure.
    fn publish(&mut self, message: &Message) -> bool {
        let payload = encode(message);
        if self.publisher.is_none() {
            match open_connection(&self.settings) {
                Ok(publisher) => self.publisher = Some(publisher),
                Err(error) => {
                    crate::diag::warn(format!("Redis publish failed: {error}"));
                    return false;
                }
            }
        }
        let Some(publisher) = self.publisher.as_mut() else {
            return false;
        };
        match publisher.command(&["PUBLISH", &self.settings.channel, &payload]) {
            Ok(response) => response.as_integer().is_some_and(|count| count > 0),
            Err(error) => {
                self.publisher = None;
                crate::diag::warn(format!("Redis publish failed: {error}"));
                false
            }
        }
    }
}

/// `RedisBridge.subscriptionLoop`'s push filter: a `message` push for the
/// configured channel, with a payload (`RedisBridge.java:95-101`).
fn message_payload(value: &Value, channel: &str) -> Option<String> {
    let values = value.as_array()?;
    if values.len() < 3 {
        return None;
    }
    if values[0].as_str() != Some("message") {
        return None;
    }
    if values[1].as_str() != Some(channel) {
        return None;
    }
    values[2].as_str().map(str::to_string)
}

/// A player another server reported through `UpdateNames`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePlayer {
    pub name: String,
    pub display_name: String,
    pub uuid: String,
}

/// One server's `UpdateNames` snapshot (`ChatService.RemoteServerPlayers`).
struct RemoteServerPlayers {
    updated_at: Instant,
    players: Vec<RemotePlayer>,
}

static REMOTE_PLAYERS: OnceLock<Mutex<HashMap<String, RemoteServerPlayers>>> = OnceLock::new();

fn remote_players() -> &'static Mutex<HashMap<String, RemoteServerPlayers>> {
    REMOTE_PLAYERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_remote_players() -> MutexGuard<'static, HashMap<String, RemoteServerPlayers>> {
    remote_players()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// `ChatService.expireRemotePlayers`.
fn expire_remote_players(snapshot: &mut HashMap<String, RemoteServerPlayers>, now: Instant) {
    snapshot.retain(|_, server| now.duration_since(server.updated_at) <= REMOTE_PLAYER_TTL);
}

/// `ChatService.findRemotePlayer` — a live snapshot of another server, matched
/// on the account name or the display name.
pub fn find_remote_player(requested: &str) -> Option<RemotePlayer> {
    let now = Instant::now();
    let mut snapshot = lock_remote_players();
    expire_remote_players(&mut snapshot, now);
    for server in snapshot.values() {
        for player in &server.players {
            if player.name.eq_ignore_ascii_case(requested)
                || player.display_name.eq_ignore_ascii_case(requested)
            {
                return Some(player.clone());
            }
        }
    }
    None
}

/// `ChatService.exactRemoteName` — the account name behind `/msg <name>`.
pub fn exact_remote_name(requested: &str) -> Option<String> {
    find_remote_player(requested).map(|player| player.name)
}

/// The account names of every cross-server remote player, deduplicated and
/// sorted — the remote half of `ChatService.knownPlayerNames`
/// (`ChatService.java:276-287`).
pub fn remote_player_names() -> Vec<String> {
    let now = Instant::now();
    let mut snapshot = lock_remote_players();
    expire_remote_players(&mut snapshot, now);
    let mut names: Vec<String> = snapshot
        .values()
        .flat_map(|server| server.players.iter().map(|player| player.name.clone()))
        .collect();
    names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// The remote player a UUID belongs to, for an ignore check whose sender name
/// arrived blank.
fn find_remote_player_by_uuid(uuid: &str) -> Option<RemotePlayer> {
    let now = Instant::now();
    let mut snapshot = lock_remote_players();
    expire_remote_players(&mut snapshot, now);
    for server in snapshot.values() {
        for player in &server.players {
            if player.uuid.eq_ignore_ascii_case(uuid) {
                return Some(player.clone());
            }
        }
    }
    None
}

/// How many ticks have elapsed since the plugin loaded; drives the `UpdateNames`
/// cadence (`ChatService.tick`).
static TICKS: AtomicU32 = AtomicU32::new(0);

/// Starts cross-server chat, mirroring `ChatService.reconnectRedis` plus the
/// tick hook: nothing is dialled when `redis.enabled` is false, and otherwise a
/// repeating task drives the subscriber, the 200-tick player-list snapshot and
/// the remote-player expiry.
pub fn start(context: &Context) -> Result<(), String> {
    if !enabled() {
        crate::diag::info("[TrChat] Redis cross-server chat disabled (redis.enabled: false).");
        return Ok(());
    }
    *bridge() = Some(Bridge::new(settings()));
    // `ChatService.playerListChanged` runs on join and quit so the other
    // servers learn about a player without waiting for the next snapshot.
    context
        .register_event_handler::<PlayerJoinEvent, PlayerListHandler>(
            PlayerListHandler,
            EventPriority::Normal,
            false,
        )
        .map_err(|error| error.to_string())?;
    context
        .register_event_handler::<PlayerLeaveEvent, PlayerListHandler>(
            PlayerListHandler,
            EventPriority::Normal,
            false,
        )
        .map_err(|error| error.to_string())?;
    context.schedule_repeating_task(0, 1, |server| tick(&server));
    crate::diag::info(format!(
        "[TrChat] Redis cross-server chat started (channel '{}').",
        channel()
    ));
    Ok(())
}

/// The Mod's `ChatService.tick` plus `RedisBridge`'s subscriber loop.
fn tick(server: &Server) {
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if ticks % PLAYER_NAMES_INTERVAL_TICKS == 0 {
        publish_player_names(server);
    }
    expire_remote_players(&mut lock_remote_players(), Instant::now());
    let payloads = {
        let mut guard = bridge();
        match guard.as_mut() {
            Some(bridge) => bridge.pump(),
            None => return,
        }
    };
    for payload in payloads {
        match decode(&payload) {
            Ok(message) => handle(server, &message),
            Err(error) => {
                crate::diag::warn(format!("Ignoring malformed TrChat Redis message: {error}"))
            }
        }
    }
}

/// Publishes `message`, reporting whether Redis accepted it.
///
/// `false` covers both "no bridge" and a failed publish, which is what the
/// callers branch on (`ChatService.java:126`, `:214`, `:603`).
pub fn publish(message: &Message) -> bool {
    let mut guard = bridge();
    match guard.as_mut() {
        Some(bridge) => bridge.publish(message),
        None => false,
    }
}

/// `ChatService.sendPublic` / `sendConsole` — relays one rendered broadcast to
/// the other servers (`ChatService.java:604-614`, `:127-135`).
///
/// `sender_uuid` is the canonical 36-char UUID ([`NIL_UUID`] for the console),
/// `permission` the channel's listen permission (empty = everyone), `ports` the
/// `Ports` gate (empty = every server) and `mentioned` the comma-joined
/// lowercased names the mention alert must reach.
#[allow(clippy::too_many_arguments)]
pub fn publish_broadcast(
    sender_uuid: &str,
    component_json: &str,
    permission: &str,
    double_transfer: bool,
    ports: &[u16],
    fallback: &str,
    sender_name: &str,
    mentioned: &str,
) -> bool {
    let ports = ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(";");
    // `ChatService.java:607` serialises the rendered view with the Adventure
    // GSON shape, so the wire payload must match it for the other servers to
    // keep the hover/click events (see the wire helpers above).
    let component_json = to_adventure_wire_json(component_json);
    publish(&Message::of(&[
        "BroadcastRaw",
        sender_uuid,
        component_json.as_str(),
        permission,
        if double_transfer { "true" } else { "false" },
        &ports,
        fallback,
        sender_name,
        mentioned,
    ]))
}

/// `ChatService.sendPrivate` — relays one private message to the server that
/// hosts the receiver (`TrChatProtocol.forwardPrivate`).
pub fn publish_private(
    target: &str,
    sender: &str,
    receiver_component: &str,
    fallback: &str,
    message_component: &str,
) -> bool {
    let receiver_component = to_adventure_wire_json(receiver_component);
    let message_component = to_adventure_wire_json(message_component);
    publish(&forward_private(
        target,
        sender,
        &receiver_component,
        fallback,
        &message_component,
    ))
}

/// `ChatService.setGlobalMute` — mirrors a global-mute toggle to the peers.
pub fn publish_global_mute(muted: bool) -> bool {
    publish(&Message::of(&[
        "GlobalMute",
        if muted { "on" } else { "off" },
    ]))
}

/// `ChatService.sendPrivate`'s mention alert — a language notice addressed to a
/// player on another server (`ChatService.java:225-231`).
pub fn publish_send_lang(target: &str, key: &str, arguments: &[&str]) -> bool {
    let mut fields = Vec::with_capacity(3 + arguments.len());
    fields.push("SendLang");
    fields.push(target);
    fields.push(key);
    fields.extend_from_slice(arguments);
    publish(&Message::of(&fields))
}

/// Whether a bridge exists (`ChatService.isRedisEnabled`).
pub fn is_enabled() -> bool {
    bridge().is_some()
}

/// Whether the subscription is live (`ChatService.isRedisConnected`).
pub fn is_connected() -> bool {
    bridge().as_ref().is_some_and(|bridge| bridge.subscribed)
}

/// `/trchat redis reconnect` — `ChatService.reconnectRedis`: drop both
/// connections, then dial again on the next tick.
pub fn reconnect() {
    let settings = settings();
    let enabled = enabled();
    *bridge() = if enabled {
        Some(Bridge::new(settings))
    } else {
        None
    };
}

/// `ChatService.publishPlayerNames` — tells the other servers who is online
/// here, so `/msg <name>` can reach them.
pub fn publish_player_names(server: &Server) {
    if !is_enabled() {
        return;
    }
    let id = server_id().to_string();
    let players = server.get_all_players();
    if players.is_empty() {
        publish(&empty_player_names(&id));
        return;
    }
    let names = players
        .iter()
        .map(Player::get_name)
        .collect::<Vec<_>>()
        .join(",");
    let display_names = players
        .iter()
        .map(|player| {
            let display = player.get_display_name().get_text();
            if display.trim().is_empty() {
                // `#` means "no display name, use the account name" on the
                // receiving side (`ChatService.java:1132-1137`).
                "#".to_string()
            } else {
                display.replace(',', "")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let uuids = players
        .iter()
        .map(|player| player.get_id().to_string())
        .collect::<Vec<_>>()
        .join(",");
    publish(&Message::of(&[
        "UpdateNames",
        &id,
        &names,
        &display_names,
        &uuids,
    ]));
}

/// Publishes the player list when someone joins or quits
/// (`ChatService.playerListChanged`).
struct PlayerListHandler;

impl EventHandler<PlayerJoinEvent> for PlayerListHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        publish_player_names(&server);
        event
    }
}

impl EventHandler<PlayerLeaveEvent> for PlayerListHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerLeaveEvent>,
    ) -> EventData<PlayerLeaveEvent> {
        publish_player_names(&server);
        event
    }
}

/// `ChatService.unwrap` — strips the repeated leading `ForwardMessage` marker
/// the private-message envelope is wrapped in.
fn unwrap(message: &Message) -> Vec<String> {
    let mut data = message.data().to_vec();
    while data.len() > 1 && data[0] == "ForwardMessage" {
        data.remove(0);
    }
    data
}

/// `ChatService.handleRedisMessage`.
fn handle(server: &Server, message: &Message) {
    let data = unwrap(message);
    dispatch_fields(server, &data);
}

/// Dispatches one decoded TrChat action, regardless of whether it arrived over
/// Redis or the proxy plugin-message bridge.
pub(crate) fn handle_fields(server: &Server, fields: &[String]) {
    let mut data = fields.to_vec();
    while data.len() > 1 && data[0] == "ForwardMessage" {
        data.remove(0);
    }
    dispatch_fields(server, &data);
}

fn dispatch_fields(server: &Server, data: &[String]) {
    let Some(action) = data.first() else {
        return;
    };
    match action.as_str() {
        "BroadcastRaw" => receive_broadcast(server, data),
        "SendPrivateRaw" => receive_private(server, data),
        "UpdateNames" => receive_player_names(data),
        "UpdateAllNames" => receive_all_player_names(data),
        "GlobalMute" => {
            if let Some(value) = data.get(1) {
                let muted = value.eq_ignore_ascii_case("on");
                SessionPlayers::global()
                    .write()
                    .unwrap_or_else(|error| error.into_inner())
                    .set_global_muted(muted);
            }
        }
        "SendLang" => receive_language_notice(server, data),
        other => crate::diag::debug(format!(
            "Ignoring unsupported TrChat cross-server action '{other}'"
        )),
    }
}

/// The wire shape of the component JSON carried by `BroadcastRaw` /
/// `SendPrivateRaw` is what the Bukkit side serialises with Adventure's
/// `GsonComponentSerializer`: `hoverEvent` / `clickEvent` camelCase keys, hover
/// payload under `contents`, click payload always under `value`.
///
/// The host's `TextComponent::to_json` / `from_json` speak the pumpkin serde
/// shape instead: `hover_event` / `click_event` snake_case keys, hover payload
/// under `value` (with `show_item` / `show_entity` fields flattened), click
/// payload under the variant's own key (`url` / `command` / `path` / `page`).
///
/// The two shapes do not recognise each other: feeding Adventure JSON straight
/// into `from_json`, or publishing `to_json` output to the wire, silently drops
/// hover/click as unknown fields — which is why cross-server messages lost
/// their hover (e.g. the `%server_time%` hover line). These helpers remap the
/// keys at the JSON level so the wire always carries the Adventure shape.
pub(crate) fn to_adventure_wire_json(component_json: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(component_json) else {
        return component_json.to_string();
    };
    convert_wire_json(&mut value, true);
    value.to_string()
}

fn from_adventure_wire_json(component_json: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(component_json) else {
        return component_json.to_string();
    };
    convert_wire_json(&mut value, false);
    value.to_string()
}

/// Recursively maps one component JSON object between the pumpkin serde shape
/// (`to_adventure = true`) and the Adventure wire shape (`false`).
fn convert_wire_json(value: &mut serde_json::Value, to_adventure: bool) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let (hover_from, hover_to) = if to_adventure {
        ("hover_event", "hoverEvent")
    } else {
        ("hoverEvent", "hover_event")
    };
    if let Some(hover) = object.remove(hover_from) {
        let mut hover = hover;
        convert_hover_wire_json(&mut hover, to_adventure);
        object.insert(hover_to.to_string(), hover);
    }

    let (click_from, click_to) = if to_adventure {
        ("click_event", "clickEvent")
    } else {
        ("clickEvent", "click_event")
    };
    if let Some(click) = object.remove(click_from) {
        let mut click = click;
        convert_click_wire_json(&mut click, to_adventure);
        object.insert(click_to.to_string(), click);
    }

    // `extra` (text children) and `with` (translate arguments) recurse.
    for key in ["extra", "with"] {
        if let Some(array) = object.get_mut(key).and_then(|v| v.as_array_mut()) {
            for child in array {
                convert_wire_json(child, to_adventure);
            }
        }
    }
}

fn convert_hover_wire_json(hover: &mut serde_json::Value, to_adventure: bool) {
    let Some(object) = hover.as_object_mut() else {
        return;
    };
    let action = object
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    match action.as_str() {
        "show_text" => {
            if to_adventure {
                if let Some(value) = object.remove("value") {
                    let mut value = value;
                    if let Some(array) = value.as_array_mut() {
                        for child in array {
                            convert_wire_json(child, true);
                        }
                    } else {
                        convert_wire_json(&mut value, true);
                    }
                    object.insert("contents".to_string(), value);
                }
            } else if let Some(contents) = object.remove("contents") {
                let mut contents = contents;
                if let Some(array) = contents.as_array_mut() {
                    for child in array {
                        convert_wire_json(child, false);
                    }
                } else {
                    // Adventure may send a single component where pumpkin serde
                    // expects a list.
                    convert_wire_json(&mut contents, false);
                    contents = serde_json::json!([contents]);
                }
                object.insert("value".to_string(), contents);
            }
        }
        "show_item" | "show_entity" => {
            if to_adventure {
                let mut contents = serde_json::Map::new();
                if let Some(id) = object.remove("id") {
                    contents.insert("id".to_string(), id);
                }
                if let Some(count) = object.remove("count") {
                    contents.insert("count".to_string(), count);
                }
                if action == "show_entity" {
                    // pumpkin serde keeps the entity type under `id` and the
                    // uuid under `uuid`; Adventure wants type/id under
                    // `contents.type`/`contents.id`.
                    if let Some(id) = contents.remove("id") {
                        contents.insert("type".to_string(), id);
                    }
                    if let Some(uuid) = object.remove("uuid") {
                        contents.insert("id".to_string(), uuid);
                    }
                    if let Some(name) = object.remove("name") {
                        // Adventure serialises `name` as a single component,
                        // pumpkin serde as a list.
                        let name = match name.as_array() {
                            Some(array) if array.len() == 1 => array[0].clone(),
                            Some(array) => serde_json::json!({ "extra": array }),
                            _ => name,
                        };
                        contents.insert("name".to_string(), name);
                    }
                }
                object.insert("contents".to_string(), serde_json::Value::Object(contents));
            } else if let Some(contents) = object.remove("contents") {
                if let serde_json::Value::Object(contents) = contents {
                    for (key, mut child) in contents {
                        if action == "show_entity" {
                            match key.as_str() {
                                "type" => {
                                    object.insert("id".to_string(), child);
                                    continue;
                                }
                                "id" => {
                                    object.insert("uuid".to_string(), child);
                                    continue;
                                }
                                "name" => {
                                    if let Some(array) = child.as_array_mut() {
                                        for component in array {
                                            convert_wire_json(component, false);
                                        }
                                    } else {
                                        // Adventure may send a single component
                                        // where pumpkin serde expects a list.
                                        convert_wire_json(&mut child, false);
                                        child = serde_json::json!([child]);
                                    }
                                    object.insert("name".to_string(), child);
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        if let Some(array) = child.as_array_mut() {
                            for component in array {
                                convert_wire_json(component, false);
                            }
                        }
                        object.insert(key, child);
                    }
                }
            }
        }
        _ => {}
    }
}

fn convert_click_wire_json(click: &mut serde_json::Value, to_adventure: bool) {
    let Some(object) = click.as_object_mut() else {
        return;
    };
    let action = object
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    if to_adventure {
        // pumpkin serde keeps the payload under the variant's own key;
        // Adventure always uses `value`.
        let payload_key = match action.as_str() {
            "open_url" => Some("url"),
            "run_command" | "suggest_command" => Some("command"),
            "open_file" => Some("path"),
            "change_page" => Some("page"),
            "copy_to_clipboard" => Some("value"),
            _ => None,
        };
        if let Some(key) = payload_key {
            if let Some(payload) = object.remove(key) {
                object.insert("value".to_string(), payload);
            }
        }
    } else if let Some(payload) = object.remove("value") {
        let key = match action.as_str() {
            "open_url" => "url",
            "run_command" | "suggest_command" => "command",
            "open_file" => "path",
            "change_page" => "page",
            _ => "value",
        };
        if action == "change_page" {
            // pumpkin serde expects a u32, Adventure may send a string.
            let page = payload
                .as_str()
                .and_then(|text| text.parse::<u32>().ok())
                .or_else(|| payload.as_u64().map(|page| page as u32));
            object.insert(
                "page".to_string(),
                page.map(|page| serde_json::json!(page)).unwrap_or(payload),
            );
        } else {
            object.insert(key.to_string(), payload);
        }
    }
}

/// `ComponentJson.deserialize` — the parsed component, or the legacy fallback
/// when the JSON is blank or unusable.
///
/// A `TextComponent` is a WIT resource handle, so each receiver needs its own
/// parse; the port builds it per delivery, exactly as the renderer does. The
/// wire carries Adventure JSON (see the helpers above), so it is mapped back to
/// the pumpkin serde shape before the host parses it.
fn deserialize_component(json: &str, fallback: &str) -> TextComponent {
    if !json.trim().is_empty() {
        let pumpkin_json = from_adventure_wire_json(json);
        if let Ok(component) = TextComponent::from_json(&pumpkin_json) {
            return component;
        }
    }
    TextComponent::from_legacy_string_with_code(fallback, '&')
}

/// `ChatService.receiveBroadcast` — deliver a remote broadcast to every local
/// player who may see it.
fn receive_broadcast(server: &Server, data: &[String]) {
    if data.len() < 3 {
        return;
    }
    // Field 5 is the `Ports` gate: a non-blank list of the server ids this
    // broadcast is addressed to (`ChatService.java:1000-1006`).
    if !port_accepted(data.get(5).map(String::as_str), server_id()) {
        return;
    }
    let fallback = data.get(6).map(String::as_str).unwrap_or("");
    let component_json = data.get(2).map(String::as_str).unwrap_or("");
    let permission = data.get(3).map(String::as_str).unwrap_or("").to_string();
    let sender_uuid = parse_uuid(data.get(1).map(String::as_str).unwrap_or(""));
    let sender_name = data.get(7).map(String::as_str).unwrap_or("").to_string();
    let mentioned: HashSet<String> = match data.get(8) {
        Some(value) if !value.trim().is_empty() => value.split(',').map(str::to_string).collect(),
        _ => HashSet::new(),
    };

    let mut receivers: Vec<Player> = Vec::new();
    for player in server.get_all_players() {
        let name = player.get_name();
        if ignored(&name, sender_uuid.as_deref(), &sender_name) {
            continue;
        }
        if !permission.trim().is_empty()
            && !player.has_permission(&crate::perms::node(&permission))
        {
            continue;
        }
        let _ = player.send_system_message(deserialize_component(component_json, fallback), false);
        receivers.push(player);
    }
    if !mentioned.is_empty() && !sender_name.trim().is_empty() {
        notify_mentioned(&receivers, &mentioned, &sender_name);
    }
    crate::diag::info(deserialize_component(component_json, fallback).get_text());
}

/// The `Ports` field gate of [`receive_broadcast`].
fn port_accepted(ports: Option<&str>, server_id: u32) -> bool {
    match ports {
        Some(value) if !value.trim().is_empty() => {
            let id = server_id.to_string();
            value.split(';').any(|port| port == id)
        }
        _ => true,
    }
}

/// `ChatService.notifyMentioned` — the mention alert goes only to receivers who
/// actually saw the broadcast.
fn notify_mentioned(receivers: &[Player], mentioned: &HashSet<String>, sender_name: &str) {
    for receiver in receivers {
        let name = receiver.get_name();
        if mentioned
            .iter()
            .any(|target| name.eq_ignore_ascii_case(target))
        {
            crate::functions::notify_mentioned(receiver, sender_name, &receiver.get_locale());
        }
    }
}

/// `ChatService.receivePrivate` — hand a remote private message to its local
/// target and echo it to the spies.
///
/// The target may already be gone (the sender resolved them from a snapshot
/// this server has not refreshed yet); the spy echo still runs, matching the
/// Mod's null-target branch (`ChatService.java:1040-1051`).
fn receive_private(server: &Server, data: &[String]) {
    if data.len() < 4 {
        return;
    }
    let target_name = data[1].clone();
    let from = data[2].clone();
    let fallback = data.get(4).map(String::as_str).unwrap_or("");
    let component_json = data[3].clone();
    let target = server.get_player_by_name(&target_name);
    let ignored = match (&target, find_remote_player(&from)) {
        (Some(target), Some(sender)) => {
            crate::private_msg::ignores(&target.get_name(), &sender.name)
        }
        _ => false,
    };
    let delivered = deserialize_component(&component_json, fallback).get_text();
    if let Some(target) = &target {
        if !ignored {
            let _ = target.send_system_message(deserialize_component(&component_json, fallback), false);
        }
    }
    if from.trim().is_empty() {
        return;
    }
    if target.is_some() && !ignored {
        crate::private_msg::remember_correspondent(&target_name, &from);
    }
    // Field 5 carries the spy view; its fallback is the delivered text
    // (`ChatService.java:1047-1049`).
    let spy = match data.get(5) {
        Some(value) if !value.trim().is_empty() => {
            deserialize_component(value, &delivered).get_text()
        }
        _ => delivered,
    };
    crate::private_msg::notify_spies_by_name(server, &from, &target_name, &spy);
}

/// `ChatService.receivePlayerNames` — refresh one server's snapshot.
fn receive_player_names(data: &[String]) {
    if data.len() < 5 {
        return;
    }
    let reported: &str = &data[1];
    if reported == server_id().to_string() {
        return;
    }
    let names = split_list(&data[2]);
    let display_names = split_list(&data[3]);
    let uuids = split_list(&data[4]);
    let players = parse_remote_players(&names, &display_names, &uuids);
    lock_remote_players().insert(
        reported.to_string(),
        RemoteServerPlayers {
            updated_at: Instant::now(),
            players,
        },
    );
}

/// `ChatService.receiveAllPlayerNames` — the aggregate snapshot sent by a
/// Bungee/Velocity proxy after a backend publishes `UpdateNames`.
fn receive_all_player_names(data: &[String]) {
    if data.len() < 4 {
        return;
    }
    let names = split_list(&data[1]);
    let display_names = split_list(&data[2]);
    let uuids = split_list(&data[3]);
    let players = parse_remote_players(&names, &display_names, &uuids);
    let mut remote = lock_remote_players();
    remote.clear();
    remote.insert(
        "proxy".to_string(),
        RemoteServerPlayers {
            updated_at: Instant::now(),
            players,
        },
    );
}

fn parse_remote_players(
    names: &[String],
    display_names: &[String],
    uuids: &[String],
) -> Vec<RemotePlayer> {
    let mut players = Vec::new();
    for (index, name) in names.iter().enumerate() {
        if name.trim().is_empty() {
            continue;
        }
        let display_name = match display_names.get(index) {
            Some(display) if display != "#" => display.clone(),
            _ => name.clone(),
        };
        let Some(uuid) = parse_uuid(uuids.get(index).map(String::as_str).unwrap_or("")) else {
            continue;
        };
        players.push(RemotePlayer {
            name: name.clone(),
            display_name,
            uuid,
        });
    }
    players
}

/// `ChatService.receiveLanguageNotice` — either the mention alert or a literal
/// message assembled from the key and its arguments.
fn receive_language_notice(server: &Server, data: &[String]) {
    if data.len() < 4 {
        return;
    }
    let Some(target) = server.get_player_by_name(&data[1]) else {
        return;
    };
    let key = &data[2];
    let sender_name = if data.len() > 3 {
        data[3..].join(", ")
    } else {
        String::new()
    };
    if key == "Function-Mention-Notify" {
        crate::functions::notify_mentioned(&target, &sender_name, &target.get_locale());
        return;
    }
    let text = if sender_name.is_empty() {
        key.clone()
    } else {
        format!("{key}: {sender_name}")
    };
    let _ = target.send_system_message(TextComponent::text(&text), false);
}

/// `ModerationService.hasIgnored`, name-keyed (see the module docs).
fn ignored(player: &str, sender_uuid: Option<&str>, sender_name: &str) -> bool {
    let name = if !sender_name.trim().is_empty() {
        sender_name.trim().to_string()
    } else {
        match sender_uuid.and_then(find_remote_player_by_uuid) {
            Some(player) => player.name,
            None => return false,
        }
    };
    let session = SessionPlayers::global();
    let session = session.read().unwrap_or_else(|error| error.into_inner());
    session.ignores(player, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts the `to_adventure_wire_json` → `from_adventure_wire_json` round
    /// trip preserves the pumpkin serde value exactly.
    fn assert_wire_round_trip(pumpkin_json: &str) {
        let adventure = to_adventure_wire_json(pumpkin_json);
        let back = from_adventure_wire_json(&adventure);
        let pumpkin: serde_json::Value = serde_json::from_str(pumpkin_json).unwrap();
        let round: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(round, pumpkin, "round trip changed the value");
    }

    #[test]
    fn wire_json_maps_show_text_hover_and_click() {
        let pumpkin = r##"{"text":"hi","hover_event":{"action":"show_text","value":[{"text":"t","color":"#ff0000"}]},"click_event":{"action":"run_command","command":"/tp @s"}}"##;
        let adventure = to_adventure_wire_json(pumpkin);
        let expected: serde_json::Value = serde_json::from_str(
            r##"{"text":"hi","hoverEvent":{"action":"show_text","contents":[{"text":"t","color":"#ff0000"}]},"clickEvent":{"action":"run_command","value":"/tp @s"}}"##,
        )
        .unwrap();
        let actual: serde_json::Value = serde_json::from_str(&adventure).unwrap();
        assert_eq!(actual, expected);
        assert_wire_round_trip(pumpkin);
    }

    #[test]
    fn wire_json_maps_click_variants() {
        let pumpkin = r#"{"text":"a","click_event":{"action":"open_url","url":"https://example.com"}}"#;
        let adventure: serde_json::Value =
            serde_json::from_str(&to_adventure_wire_json(pumpkin)).unwrap();
        assert_eq!(adventure["clickEvent"]["value"], "https://example.com");
        assert_wire_round_trip(pumpkin);

        let pumpkin =
            r#"{"text":"b","click_event":{"action":"change_page","page":3}}"#;
        let adventure: serde_json::Value =
            serde_json::from_str(&to_adventure_wire_json(pumpkin)).unwrap();
        assert_eq!(adventure["clickEvent"]["value"], 3);
        assert_wire_round_trip(pumpkin);

        let pumpkin = r#"{"text":"c","click_event":{"action":"copy_to_clipboard","value":"x"}}"#;
        assert_wire_round_trip(pumpkin);
    }

    #[test]
    fn wire_json_maps_show_item() {
        let pumpkin =
            r#"{"text":"i","hover_event":{"action":"show_item","id":"minecraft:diamond","count":2}}"#;
        let adventure: serde_json::Value =
            serde_json::from_str(&to_adventure_wire_json(pumpkin)).unwrap();
        assert_eq!(adventure["hoverEvent"]["contents"]["id"], "minecraft:diamond");
        assert_eq!(adventure["hoverEvent"]["contents"]["count"], 2);
        assert!(adventure["hoverEvent"].get("id").is_none());
        assert_wire_round_trip(pumpkin);
    }

    #[test]
    fn wire_json_maps_show_entity() {
        let pumpkin = r#"{"text":"e","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":"00000000-0000-0000-0000-000000000001","name":[{"text":"Pig"}]}}"#;
        let adventure: serde_json::Value =
            serde_json::from_str(&to_adventure_wire_json(pumpkin)).unwrap();
        let contents = &adventure["hoverEvent"]["contents"];
        assert_eq!(contents["type"], "minecraft:pig");
        assert_eq!(contents["id"], "00000000-0000-0000-0000-000000000001");
        assert_eq!(contents["name"], serde_json::json!({"text":"Pig"}));
        assert_wire_round_trip(pumpkin);
    }

    #[test]
    fn wire_json_recurse_into_extra() {
        let pumpkin = r#"{"text":"a","extra":[{"text":"b","hover_event":{"action":"show_text","value":[{"text":"t"}]}}]}"#;
        let adventure: serde_json::Value =
            serde_json::from_str(&to_adventure_wire_json(pumpkin)).unwrap();
        assert!(adventure["extra"][0].get("hoverEvent").is_some());
        assert!(adventure["extra"][0].get("hover_event").is_none());
        assert_wire_round_trip(pumpkin);
    }

    #[test]
    fn wire_json_show_text_single_component() {
        // Adventure may put a single component under `contents`; the round trip
        // must still yield the list shape pumpkin serde expects.
        let adventure = r##"{"text":"hi","hoverEvent":{"action":"show_text","contents":{"text":"t","color":"#ff0000"}}}"##;
        let back = from_adventure_wire_json(adventure);
        let pumpkin: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(pumpkin["hover_event"]["value"], serde_json::json!([{"text":"t","color":"#ff0000"}]));
        // and re-serialising keeps the list shape Adventure expects.
        let again = to_adventure_wire_json(&back);
        let adventure_val: serde_json::Value = serde_json::from_str(&again).unwrap();
        assert_eq!(adventure_val["hoverEvent"]["contents"], serde_json::json!([{"text":"t","color":"#ff0000"}]));
    }

    #[test]
    fn wire_json_passes_through_unparseable_input() {
        assert_eq!(to_adventure_wire_json("not json"), "not json");
        assert_eq!(from_adventure_wire_json("not json"), "not json");
        assert_eq!(from_adventure_wire_json("\"plain\""), "\"plain\"");
    }

    #[test]
    fn envelope_matches_the_mod_encoding() {
        let message = Message::of(&["GlobalMute", "on"]);
        assert_eq!(encode(&message), "{\"data\":[\"GlobalMute\",\"on\"]}");
    }

    #[test]
    fn envelope_escapes_like_the_mod() {
        let message = Message::of(&["a\"b\\c\nd\te\u{1}", "喵"]);
        assert_eq!(
            encode(&message),
            "{\"data\":[\"a\\\"b\\\\c\\nd\\te\\u0001\",\"喵\"]}"
        );
        assert_eq!(decode(&encode(&message)).unwrap(), message);
    }

    #[test]
    fn private_forward_envelope() {
        let message = forward_private("Bob", "Alice", "{\"text\":\"hi\"}", "hi", "{}");
        assert_eq!(
            encode(&message),
            "{\"data\":[\"ForwardMessage\",\"SendPrivateRaw\",\"Bob\",\"Alice\",\
             \"{\\\"text\\\":\\\"hi\\\"}\",\"hi\",\"{}\"]}"
        );
        // `handleRedisMessage` unwraps the marker before switching.
        assert_eq!(unwrap(&message)[0], "SendPrivateRaw");
    }

    #[test]
    fn empty_player_names_clears_the_snapshot() {
        let message = empty_player_names("25565");
        assert_eq!(
            encode(&message),
            "{\"data\":[\"UpdateNames\",\"25565\",\"\",\"#\",\
             \"00000000-0000-0000-0000-000000000000\"]}"
        );
    }

    #[test]
    fn decode_accepts_a_scalar_data_field() {
        assert_eq!(
            decode("{\"data\":\"GlobalMute\"}").unwrap(),
            Message::of(&["GlobalMute"])
        );
    }

    #[test]
    fn decode_skips_unknown_keys() {
        assert_eq!(
            decode("{\"other\":[1,2],\"data\":[\"GlobalMute\",\"off\"]}").unwrap(),
            Message::of(&["GlobalMute", "off"])
        );
    }

    #[test]
    fn decode_rejects_malformed_envelopes() {
        assert!(decode("").is_err());
        assert!(decode("[]").is_err());
        assert!(decode("{}").is_err());
        assert!(decode("{\"data\":[]}").is_err());
        assert!(decode("{\"data\":[1]}").is_err());
    }

    #[test]
    fn a_forward_message_survives_a_round_trip() {
        let payload = encode(&forward_private("Bob", "Alice", "{}", "hi", "{}"));
        let decoded = decode(&payload).unwrap();
        assert_eq!(decoded, forward_private("Bob", "Alice", "{}", "hi", "{}"));
        // `unwrap` peels every repeated `ForwardMessage` prefix, leaving the
        // actual action at the front (`ChatService.unwrap`).
        assert_eq!(
            unwrap(&decoded),
            decoded.data()[1..].to_vec(),
            "the ForwardMessage wrapper is peeled off"
        );
    }

    #[test]
    fn parse_uuid_accepts_both_forms() {
        let canonical = "00112233-4455-6677-8899-aabbccddeeff";
        assert_eq!(parse_uuid(canonical).as_deref(), Some(canonical));
        assert_eq!(
            parse_uuid("00112233445566778899AABBCCDDEEFF").as_deref(),
            Some(canonical)
        );
        assert_eq!(parse_uuid("  00112233-4455-6677-8899-AABBCCDDEEFF ").as_deref(), Some(canonical));
        assert_eq!(parse_uuid(""), None);
        assert_eq!(parse_uuid("not-a-uuid"), None);
        assert_eq!(parse_uuid("00112233-4455-6677-8899-aabbccddeef"), None);
        assert_eq!(parse_uuid("00112233445566778899aabbccddeeg"), None);
    }

    #[test]
    fn split_list_treats_an_empty_field_as_no_entries() {
        assert!(split_list("").is_empty());
        assert_eq!(split_list("Alice,Bob"), vec!["Alice", "Bob"]);
        // A single blank entry is not the same as an empty field.
        assert_eq!(split_list(","), vec!["", ""]);
    }

    #[test]
    fn ports_gate_addresses_this_server() {
        assert!(port_accepted(None, 25565));
        assert!(port_accepted(Some(""), 25565));
        assert!(port_accepted(Some("25565"), 25565));
        assert!(port_accepted(Some("25564;25565"), 25565));
        assert!(!port_accepted(Some("25564"), 25565));
        assert!(!port_accepted(Some("255650"), 25565));
    }

    #[test]
    fn remote_players_expire_after_the_ttl() {
        let mut snapshot = HashMap::new();
        snapshot.insert(
            "25564".to_string(),
            RemoteServerPlayers {
                updated_at: Instant::now(),
                players: vec![RemotePlayer {
                    name: "Bob".to_string(),
                    display_name: "Bob".to_string(),
                    uuid: NIL_UUID.to_string(),
                }],
            },
        );
        expire_remote_players(&mut snapshot, Instant::now());
        assert_eq!(snapshot.len(), 1);
        expire_remote_players(
            &mut snapshot,
            Instant::now() + REMOTE_PLAYER_TTL + Duration::from_secs(1),
        );
        assert!(snapshot.is_empty());
    }

    #[test]
    fn settings_fall_back_to_the_mod_defaults() {
        let settings = Settings::from_section(&RedisSection::default());
        assert_eq!(settings.host, "127.0.0.1");
        assert_eq!(settings.port, 6379);
        assert_eq!(settings.channel, MESSAGE_CHANNEL_DEFAULT);
        assert_eq!(settings.socket_timeout, DEFAULT_SOCKET_TIMEOUT);
        assert_eq!(settings.connect_timeout, Duration::from_millis(3000));
        assert_eq!(settings.reconnect_delay, Duration::from_millis(3000));
    }

    #[test]
    fn settings_read_the_configured_values() {
        let settings = Settings::from_section(&RedisSection {
            enabled: true,
            host: " redis.internal ".to_string(),
            port: 6380,
            username: " user ".to_string(),
            password: "secret".to_string(),
            database: 3,
            connect_timeout_millis: 1500,
            socket_timeout_millis: 2000,
            reconnect_delay_millis: 500,
            channel: "trchat-test".to_string(),
        });
        assert_eq!(settings.host, "redis.internal");
        assert_eq!(settings.port, 6380);
        assert_eq!(settings.username, "user");
        assert_eq!(settings.database, 3);
        assert_eq!(settings.connect_timeout, Duration::from_millis(1500));
        assert_eq!(settings.socket_timeout, Duration::from_millis(2000));
        assert_eq!(settings.reconnect_delay, Duration::from_millis(500));
        assert_eq!(settings.channel, "trchat-test");
    }
}
