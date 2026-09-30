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

/// Delivers one private message, rendering both sides and running the
/// `sendPrivate` side effects (correspondent tracking + spy echo).
///
/// Shared by `/msg` and `/trreply` so both behave identically. Returns `false`
/// when the receiver ignores the sender, in which case nothing is delivered.
pub fn deliver(server: &Server, sender: &Player, target: &Player, message: &str) -> bool {
    let sender_name = sender.get_name();
    let target_name = target.get_name();

    // The receiver ignores the sender → the message is swallowed (spec §2.7).
    {
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        if session.ignores(&target_name, &sender_name) {
            return false;
        }
    }

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
    let (sender_view, receiver_view, sender_tpl, receiver_tpl) = {
        let config = crate::config::global_config();
        let config = config.read();
        let channel = config.private_channel();
        let local = [("trchat_toplayer", target_name.as_str())];
        (
            crate::chat::render_audience_view(
                channel,
                Audience::Sender,
                sender,
                server,
                &config,
                message,
                &local,
            ),
            crate::chat::render_audience_view(
                channel,
                Audience::Receiver,
                sender,
                server,
                &config,
                message,
                &local,
            ),
            config.msg.sender.clone(),
            config.msg.receiver.clone(),
        )
    };

    // The sender's copy: their audience view, else the flattened template.
    if let Some(component) = sender_view {
        sender.send_system_message(component, false);
    } else if !sender_tpl.is_empty() {
        let text = render_msg(&sender_tpl, &sender_name, &target_name, message);
        let component = TextComponent::from_legacy_string_with_code(&text, '&');
        sender.send_system_message(component, false);
    }

    if shadow_muted {
        // No correspondent is remembered either: from the receiver's point of
        // view the conversation never happened, so `/reply` must not find it.
        let config = crate::config::global_config();
        let config = config.read();
        crate::chat::log_private_message(&config, sender, server, &target_name, message);
        return true;
    }

    // The target's copy: the `Receiver` view, whose subject is still the sender,
    // so both sides read "sender ➥ target".
    if let Some(component) = receiver_view {
        target.send_system_message(component, false);
    } else if !receiver_tpl.is_empty() {
        let text = render_msg(&receiver_tpl, &sender_name, &target_name, message);
        let component = TextComponent::from_legacy_string_with_code(&text, '&');
        target.send_system_message(component, false);
    }

    remember_correspondent(&target_name, &sender_name);
    notify_spies(server, sender, target, message);
    // §1.6 — the console records private messages through `logPrivate` plus the
    // rendered `Console` (or `Formats`) view of the Private channel.
    {
        let config = crate::config::global_config();
        let config = config.read();
        crate::chat::log_private_message(&config, sender, server, &target_name, message);
    }
    true
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

/// Delivers the spy echo to every player with spy enabled, excluding the two
/// participants (they already saw the conversation themselves).
pub fn notify_spies(server: &Server, sender: &Player, target: &Player, message: &str) {
    let sender_name = sender.get_name();
    let target_name = target.get_name();
    for player in server.get_all_players() {
        let name = player.get_name();
        if name.eq_ignore_ascii_case(&sender_name) || name.eq_ignore_ascii_case(&target_name) {
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
            .replace("{0}", &sender_name)
            .replace("{1}", &target_name)
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
