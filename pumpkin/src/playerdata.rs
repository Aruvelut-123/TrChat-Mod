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
#[allow(dead_code)] // `shadow_muted`/`colour` are Bukkit v2 surface, not wired yet
pub struct PlayerState {
    /// Active channel id (original case, e.g. `Normal`).
    pub active_channel: String,
    /// Joined channel ids, lowercased.
    pub joined_channels: HashSet<String>,
    /// True while the player is muted.
    pub muted: bool,
    /// True while shadow-muted (messages are rendered back to the sender only).
    pub shadow_muted: bool,
    /// Players ignored by this player, lowercased names.
    pub ignored: HashSet<String>,
    /// Chosen chat colour code (single char, no `&`).
    pub colour: String,
    /// Last player who privately messaged this player, lowercased — the target
    /// of `/trreply` (spec §1.6 `lastPrivateSender`).
    pub last_private_sender: String,
    /// True while private-message spy is enabled (`/trchat spy`).
    pub private_spy: bool,
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

    /// Whether `name` is muted (or globally muted).
    pub fn is_muted(&self, name: &str) -> bool {
        self.global_mute
            || self
                .states
                .get(&name.to_ascii_lowercase())
                .is_some_and(|s| s.muted)
    }

    /// Whether `muted_by` ignores `target`.
    pub fn ignores(&self, muted_by: &str, target: &str) -> bool {
        self.states
            .get(&muted_by.to_ascii_lowercase())
            .is_some_and(|s| s.ignored.contains(&target.to_ascii_lowercase()))
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
        s.state_mut("alice").unwrap().muted = true;
        assert!(s.is_muted("Alice"));

        s.state_mut("alice")
            .unwrap()
            .ignored
            .insert("bob".to_string());
        assert!(s.ignores("Alice", "Bob"));
        assert!(!s.ignores("Bob", "Alice"));
    }

    #[test]
    fn global_mute_overrides() {
        let mut s = SessionPlayers::default();
        s.set_global_muted(true);
        assert!(s.is_muted("anyone"));
    }
}
