//! Chat pipeline — the Bukkit v2 `ChatService.handleChat` flow ported to Pumpkin.
//!
//! Order of operations (mirroring `guardMessage`, see `docs/spec/chat.md`):
//!
//! 1. trim; empty message → swallowed,
//! 2. length guard (`messageMaxLength`, counted in UTF-16 code units),
//! 3. global mute → per-player mute,
//! 4. cooldown (`cooldownMillis`, measured from the last *accepted* message),
//! 5. anti-repeat similarity guard,
//! 6. filtering — `filter.yml` profile (`TextFilter`) then `settings.yml`
//!    blocked words (`MessageGuard`),
//! 7. channel routing (longest prefix), speak-permission check, radius check,
//! 8. rendering with the channel format + built-in placeholders,
//! 9. broadcast to every eligible online player.
//!
//! `TextComponent` is a WIT resource handle (not `Clone`), so the rendered
//! component is built once per receiver from the same template string.

use pumpkin_plugin_api::{
    events::{
        player::{PlayerChatEvent, PlayerCommandPreprocessEvent},
        EventData, EventHandler, EventPriority,
    },
    text::TextComponent,
    Context, Server,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::config::{color_code, ChannelConfig, Route, SharedConfig, TrChatConfig};
use crate::filter::{MessageGuard, TextFilter};
use crate::functions;
use crate::lang;
use crate::placeholder;
use crate::playerdata::SessionPlayers;
use crate::special;

/// Per-player transient chat state (cooldown + recent messages). In the Bukkit
/// plugin the same data lives in `ChatService` maps keyed by UUID; here the
/// lowercased player name is the key (unique per server session, and the WIT
/// `uuid` type has no string form yet).
#[derive(Default)]
struct PlayerChatState {
    /// When the last *accepted* message was sent (cooldown source).
    last_sent_at: Option<Instant>,
    /// Recently sent messages for the anti-repeat guard.
    recent: VecDeque<RecentMessage>,
}

struct RecentMessage {
    text: String,
    at: Instant,
}

static PLAYER_STATES: OnceLock<Mutex<HashMap<String, PlayerChatState>>> = OnceLock::new();

fn states() -> &'static Mutex<HashMap<String, PlayerChatState>> {
    PLAYER_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Owns chat wiring for the plugin.
pub struct ChatManager;

impl ChatManager {
    /// Loads the configuration and registers the chat event handler.
    pub fn init(context: Context) -> Result<(), String> {
        let config = SharedConfig::load(&context)?;
        // Seed the process-wide config handle used by the command surface
        // (`commands::ChannelCommand`, `MsgCommand`) before any command runs.
        crate::config::init_global(&config, context.get_data_folder());
        context
            .register_event_handler::<PlayerChatEvent, ChatHandler>(
                ChatHandler {
                    config: config.clone(),
                },
                EventPriority::High,
                true,
            )
            .map_err(|e| e.to_string())?;
        // The command guard rides the preprocess hook so a denied command
        // never reaches the server (`ChatFunctionService.checkCommand`).
        context
            .register_event_handler::<PlayerCommandPreprocessEvent, CommandGuardHandler>(
                CommandGuardHandler { config },
                EventPriority::High,
                true,
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Handles `PlayerCommandPreprocessEvent` — the `Command-Controller` guard.
struct CommandGuardHandler {
    config: SharedConfig,
}

impl EventHandler<PlayerCommandPreprocessEvent> for CommandGuardHandler {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerCommandPreprocessEvent>,
    ) -> EventData<PlayerCommandPreprocessEvent> {
        let verdict =
            crate::command_controller::check_command(&event.player, &event.command, &self.config);

        let key = match verdict {
            crate::command_controller::Verdict::Allow => return event,
            crate::command_controller::Verdict::Deny => "Command-Controller-Deny",
            crate::command_controller::Verdict::Cooldown => "Command-Controller-Cooldown",
        };

        let locale = event.player.get_locale();
        let text = lang::lang().read().unwrap_or_else(|e| e.into_inner()).format(
            key,
            &locale,
            &[],
        );
        let _ = event.player.send_system_message(
            TextComponent::from_legacy_string_with_code(&text, '&'),
            false,
        );
        event.cancelled = true;
        event
    }
}

/// Handles `PlayerChatEvent` — the whole `handleChat` pipeline.
struct ChatHandler {
    config: SharedConfig,
}

impl EventHandler<PlayerChatEvent> for ChatHandler {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<PlayerChatEvent>,
    ) -> EventData<PlayerChatEvent> {
        let config = self.config.read();
        let _ = chat_pipeline(&server, &mut event, &config);

        // Every path intercepted here suppresses the vanilla broadcast; the
        // plugin's own rendering has already reached the allowed receivers.
        event.cancelled = true;
        event.message = String::new();
        event.recipients = Vec::new();
        event
    }
}

/// The full guard → filter → route → render → broadcast flow.
/// Returns `true` when the message was accepted (broadcast), `false` when a
/// guard rejected it (the event is cancelled either way).
fn chat_pipeline(
    server: &Server,
    event: &mut EventData<PlayerChatEvent>,
    config: &TrChatConfig,
) -> bool {
    let _ = server;
    let name = event.player.get_name();
    let locale = event.player.get_locale();
    let message = event.message.trim();

    // 1. Empty message → silently swallowed (Bukkit: `return` without hint).
    if message.is_empty() {
        return false;
    }

    // 2. Length guard — UTF-16 code units (Java `String.length()`), not chars.
    let length = message.encode_utf16().count();
    let max_len = config.message_max_length().max(1) as usize;
    if length > max_len {
        let text = lang::lang()
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .format(
                "General-Too-Long",
                &locale,
                &[&length.to_string(), &max_len.to_string()],
            );
        let _ = event.player.send_system_message(
            TextComponent::from_legacy_string_with_code(&text, '&'),
            false,
        );
        return false;
    }

    // 3. Mute guards: global mute first, then the player's own mute.
    {
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        if session.is_global_muted() || session.is_muted(&name) {
            let key = if session.is_global_muted() {
                "General-Global-Muting"
            } else {
                "General-Muted"
            };
            // The session store tracks no expiry/reason yet — fill the
            // upstream `{0}`/`{1}` args with placeholder values.
            let expiry = "∞".to_string();
            let reason = "—".to_string();
            let text = lang::lang()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .format(key, &locale, &[&expiry, &reason]);
            let _ = event.player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
            return false;
        }
    }

    let player_key = name.to_ascii_lowercase();

    // 4. Cooldown — measured from the last accepted message.
    {
        let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(player_key.clone()).or_default();
        if let Some(last) = state.last_sent_at {
            let cooldown = config.cooldown_millis().max(0) as u128;
            if last.elapsed().as_millis() < cooldown {
                let text = lang::lang()
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .format("Cooldowns-Chat", &locale, &[&cooldown.to_string()]);
                let _ = event.player.send_system_message(
                    TextComponent::from_legacy_string_with_code(&text, '&'),
                    false,
                );
                return false;
            }
        }
    }

    // 5. Anti-repeat — only similar messages are recorded; with
    //    `antiRepeatMaxPerPeriod: 0` any similar message is blocked.
    {
        let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
        let state = guard
            .get_mut(&player_key)
            .expect("state exists after cooldown");
        let period = config.anti_repeat_period_millis().max(1) as u128;
        let now = Instant::now();
        while state
            .recent
            .front()
            .is_some_and(|m| now.duration_since(m.at).as_millis() > period)
        {
            state.recent.pop_front();
        }
        let similarity = config.anti_repeat_similarity().max(0.0).min(1.0);
        let too_similar = state
            .recent
            .iter()
            .any(|m| similarity_score(&m.text, message) >= similarity);
        if too_similar {
            let text = lang::lang()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .format("General-Too-Similar", &locale, &[]);
            let _ = event.player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
            return false;
        }
        state.recent.push_back(RecentMessage {
            text: message.to_string(),
            at: now,
        });
    }

    // 6. Filtering — the `filter.yml` profile first (local words, ignored
    //    punctuation, white list; the Mod's `FilterService`), then the
    //    `settings.yml` blocked-words guard (the Mod's `MessageGuard`).
    let message = {
        let f = config.filter_config();
        let sensitive = TextFilter::new(
            &f.local_words,
            &f.ignored_punctuations,
            &f.white_list,
            f.replacement,
        );
        let text = if f.chat_enabled && sensitive.is_active() {
            sensitive.filter(message)
        } else {
            message.to_string()
        };
        MessageGuard::new(config.blocked_words(), config.filter_replacement()).filter(&text)
    };

    // 7. Channel routing (longest prefix wins) + speak permission check.
    let route = config.route(&message);
    let (channel, body) = match route {
        Route::Channel(channel, body) => (Some(channel), body),
        Route::Plain(body) => (None, body),
    };

    if let Some(channel) = channel {
        if !channel.permission().is_empty() && !event.player.has_permission(channel.permission()) {
            let text = lang::lang()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .format("Channel-No-Speak-Permission", &locale, &[]);
            let _ = event.player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
            return false;
        }
    }

    // 8. Chat functions (§1.3 step 6) — `Mention` / `Mention-All` scanning,
    //    permission + cooldown gating, and span rendering. Runs after the
    //    speak-permission check and before any receiver sees the message; it
    //    also strips legacy codes, so a `None` outcome keeps the plain
    //    template render (the Mod's no-component path, §3.1).
    let disabled: &[String] = channel
        .map(|c| c.options.disabled_functions.as_slice())
        .unwrap_or(&[]);
    let outcome = functions::process(server, &event.player, &body, config, disabled);

    // 8b. Render — one template string, then one component per receiver.
    let server_name = config.server_name();
    let world = event.player.get_world().get_name();
    let template = match (&outcome, channel) {
        // A processed body carries its own styled component, so the body text
        // is *not* interpolated into the template; the caller passes the
        // component instead (§3.1: "若调用方传入了 messageComponent").
        (Some(_), Some(ch)) => render_template(
            &ch.template,
            &name,
            "",
            &ch.id,
            server_name,
            &world,
            "",
            &event.player,
            server,
            config,
        ),
        (Some(_), None) => render_template(
            &config.plain_template(),
            &name,
            "",
            "",
            server_name,
            &world,
            "",
            &event.player,
            server,
            config,
        ),
        (None, Some(ch)) => {
            let body = wrap_special_characters(ch, &body);
            render_template(
                &ch.template,
                &name,
                &body,
                &ch.id,
                server_name,
                &world,
                "",
                &event.player,
                server,
                config,
            )
        }
        (None, None) => render_template(
            &config.plain_template(),
            &name,
            &body,
            "",
            server_name,
            &world,
            "",
            &event.player,
            server,
            config,
        ),
    };

    // 9. Broadcast — every online player; radius-limited channels use squared
    //    distance (Bukkit `DISTANCE` semantics; 0.0 = unlimited).
    let radius = channel.map(|c| c.radius()).unwrap_or(0.0f64);
    let origin = event.player.get_position();
    // Receivers that really got the message, in broadcast order — the notify
    // pass (§1.3 step 9) only ever touches these players.
    let mut receivers: Vec<&pumpkin_plugin_api::player::Player> = Vec::new();
    let players = server.get_all_players();
    for player in &players {
        if radius > 0.0 {
            let pos = player.get_position();
            let dx = pos.0 - origin.0;
            let dy = pos.1 - origin.1;
            let dz = pos.2 - origin.2;
            if dx * dx + dy * dy + dz * dz > radius * radius {
                continue;
            }
        }
        let component = match &outcome {
            Some(out) => functions::build_body_component(&template, out, &name, &locale),
            None => TextComponent::from_legacy_string_with_code(&template, '&'),
        };
        let _ = player.send_system_message(component, false);
        receivers.push(player);
    }

    // 9b. `notifyMentioned` (§1.3 step 9, §2.9) — titles/sound for mentioned
    //     players that actually received the broadcast.
    if let Some(out) = &outcome {
        if !out.mentioned.is_empty() {
            for player in &receivers {
                let receiver = player.get_name().to_ascii_lowercase();
                if receiver.eq_ignore_ascii_case(&name) {
                    continue;
                }
                if out.mentioned.contains(&receiver) {
                    functions::notify_mentioned(player, &name, &player.get_locale());
                }
            }
        }
    }

    // Accepted — update the cooldown timestamp.
    {
        let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = guard.get_mut(&player_key) {
            state.last_sent_at = Some(Instant::now());
        }
    }
    true
}

/// Renders a format template with the built-in placeholders (`{player}`,
/// `{message}`, `{channel}`, `{server}`, `{world}`, `{target}` for private
/// chat). Config templates are normalized in `config.rs` from the Mod's
/// `%player_name%`-style tokens to these `{…}` names.
///
/// `%token%` placeholders (`%server_online%`, `%player_health%`, …) are
/// resolved first (§1.1), against the **message subject** for `player_*`.
/// Message bodies are resolved *and then* legacy-code-stripped, matching the
/// Mod's body pipeline (§1.1 / `ChannelRenderer.java:116-133`).
fn render_template(
    template: &str,
    name: &str,
    message: &str,
    channel: &str,
    server: &str,
    world: &str,
    target: &str,
    player: &pumpkin_plugin_api::player::Player,
    server_ref: &Server,
    config: &crate::config::TrChatConfig,
) -> String {
    let template = placeholder::resolve(template, player, server_ref, config);
    let message = placeholder::resolve(message, player, server_ref, config);
    template
        .replace("{player}", name)
        .replace("{message}", &message)
        .replace("{channel}", channel)
        .replace("{server}", server)
        .replace("{world}", world)
        .replace("{target}", target)
}

/// Applies the channel's special-character wrap to the message body (Mod
/// `ChannelRenderer` behavior): configured resource-pack glyphs get the
/// channel's `msg.special-char-color`, with the message default color
/// restored after each glyph run. `special-chars.yml` is loaded process-wide
/// by [`crate::special::reload`]; an empty table or an empty color leaves the
/// body untouched.
fn wrap_special_characters(ch: &ChannelConfig, body: &str) -> String {
    let Some(layer) = ch.render_layer() else {
        return body.to_string();
    };
    if layer.special_char_color.is_empty() || !special::has_special_chars(body) {
        return body.to_string();
    }
    let color = color_code(&layer.special_char_color);
    let default = color_code(&layer.msg_default_color);
    special::wrap_special_chars(body, &color, &default)
}

/// Normalized similarity in `[0, 1]` (a plain-normalized Jaro–Winkler stand-in
/// for the ordered similarity used by the Bukkit anti-repeat guard).
fn similarity_score(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let ca: Vec<char> = a.chars().collect();
    let cb: Vec<char> = b.chars().collect();
    if ca.is_empty() || cb.is_empty() {
        return 0.0;
    }
    let max_dist = (ca.len().max(cb.len()) / 2).saturating_sub(1);
    let mut a_matched = vec![false; ca.len()];
    let mut b_matched = vec![false; cb.len()];
    let mut matches = 0usize;
    for (i, &ca_i) in ca.iter().enumerate() {
        let lo = i.saturating_sub(max_dist);
        let hi = (i + max_dist + 1).min(cb.len());
        for j in lo..hi {
            if !b_matched[j] && cb[j] == ca_i {
                a_matched[i] = true;
                b_matched[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let mut t = 0usize;
    let mut j = 0usize;
    for (i, matched) in a_matched.iter().enumerate() {
        if !*matched {
            continue;
        }
        while j < b_matched.len() && !b_matched[j] {
            j += 1;
        }
        if j >= b_matched.len() {
            break;
        }
        if ca[i] != cb[j] {
            t += 1;
        }
        j += 1;
    }
    let m = matches as f64;
    let t = t as f64 / 2.0;
    (m / ca.len() as f64 + m / cb.len() as f64 + (m - t) / m) / 3.0
}

#[cfg(test)]
mod tests {
    use super::similarity_score;

    #[test]
    fn similarity_basics() {
        assert_eq!(similarity_score("hello", "hello"), 1.0);
        assert!(similarity_score("hello", "helloo") > 0.9);
        assert!(similarity_score("hello", "world") < 0.5);
    }
}
