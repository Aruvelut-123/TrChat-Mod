//! Private messages (§1.6) — `/trreply` target tracking, the two audience views
//! `/msg` delivers, and the private-message spy.
//!
//! `sendPrivate` renders the message twice against the `Private` channel: the
//! `Audience.Sender` tier for the sender and `Audience.Receiver` for the target,
//! both with the sender as the placeholder *subject* and the target name in the
//! `trchat_toplayer` local key (`ChatService.java:174-185`). The side effects
//! that live here are correspondent tracking and the spy echo.

use pumpkin_plugin_api::player::Player;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::Server;

use crate::config::Audience;
use crate::lang;
use crate::playerdata::SessionPlayers;

/// Resolves the target of `/trreply` for `name`, or `None` when they have not
/// been messaged yet (spec: `Private-Message-No-Reply`).
pub fn reply_target(name: &str) -> Option<String> {
    let session = SessionPlayers::global();
    let session = session.read().unwrap_or_else(|e| e.into_inner());
    let state = session.state(name)?;
    if state.last_private_sender.is_empty() {
        None
    } else {
        Some(state.last_private_sender.clone())
    }
}

/// Records `from` as `to`'s most recent private correspondent.
pub fn remember_correspondent(to: &str, from: &str) {
    let session = SessionPlayers::global();
    let mut session = session.write().unwrap_or_else(|e| e.into_inner());
    if let Some(state) = session.state_mut(to) {
        state.last_private_sender = from.to_ascii_lowercase();
    }
}

/// Whether `name` has private-message spy enabled.
pub fn is_spying(name: &str) -> bool {
    let session = SessionPlayers::global();
    let session = session.read().unwrap_or_else(|e| e.into_inner());
    session.state(name).is_some_and(|s| s.private_spy)
}

/// Toggles spy for `name` and returns the new state.
pub fn toggle_spy(name: &str) -> bool {
    let session = SessionPlayers::global();
    let mut session = session.write().unwrap_or_else(|e| e.into_inner());
    match session.state_mut(name) {
        Some(state) => {
            state.private_spy = !state.private_spy;
            state.private_spy
        }
        None => false,
    }
}

/// What [`send_private`] did — the hint the caller reports, if any.
///
/// `sendPrivate` answers with a plain `int`, but each failure carries its own
/// language key, so the port names the outcome instead
/// (`ChatService.java:151-234`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateOutcome {
    /// The receiver got their copy — locally, or through the Redis relay.
    Delivered,
    /// The receiver ignores the sender: the sender still saw their own copy and
    /// the spies still saw the conversation, but the receiver did not.
    Ignored,
    /// Neither a local player nor a cross-server remote player has that name
    /// (`General-Player-Not-Found`).
    NotFound,
    /// The receiver is on another server and no cross-server transport accepted the message
    /// (`Redis-Private-Unavailable`).
    RedisUnavailable,
    /// A cross-server message displayed an item another server cannot resolve,
    /// so it was refused before anything was sent (`Redis-Unsafe-Item`).
    UnsafeItem,
}

/// `ChatService.sendPrivate` — delivers one private message to a local or a
/// remote receiver, rendering both sides and running the `sendPrivate` side
/// effects (correspondent tracking, mention alert, spy echo).
///
/// Shared by `/msg`, `/trreply` and the private-channel aliases. `target_name`
/// is the spelling the sender typed: the exact account name is resolved from
/// the local player list first and then from the Redis `UpdateNames` snapshots,
/// from the cross-server `UpdateNames` snapshots, which is how a message reaches
/// a player on another server (`ChatService.java:162-167`).
pub fn send_private(
    server: &Server,
    sender: &Player,
    target_name: &str,
    message: &str,
) -> PrivateOutcome {
    let sender_name = sender.get_name();
    let local_target = server.get_player_by_name(target_name);
    let exact_target = match &local_target {
        Some(target) => target.get_name(),
        None => match crate::redis::exact_remote_name(target_name) {
            Some(name) => name,
            None => return PrivateOutcome::NotFound,
        },
    };

    // §2.5 — a shadow-muted sender is not blocked by a guard: the message is
    // instead split here, exactly as `sendPublic` splits it at step 6. Only the
    // sender sees it and only the log records it, so the receiver and the spies
    // are skipped (`ChatService.java:186-196`).
    let shadow_muted = {
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        session.is_shadow_muted(&sender_name)
    };

    // §3 — the two views of one private message. Both are evaluated for the
    // *sender* as subject: `Audience.Sender` for the sender's own copy and
    // `Audience.Receiver` for the target's, with `trchat_toplayer` filled with
    // the exact target name. The flattened templates are only the fallback for a
    // missing `Private` channel or a tier that yields no match
    // (`ChatService.java:174-185`, `ChannelRenderer.java:96-99`).
    let target_key = exact_target.to_ascii_lowercase();
    let (sender_view, receiver_view, processed, mention_target, sender_tpl, receiver_tpl) = {
        let config = crate::config::global_config();
        let config = config.read();
        let channel = config.private_channel();
        // §1.3 step 3 — the function pass runs for private messages too, gated by
        // the `Private` channel's `Disabled-Functions` (`ChatService.java:171-173`;
        // the shipped config disables `Mention` there, so only the item/snapshot
        // functions fire).
        let disabled: &[String] = channel
            .map(|c| c.options.disabled_functions.as_slice())
            .unwrap_or(&[]);
        let processed = crate::functions::process(server, sender, message, &config, disabled);
        // §1.3 step 9b — the target is notified when the message mentions them
        // (`ChatService.java:203-205`).
        let mention_target = processed
            .as_ref()
            .is_some_and(|out| out.mentioned.contains(&target_key));
        let local = [("trchat_toplayer", exact_target.as_str())];
        let processed_ref = processed.as_ref();
        let sender_view = crate::chat::render_audience_view(
            &crate::chat::AudienceRender {
                channel,
                audience: Audience::Sender,
                subject: sender,
                viewer: sender,
                message,
                processed: processed_ref,
                local: &local,
            },
            server,
            &config,
        );
        // A remote receiver has no `Player` handle here, so the sender stands in
        // as the viewer. That is what the Mod does in effect: its
        // `PlaceholderResolver.resolve` never reads the viewer, and the only
        // thing this port takes from it is the locale used for the function
        // hovers — which the Mod also localises with the *sender*
        // (`ChatService.java:181`, `ChatFunctionService.java:342`).
        let receiver_view = crate::chat::render_audience_view(
            &crate::chat::AudienceRender {
                channel,
                audience: Audience::Receiver,
                subject: sender,
                viewer: local_target.as_ref().unwrap_or(sender),
                message,
                processed: processed_ref,
                local: &local,
            },
            server,
            &config,
        );
        (
            sender_view,
            receiver_view,
            processed,
            mention_target,
            config.msg.sender.clone(),
            config.msg.receiver.clone(),
        )
    };

    // §2.8 — a cross-server hop must be able to resolve every displayed item, so
    // the message is refused before the sender even sees it
    // (`ChatService.java:187-190`). A local delivery never takes this branch.
    let cross_server_safe = processed.as_ref().is_none_or(|out| out.cross_server_safe);
    if !shadow_muted && local_target.is_none() && !cross_server_safe {
        return PrivateOutcome::UnsafeItem;
    }

    // The sender's copy: their audience view, else the flattened template.
    // `ChatService.java:191` — it goes out before the receiver's copy, so an
    // ignored message still shows the sender what they typed.
    if let Some(component) = sender_view {
        sender.send_system_message(component, false);
    } else if !sender_tpl.is_empty() {
        let text = render_msg(&sender_tpl, &sender_name, &exact_target, message);
        let component = TextComponent::from_legacy_string_with_code(&text, '&');
        sender.send_system_message(component, false);
    }

    if shadow_muted {
        // No correspondent is remembered either: from the receiver's point of
        // view the conversation never happened, so `/reply` must not find it.
        log_private(server, sender, &exact_target, message);
        return PrivateOutcome::Delivered;
    }

    // §1.6 local delivery (`ChatService.java:198-212`) — the receiver may still
    // ignore the sender, in which case only the spy echo and the log happen.
    if let Some(target) = local_target {
        let delivered_name = target.get_name();
        let ignored = ignores(&delivered_name, &sender_name);
        if !ignored {
            // The target's copy: the `Receiver` view, whose subject is still the
            // sender, so both sides read "sender ➥ target".
            if let Some(component) = receiver_view {
                target.send_system_message(component, false);
            } else if !receiver_tpl.is_empty() {
                let text = render_msg(&receiver_tpl, &sender_name, &exact_target, message);
                let component = TextComponent::from_legacy_string_with_code(&text, '&');
                target.send_system_message(component, false);
            }
            // §1.3 step 9b — a mentioned target gets the mention notification, but
            // only once the message really reached them (`ChatService.java:203-205`).
            if mention_target {
                crate::functions::notify_mentioned(&target, &sender_name, &target.get_locale());
            }
            remember_correspondent(&delivered_name, &sender_name);
        }
        notify_spies(server, sender, &target, message);
        log_private(server, sender, &exact_target, message);
        return if ignored {
            PrivateOutcome::Ignored
        } else {
            PrivateOutcome::Delivered
        };
    }

    // §1.6 cross-server relay (`ChatService.java:214-233`) — a `Private` channel
    // without `Proxy` cannot relay, and neither can one whose publish fails;
    // both report `Redis-Private-Unavailable`.
    let (receiver_json, receiver_fallback) = match receiver_view {
        Some(component) => {
            let fallback = component.get_text();
            (component.to_json(), fallback)
        }
        None => {
            let text = if receiver_tpl.is_empty() {
                message.to_string()
            } else {
                render_msg(&receiver_tpl, &sender_name, &exact_target, message)
            };
            let component = TextComponent::from_legacy_string_with_code(&text, '&');
            let fallback = component.get_text();
            (component.to_json(), fallback)
        }
    };
    // Field 5 is the spy view. The Mod serialises the processed component; only
    // its plain text is ever read back (`receivePrivate`), so the processed body
    // is what travels.
    let processed_body = processed.as_ref().map_or(message, |out| out.body.as_str());
    let message_json = TextComponent::text(processed_body).to_json();
    let published = crate::redis::publish_private(
        &exact_target,
        &sender_name,
        &receiver_json,
        &receiver_fallback,
        &message_json,
    ) || crate::proxy::publish_private(
        server,
        &exact_target,
        &sender_name,
        &receiver_json,
        &receiver_fallback,
        &message_json,
    );
    if !published {
        return PrivateOutcome::RedisUnavailable;
    }
    if mention_target {
        let _ = crate::redis::publish_send_lang(
            &exact_target,
            "Function-Mention-Notify",
            &[&sender_name],
        ) || crate::proxy::publish_send_lang(
            server,
            &exact_target,
            "Function-Mention-Notify",
            &[&sender_name],
        );
    }
    log_private(server, sender, &exact_target, message);
    PrivateOutcome::Delivered
}

/// §1.6 — records one private message in the console log
/// (`ChatService.logToConsole` on the `Private` channel).
fn log_private(server: &Server, sender: &Player, target_name: &str, message: &str) {
    let config = crate::config::global_config();
    let config = config.read();
    crate::chat::log_private_message(&config, sender, server, target_name, message);
}

/// Renders the **fallback** private-message template (`{player}`, `{target}`,
/// `{message}`) — the flattened `Private` templates derived at config load.
///
/// Only used when the `Private` channel is missing or none of its `Sender` /
/// `Receiver` tiers passes, in which case the audience renderer returns `None`.
fn render_msg(template: &str, from: &str, to: &str, text: &str) -> String {
    template
        .replace("{player}", from)
        .replace("{target}", to)
        .replace("{message}", text)
}

/// Whether `observer` ignores `other` (spec §2.7) — the `ModerationService`
/// check shared by `/msg` and the cross-server receive path, where the sender
/// only exists as a name.
pub fn ignores(observer: &str, other: &str) -> bool {
    let session = SessionPlayers::global();
    let session = session.read().unwrap_or_else(|e| e.into_inner());
    session.ignores(observer, other)
}

/// Delivers the spy echo to every player with spy enabled, excluding the two
/// participants (they already saw the conversation themselves).
pub fn notify_spies(server: &Server, sender: &Player, target: &Player, message: &str) {
    notify_spies_by_name(server, &sender.get_name(), &target.get_name(), message);
}

/// [`notify_spies`] for a pair named by string — the shape the Redis receive
/// path needs, where the sender is on another server and the target may already
/// have left (`ChatService.notifyPrivateSpies`).
pub fn notify_spies_by_name(server: &Server, sender_name: &str, target_name: &str, message: &str) {
    for player in server.get_all_players() {
        let name = player.get_name();
        if name.eq_ignore_ascii_case(sender_name) || name.eq_ignore_ascii_case(target_name) {
            continue;
        }
        if !is_spying(&name) {
            continue;
        }
        // The spy line is localised rather than config-templated (spec §2.6).
        let locale = player.get_locale();
        let template = lang::lang()
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("Private-Message-Spy-Format", &locale)
            .to_string();
        if template.is_empty() {
            continue;
        }
        let text = template
            .replace("{0}", sender_name)
            .replace("{1}", target_name)
            .replace("{2}", message);
        let component = TextComponent::from_legacy_string_with_code(&text, '&');
        player.send_system_message(component, false);
    }
}

/// Reports the current spy state back to the player (spec: `Private-Message-
/// Spy-On` / `-Off`, sent only to the player who toggled it).
pub fn announce_spy(player: &Player, enabled: bool) {
    let key = if enabled {
        "Private-Message-Spy-On"
    } else {
        "Private-Message-Spy-Off"
    };
    let text = lang::lang()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(key, &player.get_locale())
        .to_string();
    if !text.is_empty() {
        let component = TextComponent::from_legacy_string_with_code(&text, '&');
        player.send_system_message(component, false);
    }
}

/// Renders the "no reply target" hint (spec: `Private-Message-No-Reply`).
pub fn no_reply_hint(player: &Player) {
    let text = lang::lang()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get("Private-Message-No-Reply", &player.get_locale())
        .to_string();
    if !text.is_empty() {
        let component = TextComponent::from_legacy_string_with_code(&text, '&');
        player.send_system_message(component, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec §1.6 — `/trreply` targets the last correspondent, and `remember`
    /// stores the name lowercased so lookups are case-insensitive.
    #[test]
    fn correspondent_tracking_drives_reply_target() {
        let session = SessionPlayers::global();
        {
            let mut s = session.write().unwrap_or_else(|e| e.into_inner());
            s.join("Alice", "Normal");
            s.join("Bob", "Normal");
        }

        // Nobody has messaged Alice yet.
        assert_eq!(reply_target("Alice"), None);

        remember_correspondent("Alice", "Bob");
        assert_eq!(reply_target("Alice").as_deref(), Some("bob"));

        // The reply itself makes Alice Bob's correspondent (both ends track).
        remember_correspondent("Bob", "Alice");
        assert_eq!(reply_target("Bob").as_deref(), Some("alice"));

        // A newer correspondent replaces the older one.
        remember_correspondent("Alice", "Carol");
        assert_eq!(reply_target("Alice").as_deref(), Some("carol"));

        // Unknown players have no state and therefore no reply target.
        assert_eq!(reply_target("Nobody"), None);
    }

    /// Spec §2.6 — spy toggles per player and is independent between players.
    #[test]
    fn spy_toggles_per_player() {
        let session = SessionPlayers::global();
        {
            let mut s = session.write().unwrap_or_else(|e| e.into_inner());
            s.join("Watcher", "Normal");
            s.join("Other", "Normal");
        }

        assert!(!is_spying("Watcher"));
        assert!(toggle_spy("Watcher"), "first toggle enables spy");
        assert!(is_spying("Watcher"));
        // The other player is unaffected.
        assert!(!is_spying("Other"));

        assert!(!toggle_spy("Watcher"), "second toggle disables spy");
        assert!(!is_spying("Watcher"));

        // An offline/unknown player cannot be spying.
        assert!(!is_spying("Nobody"));
        assert!(!toggle_spy("Nobody"));
    }

    /// The fallback renderer walks the three placeholders in one literal pass.
    ///
    /// Both copies pass `(sender, target)`: the Mod renders a private message
    /// with the sender as subject for *both* sides, so the two copies differ by
    /// the template (`Sender` / `Receiver` tier), not by swapped arguments.
    #[test]
    fn render_msg_fills_all_three_placeholders() {
        let tpl = "&6{player} &7-> &3{target}&f: &7{message}";
        // Sender's copy.
        assert_eq!(
            render_msg(tpl, "Alice", "Bob", "hi"),
            "&6Alice &7-> &3Bob&f: &7hi"
        );
        // Receiver's copy: same arguments, the `Receiver` template instead.
        let receiver_tpl = "&6{player} &7<- &3{target}&f: &7{message}";
        assert_eq!(
            render_msg(receiver_tpl, "Alice", "Bob", "hi"),
            "&6Alice &7<- &3Bob&f: &7hi"
        );
    }
}
