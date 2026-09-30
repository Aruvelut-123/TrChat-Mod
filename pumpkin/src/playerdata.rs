//! Player session data — the Bukkit v2 `PlayerDataStore.PlayerState` surface.
//!
//! Stores per-player chat state: active channel membership, joined channels,
//! mute / shadow-mute flags, ignore list and chosen chat colour. The upstream
//! implementation persists this to a database on logout and shutdown; this
//! Pumpkin port keeps the *session* copy in memory (see [`SessionPlayers`]) and
//! persists a JSON snapshot to the plugin data folder on unload.
//!
//! The muted/ignored snapshot is also what the Redis relay exchanges between
//! servers (35 s TTL), keeping cross-server ignore checks working without
//! sharing the full state table.

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

/// One player's chat state (mirrors `PlayerDataStore.PlayerState`).
#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    /// Active channel id (original case, e.g. `Normal`).
    pub active_channel: String,
    /// Joined channel ids, lowercased.
    pub joined_channels: HashSet<String>,
    /// Mute expiry: `0` = not muted, `< 0` = permanent, `> 0` = epoch millis
    /// (spec §6, `ModerationService.java:56-61`).
    pub mute_until: i64,
    /// Reason reported by `General-Muted` and the player status report; a blank
    /// reason is stored as `-` (`ModerationService.java:69-72`).
    pub mute_reason: String,
    /// True while shadow-muted (messages are rendered back to the sender only).
    pub shadow_muted: bool,
    /// Players ignored by this player, lowercased names.
    pub ignored: HashSet<String>,
    /// Chosen chat colour code (single char, no `&`), empty when unset.
    pub colour: String,
    /// Last player who privately messaged this player, lowercased — the target
    /// of `/trreply` (spec §1.6 `lastPrivateSender`).
    pub last_private_sender: String,
    /// True while private-message spy is enabled (`/trchat spy`).
    pub private_spy: bool,
}

impl PlayerState {
    /// Whether the personal mute is in force at `now`.
    ///
    /// An expiry at or before `now` reads as unmuted; the upstream also clears
    /// the field and persists that, which [`SessionPlayers::expire_mute`] does.
    pub fn is_mute_active(&self, now: i64) -> bool {
        match self.mute_until {
            0 => false,
            // Negative means `-1`, the permanent marker.
            until if until < 0 => true,
            until => until > now,
        }
    }
}

/// Session-wide registry: the online players' [`PlayerState`] plus the global
/// mute flag. All access is per-event and short-lived.
#[derive(Default)]
pub struct SessionPlayers {
    /// name(lowercased) → state snapshot.
    states: HashMap<String, PlayerState>,
    /// Global chat mute.
    global_mute: bool,
}

static SESSION: OnceLock<RwLock<SessionPlayers>> = OnceLock::new();

impl SessionPlayers {
    /// Access to the process-wide session registry.
    pub fn global() -> &'static RwLock<SessionPlayers> {
        SESSION.get_or_init(|| RwLock::new(SessionPlayers::default()))
    }

    /// Records a player joining (on `player-join`), keeping prior state.
    #[allow(dead_code)] // join/leave/state: session lifecycle API, wired by event handlers later
    pub fn join(&mut self, name: &str, default_channel: &str) {
        let entry = self
            .states
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| PlayerState {
                active_channel: default_channel.to_string(),
                ..PlayerState::default()
            });
        entry
            .joined_channels
            .insert(default_channel.to_ascii_lowercase());
    }

    /// Removes a player's session state (on `player-leave`).
    #[allow(dead_code)]
    pub fn leave(&mut self, name: &str) -> Option<PlayerState> {
        self.states.remove(&name.to_ascii_lowercase())
    }

    /// Borrows a player's state, creating a default if unseen.
    #[allow(dead_code)]
    pub fn state(&self, name: &str) -> Option<&PlayerState> {
        self.states.get(&name.to_ascii_lowercase())
    }

    /// Mutably borrows a player's state.
    pub fn state_mut(&mut self, name: &str) -> Option<&mut PlayerState> {
        self.states.get_mut(&name.to_ascii_lowercase())
    }

    /// True while chat is globally muted.
    pub fn is_global_muted(&self) -> bool {
        self.global_mute
    }

    /// Sets the global chat mute state.
    pub fn set_global_muted(&mut self, muted: bool) {
        self.global_mute = muted;
    }

    /// Whether `name` carries an active *personal* mute.
    ///
    /// The global mute is a separate question ([`Self::is_global_muted`]) because
    /// it does not apply to operators (spec §1.4 step 3).
    pub fn is_muted(&self, name: &str) -> bool {
        self.is_muted_at(name, crate::clock::now_millis())
    }

    /// [`Self::is_muted`] against an explicit clock, so the expiry rules are
    /// testable without waiting.
    pub fn is_muted_at(&self, name: &str, now: i64) -> bool {
        self.states
            .get(&name.to_ascii_lowercase())
            .is_some_and(|state| state.is_mute_active(now))
    }

    /// Mutes `name` for `duration_millis`, returning the applied expiry.
    ///
    /// A negative duration is the permanent marker `-1`; otherwise the expiry is
    /// `now + duration` (`ModerationService.java:74-77`). `None` means the
    /// player has no session state.
    pub fn mute(&mut self, name: &str, duration_millis: i64, reason: &str) -> Option<i64> {
        let now = crate::clock::now_millis();
        let until = if duration_millis < 0 {
            -1
        } else {
            now.saturating_add(duration_millis)
        };
        self.state_mut(name).map(|state| {
            state.mute_until = until;
            // A blank reason is reported as `-` (`ModerationService.java:69-72`).
            state.mute_reason = if reason.trim().is_empty() {
                "-".to_string()
            } else {
                reason.trim().to_string()
            };
            until
        })
    }

    /// Clears `name`'s personal mute, returning `true` when one was set.
    pub fn unmute(&mut self, name: &str) -> Option<bool> {
        self.state_mut(name).map(|state| {
            let was_muted = state.mute_until != 0;
            state.mute_until = 0;
            state.mute_reason.clear();
            was_muted
        })
    }

    /// Drops an expired mute, mirroring the upstream auto-clear on read
    /// (`ModerationService.java:54-62`). Returns `true` when one was cleared.
    pub fn expire_mute(&mut self, name: &str) -> bool {
        let now = crate::clock::now_millis();
        self.state_mut(name).is_some_and(|state| {
            // Anything other than a live expiry is left alone: `0` is unmuted
            // and a negative value is permanent.
            if state.mute_until > 0 && state.mute_until <= now {
                state.mute_until = 0;
                state.mute_reason.clear();
                true
            } else {
                false
            }
        })
    }

    /// `(mute_until, mute_reason)` for `name`, or `None` when they are offline.
    pub fn mute_state(&self, name: &str) -> Option<(i64, &str)> {
        self.state(name)
            .map(|state| (state.mute_until, state.mute_reason.as_str()))
    }

    /// Stores `name`'s chat colour code, or clears it when `colour` is `None`.
    ///
    /// Returns `false` when the player has no session state (offline).
    pub fn set_chat_color(&mut self, name: &str, colour: Option<char>) -> bool {
        self.state_mut(name).is_some_and(|state| {
            state.colour = colour.map(|code| code.to_string()).unwrap_or_default();
            true
        })
    }

    /// `name`'s chat colour code, or an empty string when unset or offline.
    pub fn chat_color(&self, name: &str) -> String {
        self.state(name)
            .map(|state| state.colour.clone())
            .unwrap_or_default()
    }

    /// Whether `muted_by` ignores `target`.
    pub fn ignores(&self, muted_by: &str, target: &str) -> bool {
        self.states
            .get(&muted_by.to_ascii_lowercase())
            .is_some_and(|s| s.ignored.contains(&target.to_ascii_lowercase()))
    }

    /// Whether `name` is shadow-muted — their messages are echoed back to them
    /// alone instead of being broadcast (spec §1.3 step 6).
    pub fn is_shadow_muted(&self, name: &str) -> bool {
        self.states
            .get(&name.to_ascii_lowercase())
            .is_some_and(|s| s.shadow_muted)
    }

    /// Sets the shadow-mute flag, returning the new state — or `None` when the
    /// player is not online (no session state to touch).
    pub fn set_shadow_muted(&mut self, name: &str, muted: bool) -> Option<bool> {
        self.state_mut(name).map(|state| {
            state.shadow_muted = muted;
            muted
        })
    }

    /// Flips the shadow-mute flag, returning the new state — or `None` when the
    /// player is not online.
    pub fn toggle_shadow_muted(&mut self, name: &str) -> Option<bool> {
        self.state_mut(name).map(|state| {
            state.shadow_muted = !state.shadow_muted;
            state.shadow_muted
        })
    }
}

/// §6 `muteExpiry` — the literal `permanent` for the permanent marker (`-1`),
/// otherwise the expiry as `yyyy-MM-dd HH:mm:ss`.
///
/// Shared by `General-Muted` and the `/trchat status <player>` report.
pub fn mute_expiry_text(until: i64) -> String {
    if until < 0 {
        "permanent".to_string()
    } else {
        crate::clock::format_millis(until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_and_ignore() {
        let mut s = SessionPlayers::default();
        s.join("Alice", "Normal");
        assert!(!s.is_muted("Alice"));
        s.state_mut("alice").unwrap().mute_until = -1;
        assert!(s.is_muted("Alice"));

        s.state_mut("alice")
            .unwrap()
            .ignored
            .insert("bob".to_string());
        assert!(s.ignores("Alice", "Bob"));
        assert!(!s.ignores("Bob", "Alice"));
    }

    /// §6 — `0` is unmuted, `< 0` is permanent, `> 0` expires against the clock.
    #[test]
    fn mute_expiry_follows_the_three_state_model() {
        let mut s = SessionPlayers::default();
        s.join("Alice", "Normal");
        let state = s.state_mut("Alice").unwrap();

        state.mute_until = 0;
        assert!(!state.is_mute_active(1_000));

        state.mute_until = -1;
        assert!(state.is_mute_active(1_000));

        state.mute_until = 2_000;
        assert!(state.is_mute_active(1_000), "not yet expired");
        assert!(!state.is_mute_active(2_000), "expiry is inclusive");

        // No session state at all means nothing to look up.
        assert!(!s.is_muted_at("Nobody", 1_000));
    }

    /// `mute` derives the expiry from the duration and normalises the reason.
    #[test]
    fn mute_records_duration_and_reason() {
        let mut s = SessionPlayers::default();
        assert_eq!(s.mute("Nobody", 1_000, "x"), None, "offline players");

        s.join("Alice", "Normal");
        let now = crate::clock::now_millis();

        let until = s.mute("Alice", 60_000, "  spam  ").unwrap();
        assert!(until > now, "a positive duration expires in the future");
        assert!(s.is_muted("Alice"));
        assert_eq!(s.mute_state("Alice"), Some((until, "spam")));

        // A blank reason is reported as `-`.
        s.mute("Alice", 60_000, "   ");
        assert_eq!(s.mute_state("Alice").unwrap().1, "-");

        // A negative duration is the permanent marker.
        assert_eq!(s.mute("Alice", -5, "forever"), Some(-1));
        assert!(s.is_muted("Alice"));

        assert_eq!(s.unmute("Alice"), Some(true));
        assert!(!s.is_muted("Alice"));
        assert_eq!(s.unmute("Alice"), Some(false), "already unmuted");
        assert_eq!(s.mute_state("Alice"), Some((0, "")));
    }

    /// An expired mute is dropped on read, like the upstream auto-clear.
    #[test]
    fn expired_mutes_are_cleared_on_read() {
        let mut s = SessionPlayers::default();
        s.join("Alice", "Normal");

        // A timestamp strictly in the past, so the result cannot depend on how
        // fast the clock ticks between the two calls.
        s.state_mut("Alice").unwrap().mute_until = crate::clock::now_millis() - 1;
        assert!(s.expire_mute("Alice"));
        assert_eq!(s.mute_state("Alice").unwrap().0, 0);

        // A permanent mute is never expired.
        s.mute("Alice", -1, "forever");
        assert!(!s.expire_mute("Alice"));
        assert!(s.is_muted("Alice"));

        // Neither is an unmuted player, and an offline one has no state.
        s.unmute("Alice");
        assert!(!s.expire_mute("Alice"));
        assert!(!s.expire_mute("Nobody"));
    }

    #[test]
    fn global_mute_overrides() {
        let mut s = SessionPlayers::default();
        s.set_global_muted(true);
        assert!(s.is_global_muted());
        // The global mute is not a personal mute: the two are asked separately
        // because operators bypass only the former (spec §1.4 step 3).
        assert!(!s.is_muted("anyone"));
    }

    /// `/trchat color` — the chosen code is stored per player and cleared by
    /// `None`; an offline player has nothing to update.
    #[test]
    fn chat_color_is_stored_and_cleared() {
        let mut s = SessionPlayers::default();
        assert_eq!(s.chat_color("Alice"), "", "unset by default");
        assert!(!s.set_chat_color("Alice", Some('a')), "offline players");

        s.join("Alice", "Normal");
        assert!(s.set_chat_color("alice", Some('a')));
        assert_eq!(s.chat_color("Alice"), "a", "lookup is case-insensitive");

        assert!(s.set_chat_color("Alice", Some('f')));
        assert_eq!(s.chat_color("Alice"), "f");

        assert!(s.set_chat_color("Alice", None));
        assert_eq!(s.chat_color("Alice"), "");
    }

    /// §1.3 step 6 — shadow mute is a per-player flag, set/cleared/toggled
    /// through the session store; an offline player has no state to change.
    #[test]
    fn shadow_mute_is_tracked_per_player() {
        let mut s = SessionPlayers::default();
        assert!(!s.is_shadow_muted("Alice"), "unseen players are not muted");
        assert_eq!(
            s.set_shadow_muted("Alice", true),
            None,
            "no session state means nothing to update"
        );

        s.join("Alice", "Normal");
        assert_eq!(s.set_shadow_muted("Alice", true), Some(true));
        // Lookup is case-insensitive, like the rest of the store.
        assert!(s.is_shadow_muted("alice"));

        assert_eq!(s.toggle_shadow_muted("Alice"), Some(false));
        assert!(!s.is_shadow_muted("Alice"));
        assert_eq!(s.toggle_shadow_muted("Alice"), Some(true));

        // Shadow mute is independent of the plain mute flag.
        assert!(!s.is_muted("Alice"));
    }
}
