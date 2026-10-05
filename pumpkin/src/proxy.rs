//! Cross-server chat over the Bukkit/Velocity plugin-message bridge.
//!
//! The Java Mod and the Bukkit plugins carry the same `TrChatMessage` fields in
//! a Base64-encoded JSON array.  Plugin messages are limited to 30,000
//! characters by the upstream bridge, so this module uses the upstream
//! `uid/index/total/data` envelope and reassembles chunks before dispatching the
//! resulting action to the shared Redis/proxy receiver.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use pumpkin_plugin_api::{
    events::{
        player::{PlayerCustomPayloadEvent, PlayerJoinEvent, PlayerLeaveEvent},
        EventData, EventHandler, EventPriority,
    },
    scheduler::SchedulerExt,
    Context, Server,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::global_config;

/// The protocol's plugin-message channel for BungeeCord mode.
const BUNGEE_CHANNEL: &str = "trchat:main";
/// The backend-to-proxy channel in Velocity mode.
const VELOCITY_OUTGOING_CHANNEL: &str = "trchat:proxy";
/// The proxy-to-backend channel in Velocity mode.
const VELOCITY_INCOMING_CHANNEL: &str = "trchat:server";
/// The upstream payload chunk limit (`ProxyBridge.MAX_MESSAGE_LENGTH`).
const MAX_CHUNK_LENGTH: usize = 30_000;
/// A malformed sender must not be able to retain unbounded chunk state.
const MAX_CHUNKS: usize = 128;
const MAX_PENDING_MESSAGES: usize = 128;
const MAX_BUFFERED_CHARACTERS: usize = 8 * 1024 * 1024;
const MAX_PACKET_BYTES: usize = 32_767;
const MAX_COMPLETED_MESSAGES: usize = 1_024;
/// Matches the Java reader's ten-second partial-message TTL.
const MESSAGE_TTL: Duration = Duration::from_secs(10);
/// The UUID envelope is a canonical 36-character UUID string on the Java side.
const UID_LENGTH: usize = 36;

static READER: OnceLock<Mutex<Reader>> = OnceLock::new();
static TICKS: AtomicU32 = AtomicU32::new(0);
static UID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// The configured proxy dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyMode {
    Bungee,
    Velocity,
}

impl ProxyMode {
    fn from_config(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("bungee") {
            Self::Bungee
        } else {
            // The Java Mod's default is Velocity; unknown values use that same
            // safe default instead of disabling a configured bridge.
            Self::Velocity
        }
    }

    const fn outgoing_channel(self) -> &'static str {
        match self {
            Self::Bungee => BUNGEE_CHANNEL,
            Self::Velocity => VELOCITY_OUTGOING_CHANNEL,
        }
    }

    const fn incoming_channel(self) -> &'static str {
        match self {
            Self::Bungee => BUNGEE_CHANNEL,
            Self::Velocity => VELOCITY_INCOMING_CHANNEL,
        }
    }
}

/// One wire packet.  `data` is an ASCII Base64 chunk, not the decoded action.
#[derive(Debug, Deserialize, Serialize)]
struct WirePacket {
    uid: String,
    index: usize,
    total: usize,
    data: String,
}

#[derive(Debug)]
struct PendingMessage {
    total: usize,
    chunks: BTreeMap<usize, String>,
    created_at: Instant,
    buffered_characters: usize,
}

#[derive(Debug, Default)]
struct Reader {
    pending: HashMap<String, PendingMessage>,
    completed: HashMap<String, Instant>,
    buffered_characters: usize,
}

impl Reader {
    fn accept(&mut self, bytes: &[u8], now: Instant) -> Option<Vec<String>> {
        self.expire(now);
        if bytes.is_empty() || bytes.len() > MAX_PACKET_BYTES {
            return None;
        }
        let packet = serde_json::from_slice::<WirePacket>(bytes).ok()?;
        if !valid_uid(&packet.uid)
            || packet.total == 0
            || packet.total > MAX_CHUNKS
            || packet.index == 0
            || packet.index > packet.total
            || packet.data.len() > MAX_CHUNK_LENGTH
        {
            return None;
        }
        if self.completed.contains_key(&packet.uid) {
            // A duplicate of an already delivered message is harmless.
            return None;
        }

        if !self.pending.contains_key(&packet.uid) {
            while self.pending.len() >= MAX_PENDING_MESSAGES {
                self.evict_oldest();
            }
            self.pending.insert(
                packet.uid.clone(),
                PendingMessage {
                    total: packet.total,
                    chunks: BTreeMap::new(),
                    created_at: now,
                    buffered_characters: 0,
                },
            );
        }

        let Some(pending) = self.pending.get_mut(&packet.uid) else {
            return None;
        };
        if pending.total != packet.total {
            return None;
        }
        // Replayed chunks do not increase the memory accounting and are ignored.
        if pending.chunks.contains_key(&packet.index) {
            return None;
        }
        pending.buffered_characters += packet.data.len();
        self.buffered_characters += packet.data.len();
        pending.chunks.insert(packet.index, packet.data);

        while self.buffered_characters > MAX_BUFFERED_CHARACTERS {
            self.evict_oldest();
        }
        let Some(pending) = self.pending.get(&packet.uid) else {
            return None;
        };
        if pending.chunks.len() != pending.total {
            return None;
        }

        let pending = self.pending.remove(&packet.uid)?;
        self.buffered_characters = self
            .buffered_characters
            .saturating_sub(pending.buffered_characters);
        // Mark the UID completed before decoding so a malformed complete
        // envelope cannot be retried indefinitely by a peer.
        self.completed.insert(packet.uid, now);
        while self.completed.len() > MAX_COMPLETED_MESSAGES {
            let Some(uid) = self
                .completed
                .iter()
                .min_by_key(|(_, completed_at)| *completed_at)
                .map(|(uid, _)| uid.clone())
            else {
                break;
            };
            self.completed.remove(&uid);
        }

        let mut encoded = String::with_capacity(pending.buffered_characters);
        for index in 1..=pending.total {
            encoded.push_str(pending.chunks.get(&index)?);
        }
        let decoded = STANDARD.decode(encoded).ok()?;
        let fields = serde_json::from_slice::<Vec<String>>(&decoded).ok()?;
        (!fields.is_empty()).then_some(fields)
    }

    fn expire(&mut self, now: Instant) {
        self.pending
            .retain(|_, value| now.saturating_duration_since(value.created_at) < MESSAGE_TTL);
        self.buffered_characters = self
            .pending
            .values()
            .map(|value| value.buffered_characters)
            .sum();
        self.completed
            .retain(|_, completed_at| now.saturating_duration_since(*completed_at) < MESSAGE_TTL);
    }

    fn evict_oldest(&mut self) {
        let Some(uid) = self
            .pending
            .iter()
            .min_by_key(|(_, value)| value.created_at)
            .map(|(uid, _)| uid.clone())
        else {
            return;
        };
        if let Some(removed) = self.pending.remove(&uid) {
            self.buffered_characters = self
                .buffered_characters
                .saturating_sub(removed.buffered_characters);
        }
    }
}

fn reader() -> &'static Mutex<Reader> {
    READER.get_or_init(|| Mutex::new(Reader::default()))
}

fn valid_uid(uid: &str) -> bool {
    if uid.len() != UID_LENGTH {
        return false;
    }
    uid.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn next_uid() -> String {
    let counter = UID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let high = (nanos as u64) ^ counter.rotate_left(17);
    let low = ((nanos >> 64) as u64) ^ counter.rotate_right(11);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        high >> 32,
        (high >> 16) & 0xffff,
        high & 0xffff,
        (low >> 48) & 0xffff,
        low & 0xffff_ffff_ffff,
    )
}

fn encode_fields(fields: &[String]) -> Vec<Vec<u8>> {
    if fields.is_empty() {
        return Vec::new();
    }
    let Ok(json) = serde_json::to_vec(fields) else {
        return Vec::new();
    };
    let encoded = STANDARD.encode(json);
    let total = encoded.len().div_ceil(MAX_CHUNK_LENGTH);
    if total == 0 || total > MAX_CHUNKS {
        return Vec::new();
    }
    let uid = next_uid();
    encoded
        .as_bytes()
        .chunks(MAX_CHUNK_LENGTH)
        .enumerate()
        .filter_map(|(offset, chunk)| {
            serde_json::to_vec(&WirePacket {
                uid: uid.clone(),
                index: offset + 1,
                total,
                data: String::from_utf8(chunk.to_vec()).ok()?,
            })
            .ok()
        })
        .collect()
}

fn configured_mode() -> ProxyMode {
    let config = global_config().read();
    ProxyMode::from_config(&config.settings.proxy.mode)
}

/// Whether plugin-message transport is enabled in `settings.yml`.
pub fn is_enabled() -> bool {
    global_config().read().settings.proxy.enabled
}

/// Whether this backend currently has a Java connection that can carry a
/// plugin message to the configured proxy.
pub fn is_ready(server: &Server) -> bool {
    is_enabled()
        && server
            .get_all_players()
            .iter()
            .any(|player| player.as_java().is_some())
}

/// Sends one or more framed payloads through the first online Java player.
///
/// A plugin message is a packet on a player connection, so a backend with no
/// online Java player has no carrier and reports failure to the caller.  This
/// is intentional: the chat path then uses its normal local/unavailable branch.
fn publish_fields(server: &Server, fields: &[String]) -> bool {
    if !is_enabled() {
        return false;
    }
    let packets = encode_fields(fields);
    if packets.is_empty() {
        return false;
    }
    let mode = configured_mode();
    let Some(java) = server
        .get_all_players()
        .into_iter()
        .find_map(|player| player.as_java())
    else {
        return false;
    };
    for packet in packets {
        java.send_custom_payload(mode.outgoing_channel(), &packet);
    }
    true
}

/// Publishes a rendered public broadcast to the proxy.
#[allow(clippy::too_many_arguments)]
pub fn publish_broadcast(
    server: &Server,
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
    let component_json = crate::redis::to_adventure_wire_json(component_json);
    let fields = vec![
        "BroadcastRaw".to_string(),
        sender_uuid.to_string(),
        component_json,
        permission.to_string(),
        if double_transfer { "true" } else { "false" }.to_string(),
        ports,
        fallback.to_string(),
        sender_name.to_string(),
        mentioned.to_string(),
    ];
    publish_fields(server, &fields)
}

/// Publishes a private-message relay to the proxy.
pub fn publish_private(
    server: &Server,
    target: &str,
    sender: &str,
    receiver_component: &str,
    fallback: &str,
    message_component: &str,
) -> bool {
    let receiver_component = crate::redis::to_adventure_wire_json(receiver_component);
    let message_component = crate::redis::to_adventure_wire_json(message_component);
    let fields = vec![
        "ForwardMessage".to_string(),
        "SendPrivateRaw".to_string(),
        target.to_string(),
        sender.to_string(),
        receiver_component,
        fallback.to_string(),
        message_component,
    ];
    publish_fields(server, &fields)
}

/// Publishes a global mute transition to the proxy.
pub fn publish_global_mute(server: &Server, muted: bool) -> bool {
    // Bungee/Velocity only forwards a backend message wrapped in
    // `ForwardMessage`; direct `GlobalMute` is a Redis-side convenience.
    let fields = vec![
        "ForwardMessage".to_string(),
        "GlobalMute".to_string(),
        if muted { "on" } else { "off" }.to_string(),
    ];
    publish_fields(server, &fields)
}

/// Publishes a language/mention notice to the proxy.
pub fn publish_send_lang(server: &Server, target: &str, key: &str, arguments: &[&str]) -> bool {
    let mut fields = Vec::with_capacity(4 + arguments.len());
    fields.push("ForwardMessage".to_string());
    fields.push("SendLang".to_string());
    fields.push(target.to_string());
    fields.push(key.to_string());
    fields.extend(arguments.iter().map(|argument| (*argument).to_string()));
    publish_fields(server, &fields)
}

/// Publishes the local Java player snapshot in the proxy's aggregate format.
pub fn publish_player_names(server: &Server) -> bool {
    let mut names = Vec::new();
    let mut display_names = Vec::new();
    let mut uuids = Vec::new();
    for player in server.get_all_players() {
        if player.as_java().is_none() {
            continue;
        }
        names.push(player.get_name());
        let display = player.get_display_name().get_text();
        display_names.push(if display.trim().is_empty() {
            "#".to_string()
        } else {
            display.replace(',', "")
        });
        uuids.push(player.get_id().to_string());
    }
    let fields = vec![
        "UpdateNames".to_string(),
        global_config().read().settings.chat.server_id.to_string(),
        names.join(","),
        display_names.join(","),
        uuids.join(","),
    ];
    publish_fields(server, &fields)
}

/// Registers proxy payload handling and the player-list cadence.
pub fn start(context: &Context) -> Result<(), String> {
    context
        .register_event_handler::<PlayerCustomPayloadEvent, IncomingPayloadHandler>(
            IncomingPayloadHandler,
            EventPriority::Normal,
            false,
        )
        .map_err(|error| error.to_string())?;
    context
        .register_event_handler::<PlayerJoinEvent, ProxyPlayerListHandler>(
            ProxyPlayerListHandler,
            EventPriority::Normal,
            false,
        )
        .map_err(|error| error.to_string())?;
    context
        .register_event_handler::<PlayerLeaveEvent, ProxyPlayerListHandler>(
            ProxyPlayerListHandler,
            EventPriority::Normal,
            false,
        )
        .map_err(|error| error.to_string())?;
    context.schedule_repeating_task(0, 1, |server| tick(&server));
    if is_enabled() {
        crate::diag::info(format!(
            "[TrChat] Plugin-message cross-server chat started (mode '{}').",
            if configured_mode() == ProxyMode::Bungee {
                "bungee"
            } else {
                "velocity"
            }
        ));
    } else {
        crate::diag::info(
            "[TrChat] Plugin-message cross-server chat disabled (proxy.enabled: false).",
        );
    }
    Ok(())
}

fn tick(server: &Server) {
    let now = Instant::now();
    reader()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .expire(now);
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if is_enabled() && ticks % 200 == 0 {
        let _ = publish_player_names(server);
    }
}

struct IncomingPayloadHandler;

impl EventHandler<PlayerCustomPayloadEvent> for IncomingPayloadHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerCustomPayloadEvent>,
    ) -> EventData<PlayerCustomPayloadEvent> {
        if is_enabled() && event.channel == configured_mode().incoming_channel() {
            let decoded = reader()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .accept(&event.data, Instant::now());
            if let Some(fields) = decoded {
                crate::redis::handle_fields(&server, &fields);
            }
        }
        event
    }
}

struct ProxyPlayerListHandler;

impl EventHandler<PlayerJoinEvent> for ProxyPlayerListHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        if is_enabled() {
            let _ = publish_player_names(&server);
        }
        event
    }
}

impl EventHandler<PlayerLeaveEvent> for ProxyPlayerListHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerLeaveEvent>,
    ) -> EventData<PlayerLeaveEvent> {
        if is_enabled() {
            let _ = publish_player_names(&server);
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<String> {
        vec![
            "BroadcastRaw".to_string(),
            "sender".to_string(),
            "payload with 中文 and a deliberately long tail ".repeat(4_000),
        ]
    }

    #[test]
    fn codec_round_trips_out_of_order_chunks() {
        let original = fields();
        let packets = encode_fields(&original);
        assert!(packets.len() > 1);
        let mut reader = Reader::default();
        let mut decoded = None;
        for packet in packets.iter().rev() {
            decoded = reader.accept(packet, Instant::now()).or(decoded);
        }
        assert_eq!(decoded, Some(original));
    }

    #[test]
    fn duplicate_chunks_are_ignored() {
        let original = vec!["GlobalMute".to_string(), "on".to_string()];
        let packets = encode_fields(&original);
        let mut reader = Reader::default();
        assert_eq!(
            reader.accept(&packets[0], Instant::now()),
            Some(original.clone())
        );
        assert_eq!(reader.accept(&packets[0], Instant::now()), None);
    }

    #[test]
    fn modes_match_the_upstream_channels() {
        assert_eq!(
            ProxyMode::from_config("bungee").outgoing_channel(),
            "trchat:main"
        );
        assert_eq!(
            ProxyMode::from_config("bungee").incoming_channel(),
            "trchat:main"
        );
        assert_eq!(
            ProxyMode::from_config("velocity").outgoing_channel(),
            "trchat:proxy"
        );
        assert_eq!(
            ProxyMode::from_config("velocity").incoming_channel(),
            "trchat:server"
        );
        assert_eq!(
            ProxyMode::from_config("unknown").incoming_channel(),
            "trchat:server"
        );
    }

    #[test]
    fn malformed_and_oversized_packets_are_ignored() {
        let mut reader = Reader::default();
        assert_eq!(reader.accept(br"not-json", Instant::now()), None);
        let packet = serde_json::to_vec(&WirePacket {
            uid: "not-a-uuid".to_string(),
            index: 1,
            total: 1,
            data: STANDARD.encode(br#"["GlobalMute","on"]"#),
        })
        .unwrap();
        assert_eq!(reader.accept(&packet, Instant::now()), None);
        let oversized = serde_json::to_vec(&WirePacket {
            uid: next_uid(),
            index: 1,
            total: 1,
            data: "a".repeat(MAX_CHUNK_LENGTH + 3_000),
        })
        .unwrap();
        assert!(oversized.len() > MAX_PACKET_BYTES);
        assert_eq!(reader.accept(&oversized, Instant::now()), None);
    }

    #[test]
    fn incomplete_messages_expire() {
        let original = fields();
        let packets = encode_fields(&original);
        let first = Instant::now();
        let mut reader = Reader::default();
        assert_eq!(reader.accept(&packets[0], first), None);
        assert_eq!(reader.pending.len(), 1);
        reader.expire(first + MESSAGE_TTL + Duration::from_millis(1));
        assert!(reader.pending.is_empty());
        assert_eq!(
            reader.accept(&packets[0], first + MESSAGE_TTL + Duration::from_millis(2)),
            None
        );
    }
}
