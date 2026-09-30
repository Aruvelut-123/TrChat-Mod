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
    player::Player,
    text::TextComponent,
    Context, Server,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::condition;
use crate::config::{
    color_code, ChannelConfig, ChannelTarget, Route, SharedConfig, TrChatConfig,
};
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
        // A disabled world must yield to vanilla chat: return *without*
        // cancelling, so the original broadcast still happens (§1.1 step 2).
        if let ChatOutcome::DisabledWorld = chat_pipeline(&server, &event.player, &event.message, &config) {
            return event;
        }

        // Every other path intercepted here suppresses the vanilla broadcast;
        // the plugin's own rendering has already reached the allowed receivers.
        event.cancelled = true;
        event.message = String::new();
        event.recipients = Vec::new();
        event
    }
}

/// Sends `message` *as if* `player` had typed it, outside an event context.
///
/// Bound channel aliases (`/global hello`) use this so a body sent through an
/// alias takes exactly the same guard → filter → route → render → broadcast
/// path as the prefixed spelling (`!all hello`), with no duplicate logic.
pub fn dispatch_as_chat(server: &Server, player: &Player, message: &str) {
    let config = crate::config::global_config();
    let config = config.read();
    let _ = chat_pipeline(server, player, message, &config);
}

/// The result of a chat-pipeline run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatOutcome {
    /// The message was accepted and broadcast to its receivers.
    Accepted,
    /// A guard rejected it, or the message was swallowed.
    Rejected,
    /// The world is disabled in `chat.disabledWorlds`, so the plugin must not
    /// touch the event and vanilla chat keeps the message (§1.1 step 2).
    DisabledWorld,
}

/// The full guard → filter → route → render → broadcast flow.
///
/// [`ChatOutcome::DisabledWorld`] asks the caller to leave the event alone;
/// every other outcome means the vanilla broadcast must be suppressed.
fn chat_pipeline(
    server: &Server,
    player: &Player,
    raw_message: &str,
    config: &TrChatConfig,
) -> ChatOutcome {
    let _ = server;
    let name = player.get_name();
    let locale = player.get_locale();
    let message = raw_message.trim();

    // 1. Empty message → silently swallowed (Bukkit: `return` without hint).
    if message.is_empty() {
        return ChatOutcome::Rejected;
    }

    // 1b. Disabled world — hand the message back to vanilla chat. Checked
    //     before every guard so a disabled world never sees a hint either.
    if config.settings.chat.is_disabled_world(&player.get_world().get_name()) {
        return ChatOutcome::DisabledWorld;
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
        let _ = player.send_system_message(
            TextComponent::from_legacy_string_with_code(&text, '&'),
            false,
        );
        return ChatOutcome::Rejected;
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
            let _ = player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
            return ChatOutcome::Rejected;
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
                let _ = player.send_system_message(
                    TextComponent::from_legacy_string_with_code(&text, '&'),
                    false,
                );
                return ChatOutcome::Rejected;
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
            let _ = player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
            return ChatOutcome::Rejected;
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

    // 7. Channel routing (longest prefix wins) + speak check (`Speak-Condition`
    //    when set, otherwise `Join-Permission`).
    let route = config.route(&message);
    let (channel, body) = match route {
        Route::Channel(channel, body) => (Some(channel), body),
        Route::Plain(body) => (None, body),
    };

    if let Some(channel) = channel {
        // §3 `canSpeak`: a non-empty `Speak-Condition` replaces the
        // `Join-Permission` check (config.md §5 note 5).
        if !condition::can_speak(channel.speak_condition(), channel.permission(), player) {
            let text = lang::lang()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .format("Channel-No-Speak-Permission", &locale, &[]);
            let _ = player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
            return ChatOutcome::Rejected;
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
    let outcome = functions::process(server, player, &body, config, disabled);

    // 8b. Render — one template string, then one component per receiver.
    let server_name = config.server_name();
    let world = player.get_world().get_name();
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
            player,
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
            player,
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
                player,
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
            player,
            server,
            config,
        ),
    };

    // 9. Broadcast — every online player, subject to the four §2.3 receiver
    //    checks: ignore list, channel membership, listen permission, and the
    //    `Target` reach (SELF / WORLD / DISTANCE, squared comparison).
    let target = channel.map(|c| c.target()).unwrap_or(ChannelTarget::All);
    let origin = player.get_position();
    let origin_world = player.get_world().get_name();
    // Receivers that really got the message, in broadcast order — the notify
    // pass (§1.3 step 9) only ever touches these players.
    let mut receivers: Vec<&pumpkin_plugin_api::player::Player> = Vec::new();
    let players = server.get_all_players();
    for player in &players {
        let receiver_name = player.get_name();

        // (1) A player who ignored the sender never receives their chat.
        {
            let session = SessionPlayers::global();
            let session = session.read().unwrap_or_else(|e| e.into_inner());
            if session.ignores(&receiver_name, &name) {
                continue;
            }
        }

        if let Some(channel) = channel {
            // (2) Membership — `Always-Listen` bypasses the join requirement.
            if !channel.always_listen() {
                let session = SessionPlayers::global();
                let session = session.read().unwrap_or_else(|e| e.into_inner());
                let joined = session
                    .state(&receiver_name)
                    .map(|s| s.joined_channels.clone())
                    .unwrap_or_default();
                drop(session);
                if !channel.is_joined_by(&joined) {
                    continue;
                }
            }

            // (3) Receive permission — empty means everyone.
            let listen = channel.listen_permission();
            if !listen.is_empty() && !player.has_permission(listen) {
                continue;
            }

            // (4) Reach — SELF / WORLD / DISTANCE.
            match target {
                ChannelTarget::All => {}
                ChannelTarget::SelfOnly => {
                    if !receiver_name.eq_ignore_ascii_case(&name) {
                        continue;
                    }
                }
                ChannelTarget::SameWorld => {
                    if player.get_world().get_name() != origin_world {
                        continue;
                    }
                }
                ChannelTarget::Distance(limit) => {
                    // A negative limit (unparsable `DISTANCE;`) matches nobody.
                    if limit < 0.0 || player.get_world().get_name() != origin_world {
                        continue;
                    }
                    let pos = player.get_position();
                    let dx = pos.0 - origin.0;
                    let dy = pos.1 - origin.1;
                    let dz = pos.2 - origin.2;
                    if dx * dx + dy * dy + dz * dz > limit * limit {
                        continue;
                    }
                }
            }
        }

        let component = match &outcome {
            Some(out) => functions::build_body_component(
                &template,
                out,
                &name,
                &locale,
                player,
                server,
                config,
            ),
            None => TextComponent::from_legacy_string_with_code(&template, '&'),
        };
        // §4.4/§4.5 — the flattened template cannot carry per-part hover/click
        // events, so re-attach the selected tier's component parts here.
        let component = apply_prefix_events(
            component,
            channel,
            &name,
            &world,
            server_name,
            player,
            server,
            config,
        );
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
                    functions::notify_mentioned(player, name.as_str(), &player.get_locale());
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
    ChatOutcome::Accepted
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
    // §1.12 — `%message%` is a `local` context key: it holds this message's raw
    // text, so a format may echo it before `{message}` substitution.
    let message = placeholder::resolve_with_local(
        message,
        player,
        server_ref,
        config,
        &[("message", message)],
    );
    template
        .replace("{player}", name)
        .replace("{message}", &message)
        .replace("{channel}", channel)
        .replace("{server}", server)
        .replace("{world}", world)
        .replace("{target}", target)
}

/// §4.4/§4.5 — re-attaches the selected tier's component-part hover and click
/// events to the rendered message.
///
/// The channel template is flattened to one legacy string, which loses every
/// per-part event. Each configured prefix part becomes a leading child
/// component carrying its own hover/click, so a clickable label such as
/// `[Site]` still opens its URL — matching the upstream tree, where the
/// component parts precede `msg`.
fn apply_prefix_events(
    body: TextComponent,
    channel: Option<&ChannelConfig>,
    name: &str,
    world: &str,
    server_name: &str,
    player: &pumpkin_plugin_api::player::Player,
    server: &Server,
    config: &crate::config::TrChatConfig,
) -> TextComponent {
    let Some(ch) = channel else {
        return body;
    };
    let parts = crate::config::selected_prefix_parts(&ch.formats);
    if parts.is_empty() {
        return body;
    }
    // The message text itself is already inside `body`; component children
    // append *after* the parent text, so the parts are rendered by prefixing
    // them onto a fresh root whose styles match the flattened template.
    let mut parts_component: Option<TextComponent> = None;
    for part in &parts {
        let raw = placeholder::resolve(&part.text, player, server, config)
            .replace("{player}", name)
            .replace("{channel}", &ch.id)
            .replace("{server}", server_name)
            .replace("{world}", world);
        let mut c = TextComponent::from_legacy_string_with_code(&raw, '&');
        if !part.hover.is_empty() {
            let hover = placeholder::resolve(&part.hover, player, server, config);
            c = c.hover_show_text(TextComponent::from_legacy_string_with_code(&hover, '&'));
        }
        if !part.insertion.is_empty() {
            c = c.insertion(&part.insertion);
        }
        if !part.font.is_empty() {
            c = c.font(&part.font);
        }
        if let Some(action) = part.click_action() {
            c = action.apply(c);
        }
        parts_component = Some(match parts_component {
            Some(acc) => acc.add_child(c),
            None => c,
        });
    }
    match parts_component {
        // `body` keeps the message text; the parts ride along as a sibling in
        // front of it, which is how the upstream template renders.
        Some(parts_c) => parts_c.add_child(body),
        None => body,
    }
}
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
