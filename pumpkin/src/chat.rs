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
    color_code, Audience, ChannelConfig, ChannelTarget, FormatLayer, Route, SharedConfig,
    TrChatConfig,
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
    /// The anti-repeat period list (§5 `:705-711`). Only messages judged
    /// *similar* are pushed, so with `compareAll: true` the comparison set
    /// holds historical similar messages only (chat.md §1.4 note).
    recent: VecDeque<RecentMessage>,
    /// Anti-high-frequency window: arrival times of *accepted* messages. The
    /// Mod writes state only after every guard passes (chat.md §1.4), so
    /// blocked attempts never count toward the limit.
    sends: VecDeque<Instant>,
    /// The last *accepted* message text, the target of a `compareAll: false`
    /// similarity check ("只比上一条").
    last_message: Option<String>,
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
    if config
        .settings
        .chat
        .is_disabled_world(&player.get_world().get_name())
    {
        return ChatOutcome::DisabledWorld;
    }

    // 2. Prefix routing (§1.2) — runs on the trimmed *raw* text, before any
    //    guard, so a blocked word can never change which channel is chosen.
    let route = config.route(message);
    let (channel, body) = match route {
        Route::Channel(channel, body) => (Some(channel), body),
        Route::Plain(body) => (None, body),
    };

    // §1.2 step 5: an empty body after prefix stripping, or a channel flagged
    // private, returns silently — no hint is sent either way.
    if body.is_empty() {
        return ChatOutcome::Rejected;
    }
    if channel.is_some_and(|c| c.options.private) {
        return ChatOutcome::Rejected;
    }

    // 3. `canSpeak` (§1.3 step 1) — checked before `guardMessage`, so a player
    //    without speak rights sees the permission hint rather than a length or
    //    cooldown hint. A non-empty `Speak-Condition` replaces the
    //    `Join-Permission` check (config.md §5 note 5).
    if let Some(channel) = channel {
        if !condition::can_speak(channel.speak_condition(), channel.permission(), player) {
            return reject_with(player, &locale, "Channel-No-Speak-Permission", &[]);
        }
    }

    // 4. `guardMessage` (§1.3 step 2, order fixed by §1.4). Every guard below
    //    runs on the prefix-stripped `body`, not on the raw text.
    //
    //    Length guard — UTF-16 code units (Java `String.length()`), not chars.
    //    OP does *not* bypass this one.
    let length = body.encode_utf16().count();
    let max_len = config.message_max_length().max(1) as usize;
    if length > max_len {
        let (length, max_len) = (length.to_string(), max_len.to_string());
        return reject_with(player, &locale, "General-Too-Long", &[&length, &max_len]);
    }

    // §1.4 steps 3–4. Mute guards — the global mute exempts OPs (§1.4 line 60)
    //    while a personal mute applies to everyone. Order between them is fixed.
    let is_op = condition::is_op(player);
    {
        // §6 — a personal mute whose expiry has passed clears itself on read
        // (`ModerationService.java:54-62`).
        {
            let mut session = SessionPlayers::global()
                .write()
                .unwrap_or_else(|e| e.into_inner());
            session.expire_mute(&name);
        }
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        // The global mute is skipped for operators, the personal mute is not
        // (spec §1.4 steps 3-4).
        let globally_muted = session.is_global_muted() && !is_op;
        let personally_muted = session.is_muted(&name);
        if globally_muted || personally_muted {
            // `General-Muted` interpolates the expiry and the reason.
            let detail = session.mute_state(&name).map(|(until, reason)| {
                (
                    crate::playerdata::mute_expiry_text(until),
                    reason.to_string(),
                )
            });
            drop(session);
            if globally_muted {
                return reject_with(player, &locale, "General-Global-Muting", &[]);
            }
            let (expiry, reason) =
                detail.unwrap_or_else(|| ("permanent".to_string(), "-".to_string()));
            return reject_with(player, &locale, "General-Muted", &[&expiry, &reason]);
        }
    }

    let player_key = name.to_ascii_lowercase();

    // §1.4 step 5. Anti-repeat (algorithm §5). OP and `trchat.bypass.repeat`
    //    are exempt. `antiRepeatSimilarity: 0` *disables* the guard, whereas
    //    `antiRepeatMaxPerPeriod: 0` blocks the very first similar message.
    if !is_op && !player.has_permission("trchat.bypass.repeat") {
        let similarity = config.anti_repeat_similarity().clamp(0.0, 1.0);
        if similarity > 0.0 {
            let max_per_period = config.anti_repeat_max_per_period() as usize;
            let compare_all = config.anti_repeat_compare_all();
            let period = period_or_default(config.anti_repeat_period_millis());
            let now = Instant::now();
            let blocked = {
                let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
                let state = guard.entry(player_key.clone()).or_default();
                while state
                    .recent
                    .front()
                    .is_some_and(|m| now.duration_since(m.at).as_millis() > period)
                {
                    state.recent.pop_front();
                }
                // `compareAll: false` compares only the previous accepted
                // message; `true` compares the period list, which by
                // construction holds historical *similar* messages only.
                let too_similar = if compare_all {
                    state
                        .recent
                        .iter()
                        .any(|m| similarity_score(&m.text, &body) >= similarity)
                } else {
                    state
                        .last_message
                        .as_deref()
                        .is_some_and(|last| similarity_score(last, &body) >= similarity)
                };
                if too_similar {
                    // Only similar messages join the period list, and the limit
                    // is inclusive: `0` allows none, so the first is blocked.
                    state.recent.push_back(RecentMessage {
                        text: body.to_string(),
                        at: now,
                    });
                    state.recent.len() > max_per_period
                } else {
                    false
                }
            };
            if blocked {
                return reject_with(player, &locale, "General-Too-Similar", &[]);
            }
        }
    }

    // §1.4 step 6. Anti-duplicate phrase (algorithm §5 `maxConsecutiveRepeat`).
    //    OP does *not* bypass this one — only `trchat.bypass.duplicate` does —
    //    and a `maxRepeat` of 0 disables it.
    let max_repeat = config.anti_duplicate_phrase_max_repeat() as usize;
    if max_repeat > 0 && !player.has_permission("trchat.bypass.duplicate") {
        let repeats = max_consecutive_repeat(&body, config.anti_duplicate_phrase_whitelist());
        if repeats > max_repeat {
            return reject_with(player, &locale, "General-Too-Duplicate", &[]);
        }
    }

    // §1.4 step 7. Cooldown — measured from the last message that passed every
    //    guard, so a failed guard never refreshes the timestamp. OP is exempt.
    if !is_op {
        let cooldown = config.cooldown_millis().max(0) as u128;
        let remaining = {
            let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
            let state = guard.entry(player_key.clone()).or_default();
            state.last_sent_at.and_then(|last| {
                let elapsed = last.elapsed().as_millis();
                (elapsed < cooldown).then_some(cooldown - elapsed)
            })
        };
        if let Some(remaining) = remaining {
            let remaining = remaining.to_string();
            return reject_with(player, &locale, "Cooldowns-Chat", &[&remaining]);
        }
    }

    // §1.4 step 8. Anti-high-frequency. OP and `trchat.bypass.highfrequency`
    //    are exempt; a `max` of 0 disables the guard.
    if !is_op && !player.has_permission("trchat.bypass.highfrequency") {
        let max_per_period = config.anti_high_frequency_max_per_period() as usize;
        if max_per_period > 0 {
            let period = period_or_default(config.anti_high_frequency_period_millis());
            let now = Instant::now();
            let blocked = {
                let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
                let state = guard.entry(player_key.clone()).or_default();
                while state
                    .sends
                    .front()
                    .is_some_and(|at| now.duration_since(*at).as_millis() > period)
                {
                    state.sends.pop_front();
                }
                state.sends.len() >= max_per_period
            };
            if blocked {
                return reject_with(player, &locale, "General-Too-Frequent", &[]);
            }
        }
    }

    // §1.4 step 9. Filtering — the `filter.yml` profile first (local words,
    //    ignored punctuation, white list; the Mod's `FilterService`), then the
    //    `settings.yml` blocked-words guard (the Mod's `MessageGuard`). The
    //    result replaces `body`: everything downstream (functions, rendering,
    //    stored state) sees the filtered text, while the guards above ran on
    //    the pre-filter text, as the Mod does.
    let body = {
        let f = config.filter_config();
        let sensitive = TextFilter::new(
            &f.local_words,
            &f.ignored_punctuations,
            &f.white_list,
            f.replacement,
        );
        let text = if f.chat_enabled && sensitive.is_active() {
            sensitive.filter(&body)
        } else {
            body.clone()
        };
        MessageGuard::new(config.blocked_words(), config.filter_replacement()).filter(&text)
    };

    // Every guard has now passed, so the per-player state is written here
    // (§1.4 `:747`). The stored text is the *filtered* one.
    {
        let now = Instant::now();
        let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(player_key.clone()).or_default();
        state.last_sent_at = Some(now);
        state.last_message = Some(body.clone());
        // The high-frequency window counts accepted messages only. Trim here as
        // well so a long-idle player's list cannot grow without bound.
        let period = period_or_default(config.anti_high_frequency_period_millis());
        while state
            .sends
            .front()
            .is_some_and(|at| now.duration_since(*at).as_millis() > period)
        {
            state.sends.pop_front();
        }
        state.sends.push_back(now);
    }

    // 8. Chat functions (§1.3 step 3) — `Mention` / `Mention-All` scanning,
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
    // §3 steps 1–2: the tier is chosen for this sender by `condition` with
    // `priority` descending (stable). When a channel exists but no tier passes,
    // the Mod renders the bare resolved message — no prefix, no suffix.
    let layer = channel.and_then(|ch| select_format_layer(ch, player));
    // §1.3 step 4 / §3: a chat colour the sender picked overrides the channel's
    // `msg.default-color` for their body, but only when the sender may use it.
    let chat_colour = sender_chat_color(player);
    // §3 step 3 — the prefix groups are *not* flattened into the body here:
    // `apply_prefix_events` prepends them as component parts, which is what
    // carries their per-player `condition` and their hover/click events. A body
    // that already contained them would render every prefix twice.
    let template = match (layer, channel) {
        (Some(layer), _) => crate::config::layer_body_template_with_colour(layer, &chat_colour),
        (None, Some(_)) => "{message}".to_string(),
        (None, None) => config.plain_template(),
    };
    let template = match (&outcome, channel) {
        // A processed body carries its own styled component, so the body text
        // is *not* interpolated into the template; the caller passes the
        // component instead (§3.1: "若调用方传入了 messageComponent").
        (Some(_), Some(ch)) => render_template(
            &template,
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
            &template,
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
            // §3 step 4 — the Mod cleans the body first and only then wraps the
            // resource-pack glyphs (`wrapSpecialChars(bodyText, …)` runs on the
            // already colour-stripped `bodyText`), so the wrap must happen after
            // `render_template` has stripped the player's own codes.
            let text = render_template(
                &template,
                &name,
                &body,
                &ch.id,
                server_name,
                &world,
                "",
                player,
                server,
                config,
            );
            wrap_special_characters(ch, Audience::Chat, player, &text)
        }
        (None, None) => render_template(
            &template,
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

    // §1.3 step 6 — the shadow-mute flag is read once, before broadcasting.
    let shadow_muted = {
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        session.is_shadow_muted(&name)
    };

    // The receiver-specific component: the template rendered around the
    // processed body, plus the selected tier's prefix events and `msg.hover`.
    //
    // `viewer` only feeds the body's mention/item components; the prefix parts,
    // their conditions and `msg.hover` are resolved against the **sender**, the
    // subject of the render (`ChannelRenderer.java:86-95` — the Mod passes
    // `sender` as subject and the receiver as viewer).
    let build_component = |viewer: &pumpkin_plugin_api::player::Player| {
        let component = match &outcome {
            Some(out) => functions::build_body_component(
                &template, out, &name, &locale, viewer, server, config,
            ),
            None => TextComponent::from_legacy_string_with_code(&template, '&'),
        };
        let component = apply_prefix_events(
            component,
            channel,
            Audience::Chat,
            player,
            server,
            config,
            &[],
        );
        // §3 step 5 — the tier's suffix groups trail the body (`:144`).
        let component = apply_suffix_events(
            component,
            channel,
            Audience::Chat,
            player,
            server,
            config,
            &[],
        );
        apply_msg_hover(component, channel, Audience::Chat, player, server, config)
    };

    // §1.3 step 6 — a shadow-muted sender sees their own message and nothing
    // else: it is echoed back to them, written to the log, and never broadcast
    // (nor relayed to other servers).
    if shadow_muted {
        player.send_system_message(build_component(player), false);
        log_to_console(config, player, channel, server, &body);
        return ChatOutcome::Accepted;
    }

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

        player.send_system_message(build_component(player), false);
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

    // §1.3 step 10 — `logToConsole`. Uses the `Console` tier when the channel
    //    declares one, otherwise the same template the chat audience saw.
    log_to_console(config, player, channel, server, &body);

    // Accepted — update the cooldown timestamp.
    {
        let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = guard.get_mut(&player_key) {
            state.last_sent_at = Some(Instant::now());
        }
    }
    ChatOutcome::Accepted
}

/// `logging.normalMessageFormat` — the upstream default, used when the key is
/// omitted from `settings.yml`.
const DEFAULT_NORMAL_MESSAGE_FORMAT: &str = "[{0}] {1}: {2}";

/// `logging.privateMessageFormat` — the upstream default (see
/// [`DEFAULT_NORMAL_MESSAGE_FORMAT`]).
const DEFAULT_PRIVATE_MESSAGE_FORMAT: &str = "[{0}] {1} -> {2}: {3}";

/// §1.6 `logToConsole` — writes one line to the server console.
///
/// The console line is the rendered audience view: the channel's `Console`
/// tier when the section is non-empty, otherwise its `Formats`
/// (`ChannelRenderer.java:86-99`), printed as plain text the way upstream
/// `LOGGER.info(...)` prints `component().getString()`. The `chatLogs` record
/// built from `logging.normalMessageFormat` / `logging.privateMessageFormat`
/// goes to the daily file upstream, which the WASM sandbox cannot create, so it
/// is emitted at **debug** level instead — the console keeps matching the
/// in-game format.
///
/// `{0}` of that record is rendered from the host clock in **UTC** because the
/// sandbox carries no timezone database for the Mod's system-local timestamp.
fn log_to_console(
    config: &TrChatConfig,
    player: &Player,
    channel: Option<&ChannelConfig>,
    server: &Server,
    body: &str,
) {
    // `Private: true` marks a channel whose sends are private messages; the
    // public path never reaches here with one (routing drops it at §1.2 step 5),
    // so the target is only known on the `/msg` path.
    let target = channel.filter(|c| c.options.private).map(|_| "");
    log_record(&chat_log_line(config, &player.get_name(), target, body));
    if let Some(line) = console_audience_line(channel, player, server, config, body, &[]) {
        log_line(&line);
    }
}

/// §1.6 — the private-message half of [`log_to_console`]: the `chatLogs`
/// `logPrivate` record plus the rendered `Sender`/`Receiver`/`Console` view,
/// with the target exposed to the formatter as the `%trchat_toplayer%` local.
pub fn log_private_message(
    config: &TrChatConfig,
    player: &Player,
    server: &Server,
    target: &str,
    message: &str,
) {
    log_record(&chat_log_line(
        config,
        &player.get_name(),
        Some(target),
        message,
    ));
    let channel = config.private_channel();
    if let Some(line) = console_audience_line(
        channel,
        player,
        server,
        config,
        message,
        &[("trchat_toplayer", target)],
    ) {
        log_line(&line);
    }
}

/// §1.6 — the console view of one message (`ChatService.java:960-969`).
///
/// `renderer.render(channel, CONSOLE-or-CHAT, player, null, message, local)` is
/// printed as `.component().getString()`, so the port strips the legacy codes it
/// used to build the text. `None` means the message was routed without a
/// channel (the legacy `Plain` route), which the upstream cannot reach.
fn console_audience_line(
    channel: Option<&ChannelConfig>,
    player: &Player,
    server: &Server,
    config: &TrChatConfig,
    body: &str,
    local: &[(&str, &str)],
) -> Option<String> {
    let ch = channel?;
    // §1.6 — an empty `Console` section renders with the chat audience.
    let audience = if ch.console.is_empty() {
        Audience::Chat
    } else {
        Audience::Console
    };

    // §1.12 — the body is resolved once; `%message%` echoes its own raw text.
    let mut locals: Vec<(&str, &str)> = local.to_vec();
    locals.push(("message", body));
    let message = placeholder::resolve_with_local(body, player, server, config, &locals);

    // No tier passes → the bare resolved message is rendered (`:96-99`).
    let Some(layer) = select_audience_layer(ch, audience, player) else {
        return Some(assemble_console_text(&[], &message, &[]));
    };

    // §3 steps 3/5 — one variant per prefix and suffix group, in YAML order,
    // keeping the first variant whose `condition` passes
    // (`ChannelRenderer.java:155-159`). The component path attaches these with
    // their hover/click events; the console keeps the text only, because
    // upstream prints the whole `component().getString()`.
    let prefix = resolved_group_texts(&layer.prefix, player, server, config, &ch.id, local);
    let suffix = resolved_group_texts(&layer.suffix, player, server, config, &ch.id, local);
    Some(assemble_console_text(&prefix, &message, &suffix))
}

/// The resolved text of a tier's `prefix` / `suffix` groups for the console
/// line: one variant per group, `{…}` names substituted like the component path.
fn resolved_group_texts(
    groups: &[crate::config::PartGroup],
    player: &Player,
    server: &Server,
    config: &TrChatConfig,
    channel_id: &str,
    local: &[(&str, &str)],
) -> Vec<String> {
    let name = player.get_name();
    let world = player.get_world().get_name();
    let server_name = config.server_name();
    groups
        .iter()
        .filter_map(|group| group.select(|condition| condition::test(condition, player)))
        .map(|part| {
            placeholder::resolve_with_local(&part.text, player, server, config, local)
                .replace("{player}", &name)
                .replace("{channel}", channel_id)
                .replace("{server}", server_name)
                .replace("{world}", &world)
        })
        .collect()
}

/// Joins a tier's resolved prefix groups, the resolved body and its resolved
/// suffix groups, then strips the legacy codes — which is how the port
/// reproduces upstream's `component().getString()`. The body colour code is
/// irrelevant here because `getString()` drops formatting, so no colour is
/// emitted.
fn assemble_console_text(prefix: &[String], message: &str, suffix: &[String]) -> String {
    let mut text = prefix.concat();
    text.push_str(message);
    text.push_str(&suffix.concat());
    functions::strip_legacy_codes(&text)
}

/// Emits a rendered console line at INFO through the host logger.
fn log_line(line: &str) {
    pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Info, line);
}

/// Emits one `chatLogs` record at DEBUG — the sandbox cannot write the daily
/// file the record belongs to, so it stays available without doubling the
/// console output of every chat message (see [`log_to_console`]).
fn log_record(line: &str) {
    pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Debug, line);
}

/// Builds a `chatLogs` record: `logging.normalMessageFormat` for public chat and
/// `logging.privateMessageFormat` when a `target` is given. `{0}` is the current
/// `HH:mm:ss`, and every field is flattened to a single line because the
/// upstream `ChatLogService.safe()` replaces `\r` / `\n` with spaces.
pub fn chat_log_line(
    config: &TrChatConfig,
    sender: &str,
    target: Option<&str>,
    message: &str,
) -> String {
    let logging = &config.settings.logging;
    let (configured, mut args) = match target {
        Some(target) => (
            logging.private_message_format.as_str(),
            vec![
                clock_hhmmss(),
                sender.to_string(),
                target.to_string(),
                message.to_string(),
            ],
        ),
        None => (
            logging.normal_message_format.as_str(),
            vec![clock_hhmmss(), sender.to_string(), message.to_string()],
        ),
    };
    // An omitted `logging.*MessageFormat` deserializes to an empty string;
    // fall back to the upstream default instead of logging a blank line.
    let format = if configured.is_empty() {
        if target.is_some() {
            DEFAULT_PRIVATE_MESSAGE_FORMAT
        } else {
            DEFAULT_NORMAL_MESSAGE_FORMAT
        }
    } else {
        configured
    };
    let mut out = format.to_string();
    for (index, value) in args.drain(..).enumerate() {
        out = out.replace(&format!("{{{index}}}"), &sanitise_log_field(&value));
    }
    sanitise_log_field(&out)
}

/// The `HH:mm:ss` timestamp of §1.6, from the host clock (UTC — see
/// [`log_to_console`] for why the sandbox cannot use the system timezone).
fn clock_hhmmss() -> String {
    crate::clock::now_hhmmss()
}

/// `ChatLogService.safe()` — one line per record, so `\r` and `\n` become
/// spaces.
fn sanitise_log_field(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

/// Renders a format template with the built-in placeholders (`{player}`,
/// `{message}`, `{channel}`, `{server}`, `{world}`, `{target}` for private
/// chat). Config templates are normalized in `config.rs` from the Mod's
/// `%player_name%`-style tokens to these `{…}` names.
///
/// `%token%` placeholders (`%server_online%`, `%player_health%`, …) are
/// resolved first (§1.1), against the **message subject** for `player_*`. The
/// `{message}` slot takes the Mod's `cleanMessage`: the raw text with its
/// placeholders resolved and its legacy codes **stripped**, so a player cannot
/// colour their own message — the channel's `msg.default-color` (or the picked
/// `trchat.color`) decides how it looks (`ChannelRenderer.java:116-133`,
/// chat.md §3 step 4). A format's own `%message%` still echoes the raw text,
/// which is what the `local` map holds (`ChatService.java:633`).
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
    // §1.12 — `%message%` is a `local` context key: it holds this message's raw
    // text, so a format may echo it before `{message}` substitution.
    let local = [("message", message)];
    let template = placeholder::resolve_with_local(template, player, server_ref, config, &local);
    let message = placeholder::resolve_with_local(message, player, server_ref, config, &local);
    let message = clean_message(&message);
    template
        .replace("{player}", name)
        .replace("{message}", &message)
        .replace("{channel}", channel)
        .replace("{server}", server)
        .replace("{world}", world)
        .replace("{target}", target)
}

/// §3 step 4 — `LegacyText.stripLegacyCodes(message)`: the resolved message with
/// every `&`/`§` code removed, so the body colour comes from the tier and not
/// from the text the player typed. Pure, so the strip can be asserted without a
/// host.
fn clean_message(resolved: &str) -> String {
    functions::strip_legacy_codes(resolved)
}

/// §4.4/§4.5 — re-attaches the selected tier's component-part hover and click
/// events to the rendered message.
///
/// The channel template is flattened to one legacy string, which loses every
/// per-part event. Each configured prefix part becomes a leading child
/// component carrying its own hover/click, so a clickable label such as
/// `[Site]` still opens its URL — matching the upstream tree, where the
/// component parts precede `msg`.
///
/// This is the **only** place the prefix text is produced for public chat: the
/// body template is built by [`crate::config::layer_body_template_with_colour`]
/// and therefore carries no prefix. Flattening the prefix into the body as well
/// would render it twice.
///
/// `local` carries the `%…%` context keys the parts may use — the private path
/// passes `trchat_toplayer` (`ChatService.java:170`), the same key the Mod's
/// `local` map holds.
///
/// Everything is resolved against `subject`, never the viewer: the Mod calls
/// `placeholders.resolve(part.text(), subject, viewer, local)` and its token
/// dispatch reads `player_*` from the **subject**
/// (`PlaceholderResolver.java:101-112`). A viewer-dependent prefix would show
/// every receiver their own name.
fn apply_prefix_events(
    body: TextComponent,
    channel: Option<&ChannelConfig>,
    audience: Audience,
    subject: &Player,
    server: &Server,
    config: &crate::config::TrChatConfig,
    local: &[(&str, &str)],
) -> TextComponent {
    let Some(ch) = channel else {
        return body;
    };
    // §3 step 3 — the tier's prefix groups keep YAML order; within each group
    // only the **first passing variant** renders, so an operator sees the OP
    // badge *or* the default name, never both (`ChannelRenderer.java:155-159`).
    let Some(layer) = select_audience_layer(ch, audience, subject) else {
        return body;
    };
    let parts = group_components(&layer.prefix, subject, server, config, &ch.id, local);
    if parts.is_empty() {
        return body;
    }
    // The message text is already inside `body`; component children append
    // *after* the parent text, so the parts are chained onto a fresh root that
    // sits in front of it — upstream appends the prefix groups first
    // (`ChannelRenderer.java:102`).
    match parts.into_iter().reduce(|acc, part| acc.add_child(part)) {
        Some(parts_c) => parts_c.add_child(body),
        None => body,
    }
}

/// §3 step 5 — the tier's `suffix` groups, appended **after** the message body
/// with the same group/variant rules (`ChannelRenderer.java:144`).
fn apply_suffix_events(
    body: TextComponent,
    channel: Option<&ChannelConfig>,
    audience: Audience,
    subject: &Player,
    server: &Server,
    config: &crate::config::TrChatConfig,
    local: &[(&str, &str)],
) -> TextComponent {
    let Some(ch) = channel else {
        return body;
    };
    let Some(layer) = select_audience_layer(ch, audience, subject) else {
        return body;
    };
    let parts = group_components(&layer.suffix, subject, server, config, &ch.id, local);
    // Children append after the message text, which is exactly where a suffix
    // belongs.
    parts
        .into_iter()
        .fold(body, |acc, part| acc.add_child(part))
}

/// §3 steps 3/5 — resolves the tier's `prefix` / `suffix` groups into component
/// parts: **one variant per group** (the first whose `condition` passes, in YAML
/// order — `ChannelRenderer.java:155-159`), each with its own hover, insertion,
/// font and click event.
fn group_components(
    groups: &[crate::config::PartGroup],
    subject: &Player,
    server: &Server,
    config: &TrChatConfig,
    channel_id: &str,
    local: &[(&str, &str)],
) -> Vec<TextComponent> {
    let name = subject.get_name();
    let world = subject.get_world().get_name();
    let server_name = config.server_name();
    let mut out = Vec::new();
    for part in groups
        .iter()
        .filter_map(|group| group.select(|condition| condition::test(condition, subject)))
    {
        let raw = placeholder::resolve_with_local(&part.text, subject, server, config, local)
            .replace("{player}", &name)
            .replace("{channel}", channel_id)
            .replace("{server}", server_name)
            .replace("{world}", &world);
        let mut c = TextComponent::from_legacy_string_with_code(&raw, '&');
        if !part.hover.is_empty() {
            let hover =
                placeholder::resolve_with_local(&part.hover, subject, server, config, local);
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
        out.push(c);
    }
    out
}

/// §3 step 4 special-char wrapping (`SpecialChars.wrapSpecialChars` /
/// `ChannelRenderer` behavior): configured resource-pack glyphs get the
/// channel's `msg.special-char-color`, with the message default color
/// restored after each glyph run. `special-chars.yml` is loaded process-wide
/// by [`crate::special::reload`]; an empty table or an empty color leaves the
/// body untouched.
fn wrap_special_characters(
    ch: &ChannelConfig,
    audience: Audience,
    player: &Player,
    body: &str,
) -> String {
    let Some(layer) = select_audience_layer(ch, audience, player) else {
        return body.to_string();
    };
    if layer.special_char_color.is_empty() || !special::has_special_chars(body) {
        return body.to_string();
    }
    let color = color_code(&layer.special_char_color);
    let default = color_code(&layer.msg_default_color);
    special::wrap_special_chars(body, &color, &default)
}

/// §3 step 4 — the sender's effective chat colour code, or an empty string.
///
/// The stored colour only takes effect when the sender is an operator or holds
/// the matching `trchat.color.<code>` node, mirroring `ChatService.java:637-643`
/// which writes `trchat_message_color` under that same condition. The private
/// path reuses it: `messageContext` fills that key once and both views of a
/// private message share it (`ChatService.java:169-185`).
pub(crate) fn sender_chat_color(player: &Player) -> String {
    let colour = {
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        session.chat_color(&player.get_name())
    };
    if colour.is_empty() {
        return colour;
    }
    if condition::is_op(player) || player.has_permission(&format!("trchat.color.{colour}")) {
        colour
    } else {
        String::new()
    }
}

/// §3 step 1 — the tier that applies to `player`: the candidates in selection
/// order (`priority` descending, stable) with the first passing `condition`
/// winning. Returns `None` when no tier matches, which the renderer treats as
/// the Mod's plain fallback (§3 step 2).
/// §3 steps 1–2 — the first tier of `audience` whose `condition` passes for
/// `player`, in `priority` order (descending, stable).
///
/// A tier list that yields no match is the upstream `format == null` case: the
/// caller renders the bare resolved message instead (`ChannelRenderer.java:96-99`).
fn select_audience_layer<'a>(
    ch: &'a ChannelConfig,
    audience: Audience,
    player: &Player,
) -> Option<&'a FormatLayer> {
    crate::config::format_candidates(ch.audience_formats(audience))
        .into_iter()
        .find(|layer| condition::test(&layer.condition, player))
}

/// The public `CHAT` tier of a channel — every in-game render walks `Formats`.
fn select_format_layer<'a>(ch: &'a ChannelConfig, player: &Player) -> Option<&'a FormatLayer> {
    select_audience_layer(ch, Audience::Chat, player)
}

/// §3 — one audience render request: everything `render_audience_view` needs to
/// build a single view of a message.
///
/// Bundling these keeps the render call small and mirrors the Mod's
/// `render(channel, audience, subject, viewer, messageComponent, message, local)`
/// (`ChannelRenderer.java:88-100`).
pub(crate) struct AudienceRender<'a> {
    /// The channel to render; `None` means the plain fallback route.
    pub channel: Option<&'a ChannelConfig>,
    pub audience: Audience,
    /// The player the tiers' `condition` and `%player_*%` are evaluated for — the
    /// **sender** for both sides of a private message (`ChatService.java:174-185`).
    pub subject: &'a Player,
    /// The receiving player; their locale localises the function hover text.
    pub viewer: &'a Player,
    /// The raw message text (the `%message%` local and the body slot).
    pub message: &'a str,
    /// The component `functions::process` built when a function matched
    /// (upstream `processed.component()`); it replaces the text body.
    pub processed: Option<&'a crate::functions::FunctionOutcome>,
    /// `%…%` context keys, e.g. `trchat_toplayer` for private messages.
    pub local: &'a [(&'a str, &'a str)],
}

/// §3 — renders one `audience` view of a message into a component: the tier's
/// component parts (their `condition`, hover, click, insertion and font), then
/// the body coloured by `msg.default-color` (or the sender's chat colour), with
/// `msg.hover` on the whole message.
///
/// `None` means the channel is absent or none of its tiers passed, which is the
/// upstream `format == null` case — the caller then falls back to the flattened
/// template (`ChannelRenderer.java:96-99`).
pub(crate) fn render_audience_view(
    req: &AudienceRender<'_>,
    server: &Server,
    config: &TrChatConfig,
) -> Option<TextComponent> {
    let ch = req.channel?;
    let layer = select_audience_layer(ch, req.audience, req.subject)?;
    let name = req.subject.get_name();
    let world = req.subject.get_world().get_name();
    let server_name = config.server_name();
    // `messageContext` fills `trchat_message_color` once and both views of a
    // private message share it (`ChatService.java:169-185`).
    let colour = sender_chat_color(req.subject);
    // The prefix/suffix are *not* part of this template: `apply_prefix_events`
    // and `apply_suffix_events` build them from the tier's parts, which is what
    // keeps their events and conditions.
    let template = crate::config::layer_body_template_with_colour(layer, &colour);
    let component = match req.processed {
        // §3.1 — a caller-supplied component replaces the text body: the
        // template contributes only its formatting (`{message}` stays empty) and
        // the processed body is appended as a child, exactly like the Mod's
        // `Component.empty().withStyle(applyLegacyFormat(...)).append(component)`
        // (`ChannelRenderer.java:127-133`).
        Some(out) => {
            let empty = render_template(
                &template,
                &name,
                "",
                &ch.id,
                server_name,
                &world,
                local_target(req.local),
                req.subject,
                server,
                config,
            );
            let locale = req.viewer.get_locale();
            crate::functions::build_body_component(
                &empty,
                out,
                &name,
                &locale,
                req.subject,
                server,
                config,
            )
        }
        None => {
            let text = render_template(
                &template,
                &name,
                req.message,
                &ch.id,
                server_name,
                &world,
                local_target(req.local),
                req.subject,
                server,
                config,
            );
            // §3 step 4 — the resource-pack wrap applies to the cleaned body and
            // only when no component body was supplied (`:118-126`).
            let text = wrap_special_characters(ch, req.audience, req.subject, &text);
            TextComponent::from_legacy_string_with_code(&text, '&')
        }
    };
    let component = apply_prefix_events(
        component,
        req.channel,
        req.audience,
        req.subject,
        server,
        config,
        req.local,
    );
    // §3 step 5 — suffix groups append after the body (`:144`).
    let component = apply_suffix_events(
        component,
        req.channel,
        req.audience,
        req.subject,
        server,
        config,
        req.local,
    );
    Some(apply_msg_hover(
        component,
        req.channel,
        req.audience,
        req.subject,
        server,
        config,
    ))
}

/// The `trchat_toplayer` value of a `local` context — the exact target name of a
/// private message (`ChatService.java:170`) — or `""` when absent.
pub(crate) fn local_target<'a>(local: &[(&'a str, &'a str)]) -> &'a str {
    local
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("trchat_toplayer"))
        .map(|(_, value)| *value)
        .unwrap_or("")
}

/// §3 step 4 — a non-empty `msg.hover` puts `HoverEvent.ShowText` on the message
/// body. This renderer produces a single component for the whole line, so the
/// hover lands on that component; an empty value leaves it untouched.
fn apply_msg_hover(
    component: TextComponent,
    channel: Option<&ChannelConfig>,
    audience: Audience,
    player: &Player,
    server: &Server,
    config: &TrChatConfig,
) -> TextComponent {
    let Some(layer) = channel.and_then(|ch| select_audience_layer(ch, audience, player)) else {
        return component;
    };
    if layer.msg_hover.trim().is_empty() {
        return component;
    }
    let hover = placeholder::resolve(&layer.msg_hover, player, server, config);
    component.hover_show_text(TextComponent::from_legacy_string_with_code(&hover, '&'))
}

/// Emits a localised guard hint and reports the rejection. The per-player
/// state lock must already be released: this touches the language store and
/// makes a host call.
fn reject_with(player: &Player, locale: &str, key: &str, args: &[&str]) -> ChatOutcome {
    let text = lang::lang()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .format(key, locale, args);
    player.send_system_message(
        TextComponent::from_legacy_string_with_code(&text, '&'),
        false,
    );
    ChatOutcome::Rejected
}

/// §1.4 note: a zero period falls back to 60 000 ms at runtime. The config
/// default is already 60 000, so only an explicit `0` reaches the fallback —
/// unlike `antiRepeatMaxPerPeriod`, where `0` is meaningful rather than "off".
fn period_or_default(millis: u64) -> u128 {
    if millis == 0 {
        60_000
    } else {
        millis as u128
    }
}

/// §5 `maxConsecutiveRepeat(message, whitelist)` (`MessageGuard.java:80-119`).
///
/// Returns the largest number of times any substring repeats back-to-back; a
/// message with no repetition scores `1`, as does anything shorter than two
/// characters. Comparison is case-sensitive (`regionMatches` semantics).
fn max_consecutive_repeat(message: &str, whitelist: &[String]) -> usize {
    let chars: Vec<char> = message.chars().collect();
    let n = chars.len();
    if n < 2 {
        return 1;
    }
    let mut max = 1;
    for i in 0..n {
        // Pruning 1: even one char per repeat cannot beat the current best.
        if n - i <= max {
            break;
        }
        for len in 1..=(n - i) / 2 {
            // Pruning 2: the remaining run cannot hold more than `max` units.
            if (n - i) / len <= max {
                break;
            }
            // A whitelisted unit is skipped entirely rather than counted.
            if is_whitelisted_unit(&chars, i, len, whitelist) {
                continue;
            }
            let unit = &chars[i..i + len];
            let mut count = 1;
            let mut pos = i + len;
            while pos + len <= n && chars[pos..pos + len] == *unit {
                count += 1;
                pos += len;
            }
            if count > max {
                max = count;
            }
        }
    }
    max
}

/// §5 `isWhitelistedUnit(message, start, len, whitelist)` (`:126-147`): the
/// `len`-long unit is whitelisted when it equals some whitelist phrase repeated
/// a whole number of times (e.g. `哈哈` against the default `哈` entry).
fn is_whitelisted_unit(chars: &[char], start: usize, len: usize, whitelist: &[String]) -> bool {
    let unit = &chars[start..start + len];
    whitelist.iter().any(|phrase| {
        let phrase: Vec<char> = phrase.chars().collect();
        if phrase.is_empty() || len % phrase.len() != 0 {
            return false;
        }
        unit.chunks(phrase.len())
            .all(|chunk| chunk == phrase.as_slice())
    })
}

/// §5 `similarity(left, right)` (`MessageGuard.java:23-34`).
///
/// Both sides are normalised first (`toLowerCase(ROOT)` + **all** whitespace
/// removed); identical results score `1.0` (which also covers two empty
/// strings), otherwise the score is
/// `1 - levenshtein(a, b) / max(len(a), len(b))`.
///
/// The Mod uses a rolling-array Levenshtein with unit insert/delete/substitute
/// costs (`:49-66`). Comparison is per `char`; Java's `char` is a UTF-16 code
/// unit, so the two differ only for astral-plane characters (emoji).
fn similarity_score(left: &str, right: &str) -> f64 {
    let a = normalize_for_similarity(left);
    let b = normalize_for_similarity(right);
    if a == b {
        return 1.0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let longest = a.len().max(b.len());
    if longest == 0 {
        // Both sides normalised to nothing; equal-handling above covers this,
        // but guard the division in case that branch ever changes.
        return 1.0;
    }
    1.0 - (levenshtein(&a, &b) as f64 / longest as f64)
}

/// `toLowerCase(ROOT)` + `\s+` removal — the anti-repeat normalisation.
/// `to_lowercase` can expand one char into several (`İ` → `i̇`), so the
/// lowercase step is flattened rather than mapped 1:1.
fn normalize_for_similarity(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Rolling-array Levenshtein distance; insert, delete and substitute all cost 1.
fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    // `prev[j]` is the distance for `a[..i]` vs `b[..j]`.
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitute = prev[j] + usize::from(ca != cb);
            let delete = prev[j + 1] + 1;
            let insert = cur[j] + 1;
            cur[j + 1] = substitute.min(delete).min(insert);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_console_text, chat_log_line, clean_message, is_whitelisted_unit, levenshtein,
        local_target, max_consecutive_repeat, normalize_for_similarity, period_or_default,
        similarity_score,
    };
    use crate::config::TrChatConfig;

    /// §1.6 — `{0}` is the clock, `{1}` the sender, `{2}` the message; the
    /// private form pushes the target into `{2}` and the message into `{3}`.
    #[test]
    fn console_log_lines_follow_the_configured_formats() {
        let mut config = TrChatConfig::default();
        config.settings.logging.normal_message_format = "[{0}] {1}: {2}".to_string();
        config.settings.logging.private_message_format = "[{0}] {1} -> {2}: {3}".to_string();

        let normal = chat_log_line(&config, "Alice", None, "hello");
        assert!(normal.ends_with("] Alice: hello"), "{normal}");
        // `{0}` is `HH:mm:ss` — `[HH:mm:ss] Alice: hello`.
        assert!(normal.starts_with('['), "{normal}");
        assert_eq!(&normal[3..4], ":", "{normal}");
        assert_eq!(&normal[6..7], ":", "{normal}");
        assert_eq!(&normal[9..10], "]", "{normal}");

        let private = chat_log_line(&config, "Alice", Some("Bob"), "psst");
        assert!(private.ends_with("] Alice -> Bob: psst"), "{private}");
    }

    /// `ChatLogService.safe()` — one line per record, so newlines never leak.
    #[test]
    fn console_log_lines_flatten_newlines() {
        let config = TrChatConfig::default();
        let line = chat_log_line(&config, "Alice", None, "a\nb\rc");
        assert!(!line.contains('\n') && !line.contains('\r'), "{line}");
        assert!(line.contains("a b c"), "{line}");
    }

    /// An omitted `logging.*MessageFormat` must not log a blank line.
    #[test]
    fn empty_log_formats_fall_back_to_the_upstream_defaults() {
        let config = TrChatConfig::default();
        assert!(config.settings.logging.normal_message_format.is_empty());

        let normal = chat_log_line(&config, "Alice", None, "hello");
        assert!(normal.ends_with("] Alice: hello"), "{normal}");

        let private = chat_log_line(&config, "Alice", Some("Bob"), "psst");
        assert!(private.ends_with("] Alice -> Bob: psst"), "{private}");
    }

    /// §1.6 — `component().getString()` drops every legacy code, so the console
    /// line is plain text: the tier's prefix groups, the body, then its suffix
    /// groups (`ChannelRenderer.java:102-144`).
    #[test]
    fn console_text_is_plain_and_ordered() {
        let prefix = vec!["&8[&aServer&8] ".to_string(), "&7".to_string()];
        let suffix = vec![" &8(&f1.20&8)".to_string()];
        let line = assemble_console_text(&prefix, "&fhello &cthere", &suffix);
        assert_eq!(line, "[Server] hello there (1.20)");

        // No tier, no prefix and no suffix: the bare resolved message survives.
        assert_eq!(assemble_console_text(&[], "plain", &[]), "plain");
        // A suffix without a prefix still trails the message.
        assert_eq!(
            assemble_console_text(&[], "hi", &[" &7- bye".to_string()]),
            "hi - bye"
        );
    }

    /// §1.6 — a private message's `{target}` comes from the `trchat_toplayer`
    /// local key, and a context without the key leaves it empty.
    #[test]
    fn local_target_reads_the_toplayer_key() {
        let local = [("message", "hi"), ("trchat_toplayer", "Bob")];
        assert_eq!(local_target(&local), "Bob");
        // The key is matched case-insensitively, like every other local key.
        assert_eq!(local_target(&[("TRCHAT_TOPLAYER", "Carol")]), "Carol");
        assert_eq!(local_target(&[("message", "hi")]), "");
        assert_eq!(local_target(&[]), "");
    }

    /// §3 step 4 — the `{message}` slot drops legacy codes, so only the tier's
    /// colour (or the sender's picked `trchat.color`) decides how a message
    /// looks; a player cannot colour their own text with `&c`.
    #[test]
    fn message_body_drops_legacy_codes() {
        assert_eq!(clean_message("&chello &lworld"), "hello world");
        assert_eq!(clean_message("plain text"), "plain text");
        // A trailing lone symbol survives, matching the Mod's scanner.
        assert_eq!(clean_message("x&"), "x&");
        assert_eq!(clean_message("&c"), "");
    }

    /// Normalisation is `toLowerCase(ROOT)` plus removal of *all* whitespace.
    #[test]
    fn similarity_normalisation_folds_case_and_whitespace() {
        assert_eq!(normalize_for_similarity("Hello World"), "helloworld");
        assert_eq!(normalize_for_similarity("  A\tB\nC  "), "abc");
        assert_eq!(normalize_for_similarity(""), "");

        // Identical after normalisation → 1.0.
        assert_eq!(similarity_score("Hello World", "helloworld"), 1.0);
        assert_eq!(similarity_score("HELLO", "hello"), 1.0);
    }

    /// The score is `1 - levenshtein / max(len)` on the normalised text.
    #[test]
    fn similarity_scores_match_normalised_levenshtein() {
        // Identical → 1.0, and two empty strings are documented as 1.0.
        assert_eq!(similarity_score("hello", "hello"), 1.0);
        assert_eq!(similarity_score("", ""), 1.0);

        // One empty side → 0.0 (levenshtein == the other length).
        assert_eq!(similarity_score("", "abc"), 0.0);
        assert_eq!(similarity_score("abc", ""), 0.0);

        // hello/world: 4 substitutions over max length 5 → 0.2.
        assert!((similarity_score("hello", "world") - 0.2).abs() < 1e-9);

        // hello/helloo: one insertion over max length 6 → 5/6.
        let one_insert = 1.0 - 1.0 / 6.0;
        assert!((similarity_score("hello", "helloo") - one_insert).abs() < 1e-9);

        // abc/abd: one substitution over 3 → 2/3.
        assert!((similarity_score("abc", "abd") - 2.0 / 3.0).abs() < 1e-9);

        // kitten/sitting: 3 edits over 7 → 4/7.
        assert!((similarity_score("kitten", "sitting") - 4.0 / 7.0).abs() < 1e-9);

        // flaw/lawn: 2 edits over 4 → 0.5.
        assert!((similarity_score("flaw", "lawn") - 0.5).abs() < 1e-9);
    }

    /// The default `antiRepeatSimilarity` of 0.85 must separate a repeated
    /// message from an unrelated one.
    #[test]
    fn similarity_threshold_separates_repeats_from_unrelated_text() {
        const THRESHOLD: f64 = 0.85;
        assert!(similarity_score("hello there", "hello there") >= THRESHOLD);
        // A single extra character keeps a near-duplicate above the threshold.
        assert!(similarity_score("hello there", "hello theree") >= THRESHOLD);
        // A genuinely different sentence falls well below it.
        assert!(similarity_score("hello there", "goodbye world") < THRESHOLD);
    }

    /// Rolling-array Levenshtein: unit costs, and correct on the degenerate
    /// axes that the rolling buffer could get wrong.
    #[test]
    fn levenshtein_uses_unit_costs() {
        let chars = |s: &str| s.chars().collect::<Vec<char>>();
        assert_eq!(levenshtein(&chars(""), &chars("")), 0);
        assert_eq!(levenshtein(&chars(""), &chars("abc")), 3);
        assert_eq!(levenshtein(&chars("abc"), &chars("")), 3);
        assert_eq!(levenshtein(&chars("abc"), &chars("abc")), 0);
        // Pure insertion / deletion / substitution.
        assert_eq!(levenshtein(&chars("abc"), &chars("abcd")), 1);
        assert_eq!(levenshtein(&chars("abcd"), &chars("abc")), 1);
        assert_eq!(levenshtein(&chars("abc"), &chars("axc")), 1);
        // Symmetry on a longer pair.
        assert_eq!(
            levenshtein(&chars("kitten"), &chars("sitting")),
            levenshtein(&chars("sitting"), &chars("kitten"))
        );
    }

    /// §5 `maxConsecutiveRepeat`: the largest back-to-back repeat of *any*
    /// substring, with no repetition (or a message under two chars) scoring 1.
    #[test]
    fn consecutive_repeat_counts_any_repeated_substring() {
        let none: Vec<String> = Vec::new();

        // Degenerate inputs are documented as 1, not 0.
        assert_eq!(max_consecutive_repeat("", &none), 1);
        assert_eq!(max_consecutive_repeat("a", &none), 1);
        // No repetition at all.
        assert_eq!(max_consecutive_repeat("abc", &none), 1);
        // A single repeated character.
        assert_eq!(max_consecutive_repeat("aaa", &none), 3);
        // A repeated multi-character unit beats the single-char run.
        assert_eq!(max_consecutive_repeat("abab", &none), 2);
        assert_eq!(max_consecutive_repeat("ababab", &none), 3);
        // The run need not start at the beginning.
        assert_eq!(max_consecutive_repeat("xyzzzz", &none), 4);
        // CJK text behaves the same way.
        assert_eq!(max_consecutive_repeat("你好你好", &none), 2);
    }

    /// §5 `isWhitelistedUnit`: a unit is ignored when it is a whitelist phrase
    /// repeated a whole number of times. The default entries cover chat filler
    /// (`哈`, `6`, `?`, `！`, …).
    #[test]
    fn whitelisted_units_are_skipped_by_the_repeat_count() {
        let ha = vec!["哈".to_string()];
        let exclamations = vec!["!".to_string(), "！".to_string()];

        // Without a whitelist the run counts in full.
        assert_eq!(max_consecutive_repeat("哈哈哈", &[]), 3);
        assert_eq!(max_consecutive_repeat("!!!!", &[]), 4);

        // With the filler whitelisted it collapses to "no repetition".
        assert_eq!(max_consecutive_repeat("哈哈哈", &ha), 1);
        assert_eq!(max_consecutive_repeat("哈哈哈哈哈", &ha), 1);
        assert_eq!(max_consecutive_repeat("!!!!", &exclamations), 1);
        // Mixed filler still collapses when each unit is whitelisted.
        assert_eq!(max_consecutive_repeat("!!！！", &exclamations), 1);

        // The comparison is case-sensitive (`regionMatches` semantics), so a
        // differently-cased phrase is *not* whitelisted.
        let upper = vec!["A".to_string()];
        assert_eq!(max_consecutive_repeat("aaaa", &upper), 4);

        // A non-whitelisted repeat next to a whitelisted one still counts.
        assert_eq!(max_consecutive_repeat("哈哈哈xyzxyz", &ha), 2);
    }

    /// `isWhitelistedUnit` only accepts whole repetitions of a phrase.
    #[test]
    fn whitelist_matching_requires_whole_repetitions() {
        let chars = |s: &str| s.chars().collect::<Vec<char>>();
        let ha = vec!["哈".to_string()];
        // `哈哈` is `哈` twice → whitelisted; `哈哈啥` is not a whole multiple.
        assert!(is_whitelisted_unit(&chars("哈哈哈"), 0, 2, &ha));
        assert!(!is_whitelisted_unit(&chars("哈哈啥"), 0, 3, &ha));
        // An empty whitelist never matches, and an empty phrase is ignored.
        assert!(!is_whitelisted_unit(&chars("aaa"), 0, 2, &[]));
        assert!(!is_whitelisted_unit(&chars("aaa"), 0, 2, &[String::new()]));
    }

    /// §1.4 note: only an explicit `0` period falls back to 60 000 ms.
    #[test]
    fn zero_period_falls_back_to_one_minute() {
        assert_eq!(period_or_default(0), 60_000);
        assert_eq!(period_or_default(1), 1);
        assert_eq!(period_or_default(60_000), 60_000);
    }
}
