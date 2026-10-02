//! Player session data — the Bukkit v2 `PlayerDataStore.PlayerState` surface.
//!
//! Stores per-player chat state: active channel membership, joined channels,
//! mute / shadow-mute flags, ignore list and chosen chat colour. The upstream
//! implementation persists this to a database (`datasource.yml`) on logout and
//! shutdown; this Pumpkin port keeps the *session* copy in memory (see
//! [`SessionPlayers`]) and mirrors the Mod's rows in a real SQLite file through
//! the embedded turso_core (limbo) engine ([`PlayerStore`]). The `datasource.yml`
//! resolution, table names and the exact SQL texts stay in [`crate::datasource`]
//! (spec §7 verified the engine can run them).
//!
//! The muted/ignored snapshot is also what the Redis relay exchanges between
//! servers (35 s TTL), keeping cross-server ignore checks working without
//! sharing the full state table.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::num::NonZero;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use pumpkin_plugin_api::events::player::{PlayerJoinEvent, PlayerLeaveEvent};
use pumpkin_plugin_api::events::{EventData, EventHandler, EventPriority};
use pumpkin_plugin_api::{Context, Server};

use crate::datasource::Datasource;
use turso_core::{OpenOptions, SqliteDialect, StepResult, Value};

/// One player's chat state (mirrors `PlayerDataStore.PlayerState`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlayerState {
    /// Player UUID (hyphenated, `PlayerDataStore.java:150`-style key); the
    /// file-backed store keys by it. Empty until the join handler stamps it.
    pub player_uuid: String,
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

    /// Places a player joining with the state [`PlayerStore::load`] returned,
    /// mirroring `PlayerDataStore.load` + `save` on login (spec §1.4).
    ///
    /// A session entry that already exists (re-login flicker) is kept and only
    /// the default channel is re-ensured; a fresh entry takes the persisted
    /// state with the default channel joined and the stored active channel
    /// preferred over the configuration default (the Mod restores the saved
    /// `is_active` row the same way).
    pub fn join_with(&mut self, name: &str, default_channel: &str, stored: PlayerState) {
        let key = name.to_ascii_lowercase();
        if let Some(entry) = self.states.get_mut(&key) {
            entry
                .joined_channels
                .insert(default_channel.to_ascii_lowercase());
            return;
        }
        let mut state = stored;
        if state.active_channel.is_empty() {
            state.active_channel = default_channel.to_string();
        }
        state
            .joined_channels
            .insert(default_channel.to_ascii_lowercase());
        self.states.insert(key, state);
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

// ---------------------------------------------------------------------------
// Persistence: `PlayerDataStore` ports.
//
// The upstream persists the four tables (`player_state`, `player_channels`,
// `player_ignored`, `player_preferences`) through JDBC, one transaction per
// `save` (`PlayerDataStore.java:193-227`). The WASM sandbox has no JDBC
// driver, so this port executes the same SQL texts (kept exact in
// [`crate::datasource`]) through the embedded turso_core (limbo) engine
// against the resolved SQLite file. The spec §7 probe ruled the network
// backends out and confirmed PlatformIO on wasm32-wasip2 runs on GenericIO
// (= std::fs), so real files open normally.
// ---------------------------------------------------------------------------

/// SQLite-backed execution layer for `PlayerDataStore` (`PlayerStore`).
///
/// Opens `datasource.sqlite_file` through turso_core, ensures the four tables
/// (`Datasource::ddl`) and runs the Mod's exact SELECT / UPDATE / INSERT /
/// DELETE statements. `save` batches one transaction per player, mirroring
/// `PlayerDataStore.saveAsync`/`save` (`:189-227`).
#[derive(Clone)]
pub struct PlayerStore {
    conn: Arc<turso_core::Connection>,
    datasource: Datasource,
}

/// `Value::from_text` wrapper — the engine takes owned text.
fn text(value: &str) -> Value {
    Value::from_text(value.to_string())
}

impl PlayerStore {
    /// Opens `datasource`'s SQLite file through turso_core and ensures the
    /// four tables exist (`PlayerDataStore.java:97-128`).
    pub fn open(datasource: &Datasource) -> Result<PlayerStore, String> {
        let sqlite_file = &datasource.sqlite_file;
        if let Some(parent) = sqlite_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let io = Arc::new(
            turso_core::io::PlatformIO::new()
                .map_err(|error| format!("cannot initialise IO: {error}"))?,
        );
        let options = || OpenOptions::new(Arc::new(SqliteDialect {}));
        let database = turso_core::Database::open(io, &sqlite_file.to_string_lossy(), options())
            .map_err(|error| format!("cannot open {}: {error}", sqlite_file.display()))?;
        let conn = database
            .connect()
            .map_err(|error| format!("cannot connect: {error}"))?;
        let store = PlayerStore {
            conn,
            datasource: datasource.clone(),
        };
        for ddl in datasource.ddl() {
            store
                .conn
                .execute(&ddl)
                .map_err(|error| format!("cannot initialise schema: {error}"))?;
        }
        Ok(store)
    }

    /// Binds `binds` to `stmt` (parameter 1 = first element).
    fn bind_all(&self, stmt: &mut turso_core::Statement, binds: &[Value]) -> Result<(), String> {
        for (index, value) in binds.iter().enumerate() {
            stmt.bind_at(NonZero::new(index + 1).unwrap(), value.clone())
                .map_err(|error| format!("cannot bind parameter {}: {error}", index + 1))?;
        }
        Ok(())
    }

    /// Runs a single non-query statement and reports whether it touched a row
    /// (`changes()` > 0), mirroring the Mod's `executeUpdate()` counts.
    fn execute_changes(&self, sql: &str, binds: &[Value]) -> Result<bool, String> {
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|error| format!("cannot prepare {sql}: {error}"))?;
        self.bind_all(&mut stmt, binds)?;
        match stmt.step().map_err(|error| format!("cannot step {sql}: {error}"))? {
            _ => {}
        }
        let mut changed = 0i64;
        let mut count = self
            .conn
            .query("SELECT changes()")
            .map_err(|error| format!("cannot read changes(): {error}"))?
            .ok_or_else(|| "changes() returned no statement".to_string())?;
        loop {
            match count
                .step()
                .map_err(|error| format!("cannot step changes(): {error}"))?
            {
                StepResult::Row => {
                    changed = count
                        .row()
                        .ok_or_else(|| "changes() returned no row".to_string())?
                        .get::<i64>(0)
                        .unwrap_or(0);
                }
                _ => break,
            }
        }
        Ok(changed > 0)
    }

    /// Loads a player's persisted state (`PlayerDataStore.load`, `:160-187`).
    ///
    /// A missing `player_state` row means a blank record — the Mod's `SELECT`
    /// returns no rows and the defaults stand; the channel / ignore / colour
    /// tables are only consulted when the state row exists.
    pub fn load(&self, uuid: &str) -> Result<PlayerState, String> {
        let mut state = PlayerState {
            player_uuid: uuid.to_string(),
            ..PlayerState::default()
        };

        // `player_state` row (`:161-179`).
        let mut stmt = self
            .conn
            .prepare(self.datasource.load_state_sql())
            .map_err(|error| format!("cannot prepare state query: {error}"))?;
        self.bind_all(&mut stmt, &[text(uuid)])?;
        let mut found = false;
        loop {
            match stmt
                .step()
                .map_err(|error| format!("cannot step state query: {error}"))?
            {
                StepResult::Row => {
                    found = true;
                    let row = stmt.row().ok_or_else(|| "state row lost".to_string())?;
                    state.mute_until = row.get::<i64>(0).unwrap_or(0);
                    state.mute_reason = row.get::<String>(1).unwrap_or_default();
                    state.shadow_muted = row.get::<i64>(2).unwrap_or(0) != 0;
                    state.private_spy = row.get::<i64>(3).unwrap_or(0) != 0;
                }
                _ => break,
            }
        }
        if !found {
            return Ok(state);
        }

        // Channel membership (`loadMembership`, `:234-253`).
        let mut stmt = self
            .conn
            .prepare(self.datasource.load_membership_sql())
            .map_err(|error| format!("cannot prepare membership query: {error}"))?;
        self.bind_all(&mut stmt, &[text(uuid)])?;
        loop {
            match stmt
                .step()
                .map_err(|error| format!("cannot step membership query: {error}"))?
            {
                StepResult::Row => {
                    let row = stmt.row().ok_or_else(|| "membership row lost".to_string())?;
                    let channel: String = row.get::<String>(0).unwrap_or_default();
                    if channel.trim().is_empty() {
                        continue;
                    }
                    if row.get::<i64>(1).unwrap_or(0) != 0 {
                        state.active_channel = channel.clone();
                    }
                    state.joined_channels.insert(channel.to_ascii_lowercase());
                }
                _ => break,
            }
        }

        // Ignore list (`loadIgnoredPlayers`, `:276-289`). The port tracks
        // names only, so the UUID column is ignored on read.
        let mut stmt = self
            .conn
            .prepare(self.datasource.load_ignored_sql())
            .map_err(|error| format!("cannot prepare ignore query: {error}"))?;
        self.bind_all(&mut stmt, &[text(uuid)])?;
        loop {
            match stmt
                .step()
                .map_err(|error| format!("cannot step ignore query: {error}"))?
            {
                StepResult::Row => {
                    let row = stmt.row().ok_or_else(|| "ignore row lost".to_string())?;
                    let name: String = row.get::<String>(1).unwrap_or_default();
                    if !name.trim().is_empty() {
                        state.ignored.insert(name.to_ascii_lowercase());
                    }
                }
                _ => break,
            }
        }

        // Chat colour (`loadChatColor`, `:315-337`).
        let mut stmt = self
            .conn
            .prepare(self.datasource.load_chat_color_sql())
            .map_err(|error| format!("cannot prepare colour query: {error}"))?;
        self.bind_all(&mut stmt, &[text(uuid)])?;
        loop {
            match stmt
                .step()
                .map_err(|error| format!("cannot step colour query: {error}"))?
            {
                StepResult::Row => {
                    state.colour = stmt
                        .row()
                        .ok_or_else(|| "colour row lost".to_string())?
                        .get::<String>(0)
                        .unwrap_or_default();
                }
                _ => break,
            }
        }

        Ok(state)
    }

    /// Persists `state` for `name` in one transaction (`PlayerDataStore.save`,
    /// `:193-227`). Refused without a UUID (the DB row key would be NULL).
    pub fn save(&self, name: &str, state: &PlayerState) -> Result<(), String> {
        if state.player_uuid.is_empty() {
            return Err("cannot save a player state without a UUID".to_string());
        }
        self.conn
            .execute("BEGIN")
            .map_err(|error| format!("cannot begin transaction: {error}"))?;
        let result = self.save_inner(name, state);
        match result {
            Ok(()) => self
                .conn
                .execute("COMMIT")
                .map_err(|error| format!("cannot commit transaction: {error}")),
            Err(error) => {
                let _ = self.conn.execute("ROLLBACK");
                Err(error)
            }
        }
    }

    /// The four table writes behind `save`, run inside the caller's
    /// transaction (`PlayerDataStore.java:194-223`).
    fn save_inner(&self, name: &str, state: &PlayerState) -> Result<(), String> {
        // `player_state`: UPDATE first, INSERT when it touched no row
        // (`:194-218`).
        let (update, insert) = self.datasource.save_state_sql();
        let changed = self.execute_changes(
            &update,
            &[
                text(name),
                Value::from_i64(state.mute_until),
                text(&state.mute_reason),
                Value::from_i64(state.shadow_muted as i64),
                Value::from_i64(state.private_spy as i64),
                text(&state.player_uuid),
            ],
        )?;
        if !changed {
            self.execute_changes(
                &insert,
                &[
                    text(&state.player_uuid),
                    text(name),
                    Value::from_i64(state.mute_until),
                    text(&state.mute_reason),
                    Value::from_i64(state.shadow_muted as i64),
                    Value::from_i64(state.private_spy as i64),
                ],
            )?;
        }
        // `player_channels`: delete, then re-insert the joined set
        // (`:256-273`). The active channel's own casing is preserved (the
        // stored `is_active` row keeps the state's original spelling).
        let (delete, insert) = self.datasource.save_membership_sql();
        self.execute_changes(&delete, &[text(&state.player_uuid)])?;
        if !state.joined_channels.is_empty() {
            let mut joined: Vec<&String> = state.joined_channels.iter().collect();
            joined.sort();
            for channel in joined {
                let is_active = !state.active_channel.is_empty()
                    && channel.eq_ignore_ascii_case(&state.active_channel);
                let stored = if is_active {
                    state.active_channel.as_str()
                } else {
                    channel.as_str()
                };
                self.execute_changes(
                    &insert,
                    &[text(&state.player_uuid), text(stored), Value::from_i64(is_active as i64)],
                )?;
            }
        }
        // `player_ignored`: delete, then re-insert the ignore set (`:295-312`).
        // The port tracks names only, so the `ignored_uuid` column carries the
        // name (the DDL requires a non-null value; the read side ignores it).
        let (delete, insert) = self.datasource.save_ignored_sql();
        self.execute_changes(&delete, &[text(&state.player_uuid)])?;
        if !state.ignored.is_empty() {
            let mut names: Vec<&String> = state.ignored.iter().collect();
            names.sort();
            for name in names {
                self.execute_changes(
                    &insert,
                    &[
                        text(&state.player_uuid),
                        text(name),
                        text(name),
                    ],
                )?;
            }
        }
        // `player_preferences`: UPDATE first, INSERT when it touched no row
        // (`:326-337`).
        let (update, insert) = self.datasource.save_preferences_sql();
        let changed = self.execute_changes(
            &update,
            &[text(&state.colour), text(&state.player_uuid)],
        )?;
        if !changed {
            self.execute_changes(
                &insert,
                &[text(&state.player_uuid), text(&state.colour)],
            )?;
        }
        Ok(())
    }
}

/// Join handler: restores the persisted chat state (`PlayerDataStore.load`).
struct PersistJoinHandler;

impl EventHandler<PlayerJoinEvent> for PersistJoinHandler {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        let store = match player_store() {
            Ok(store) => store,
            Err(error) => {
                crate::diag::warn(format!("[TrChat] playerdata: {error}"));
                return event;
            }
        };
        let uuid = event.player.get_id().to_string();
        let stored = store.load(&uuid).unwrap_or_else(|error| {
            crate::diag::warn(format!(
                "[TrChat] playerdata: falling back to defaults for {}: {error}",
                event.player.get_name()
            ));
            PlayerState {
                player_uuid: uuid.clone(),
                ..PlayerState::default()
            }
        });
        let default_channel = default_channel_id();
        SessionPlayers::global()
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .join_with(&event.player.get_name(), &default_channel, stored);
        event
    }
}

/// Leave handler: persists the session state (`PlayerDataStore.save*`).
struct PersistLeaveHandler;

impl EventHandler<PlayerLeaveEvent> for PersistLeaveHandler {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerLeaveEvent>,
    ) -> EventData<PlayerLeaveEvent> {
        let name = event.player.get_name();
        let uuid = event.player.get_id().to_string();
        let state = SessionPlayers::global()
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .leave(&name)
            .unwrap_or_else(|| PlayerState {
                player_uuid: uuid.clone(),
                ..PlayerState::default()
            });
        let mut state = state;
        state.player_uuid = uuid.clone();
        match player_store() {
            Ok(store) => {
                if let Err(error) = store.save(&name, &state) {
                    crate::diag::warn(format!(
                        "[TrChat] playerdata: cannot persist {name}: {error}"
                    ));
                }
            }
            Err(error) => crate::diag::warn(format!("[TrChat] playerdata: {error}")),
        }
        event
    }
}

/// Registers the join/leave persistence handlers (`on_load`).
pub fn register(context: &Context) -> Result<(), String> {
    context
        .register_event_handler::<PlayerJoinEvent, PersistJoinHandler>(
            PersistJoinHandler,
            EventPriority::Normal,
            true,
        )
        .map_err(|error| error.to_string())?;
    context
        .register_event_handler::<PlayerLeaveEvent, PersistLeaveHandler>(
            PersistLeaveHandler,
            EventPriority::Normal,
            true,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// The shared database store, rooted at `datasource.yml`'s resolved SQLite
/// file (`Datasource::resolve` — spec §1.1-§1.4).
fn player_store() -> Result<PlayerStore, String> {
    let folder = crate::config::data_folder();
    if folder.is_empty() {
        return Err("plugin data folder is not initialised yet".to_string());
    }
    let config = crate::config::global_config();
    let cfg = config.read().datasource.clone();
    let datasource = crate::datasource::Datasource::resolve(&cfg, Path::new(&folder))
        .map_err(|error| format!("datasource.yml: {error}"))?;
    crate::diag::debug(format!(
        "[TrChat] playerdata backend: {:?} ({})",
        datasource.backend, folder
    ));
    PlayerStore::open(&datasource)
}

/// The configured auto-join channel id, mirroring
/// `commands.rs` `player-info`'s default (`config.default_channel()`).
fn default_channel_id() -> String {
    let config = crate::config::global_config();
    let config = config.read();
    config
        .default_channel()
        .map(|channel| channel.id.clone())
        .unwrap_or_else(|| "-".to_string())
}

/// Persists every online player's session state (`ModerationService.close`,
/// `ModerationService.java:207-212` — `store.close()` then a per-state
/// synchronous `save`; called from `on_unload`).
pub fn flush_all() {
    let session = SessionPlayers::global()
        .read()
        .unwrap_or_else(|error| error.into_inner());
    let Ok(store) = player_store() else {
        return;
    };
    for (name, state) in session.states.iter() {
        if state.player_uuid.is_empty() {
            continue;
        }
        if let Err(error) = store.save(name, state) {
            crate::diag::warn(format!(
                "[TrChat] playerdata: cannot flush {}: {error}",
                state.player_uuid
            ));
        }
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::datasource::{Backend, TableNames};
    use std::path::PathBuf;

    /// A per-test scratch dir: tests run in parallel, so the label keeps the
    /// stores from deleting each other's files.
    fn temp_dir(label: &str) -> PathBuf {
        let n = std::process::id();
        std::env::temp_dir().join(format!("trchat-playerdata-test-{n}-{label}"))
    }

    /// A test datasource pointing at `dir/data.db` with the Mod's fixed names.
    fn test_datasource(dir: &Path) -> Datasource {
        Datasource {
            backend: Backend::Sqlite,
            tables: TableNames {
                state: "trchat_player_state".into(),
                channels: "trchat_player_channels".into(),
                ignored: "trchat_player_ignored".into(),
                preferences: "trchat_player_preferences".into(),
            },
            sqlite_file: dir.join("data.db"),
        }
    }

    /// Round-trips every persisted field through `save` → `load`.
    #[test]
    fn store_round_trips_full_state() {
        let dir = temp_dir("roundtrip");
        let _ = fs::remove_dir_all(&dir);
        let store = PlayerStore::open(&test_datasource(&dir)).unwrap();

        let mut state = PlayerState {
            player_uuid: "00112233-4455-6677-8899-aabbccddeeff".to_string(),
            active_channel: "Normal".to_string(),
            joined_channels: HashSet::from(["normal".to_string(), "global".to_string()]),
            mute_until: -1,
            mute_reason: "spam".to_string(),
            shadow_muted: true,
            ignored: HashSet::from(["alice".to_string()]),
            colour: "b".to_string(),
            last_private_sender: "carol".to_string(),
            private_spy: true,
        };
        store.save("Alice", &state).unwrap();
        state.active_channel = "Other".to_string();

        let loaded = store.load(&state.player_uuid).unwrap();
        assert_eq!(loaded.player_uuid, state.player_uuid);
        assert_eq!(loaded.active_channel, "Normal", "saved snapshot wins");
        assert_eq!(loaded.joined_channels, HashSet::from(["normal".to_string(), "global".to_string()]));
        assert_eq!(loaded.mute_until, -1);
        assert_eq!(loaded.mute_reason, "spam");
        assert!(loaded.shadow_muted);
        assert_eq!(loaded.ignored, HashSet::from(["alice".to_string()]));
        assert_eq!(loaded.colour, "b");
        assert!(loaded.private_spy);
        // `last_private_sender` is a session-only convenience field: the Mod's
        // four tables have no column for it, so it does not survive a reload.
        assert!(loaded.last_private_sender.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    /// A missing database row loads as a blank record with the key stamped.
    #[test]
    fn missing_file_loads_defaults() {
        let dir = temp_dir("missing");
        let _ = fs::remove_dir_all(&dir);
        let store = PlayerStore::open(&test_datasource(&dir)).unwrap();
        let state = store.load("ffffffff-0000-0000-0000-000000000000").unwrap();
        assert_eq!(state.player_uuid, "ffffffff-0000-0000-0000-000000000000");
        assert_eq!(state.active_channel, "");
        assert!(!state.shadow_muted);
        assert!(state.ignored.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Saving creates the resolved SQLite file (`datasource.yml`'s `SQLite.File`).
    #[test]
    fn save_creates_database_file() {
        let dir = temp_dir("dbfile");
        let _ = fs::remove_dir_all(&dir);
        let datasource = test_datasource(&dir);
        let store = PlayerStore::open(&datasource).unwrap();
        let uuid = "00112233-4455-6677-8899-aabbccddeeff";
        store
            .save(
                "Alice",
                &PlayerState {
                    player_uuid: uuid.to_string(),
                    ..PlayerState::default()
                },
            )
            .unwrap();
        assert!(
            datasource.sqlite_file.exists(),
            "expected {} to exist",
            datasource.sqlite_file.display()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Saving without a UUID is refused (the DB row key would be NULL).
    #[test]
    fn save_without_uuid_is_refused() {
        let dir = temp_dir("nouuid");
        let _ = fs::remove_dir_all(&dir);
        let store = PlayerStore::open(&test_datasource(&dir)).unwrap();
        let err = store
            .save(
                "Alice",
                &PlayerState {
                    ..PlayerState::default()
                },
            )
            .unwrap_err();
        assert!(err.contains("UUID"), "err = {err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A mid-transaction failure rolls back *every* earlier write of that
    /// `save`, not just the failing statement (`save`'s ROLLBACK path).
    #[test]
    fn save_is_atomic_on_mid_transaction_failure() {
        let dir = temp_dir("atomic");
        let _ = fs::remove_dir_all(&dir);
        let mut store = PlayerStore::open(&test_datasource(&dir)).unwrap();
        let uuid = "00112233-4455-6677-8899-aabbccddeeff";

        store
            .save(
                "Alice",
                &PlayerState {
                    player_uuid: uuid.to_string(),
                    active_channel: "Global".to_string(),
                    joined_channels: HashSet::from(["global".to_string()]),
                    ..PlayerState::default()
                },
            )
            .unwrap();

        // Break the channels table name *after* open, so the schema is intact
        // but the membership DELETE fails mid-save.
        store.datasource.tables.channels = "trchat_player_channels_broken".into();
        let err = store
            .save(
                "Alice",
                &PlayerState {
                    player_uuid: uuid.to_string(),
                    active_channel: "Normal".to_string(),
                    joined_channels: HashSet::from(["normal".to_string()]),
                    ..PlayerState::default()
                },
            )
            .unwrap_err();
        assert!(
            err.contains("trchat_player_channels_broken"),
            "err = {err}"
        );

        // The broken save rolled back its own `player_state` UPDATE too, so
        // the first save's state is untouched.
        store.datasource.tables.channels = "trchat_player_channels".into();
        let loaded = store.load(uuid).unwrap();
        assert_eq!(loaded.active_channel, "Global");
        assert_eq!(loaded.joined_channels, HashSet::from(["global".to_string()]));
        let _ = fs::remove_dir_all(&dir);
    }

    /// `open` reports an error (never panics) when the SQLite file's parent
    /// cannot be created — here the parent path is an existing plain file.
    #[test]
    fn open_reports_uncreatable_parent() {
        let dir = temp_dir("badparent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("blocker");
        fs::write(&blocker, "a file, not a directory").unwrap();

        let mut datasource = test_datasource(&dir);
        datasource.sqlite_file = blocker.join("data.db");
        let err = match PlayerStore::open(&datasource) {
            Err(error) => error,
            Ok(_) => panic!("open should fail when the parent is a file"),
        };
        assert!(err.starts_with("cannot create"), "err = {err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `join_with` prefers the stored active channel, else the default.
    #[test]
    fn join_with_restores_stored_state() {
        let mut s = SessionPlayers::default();
        let stored = PlayerState {
            player_uuid: "00112233-4455-6677-8899-aabbccddeeff".to_string(),
            active_channel: "Global".to_string(),
            joined_channels: HashSet::from(["global".to_string()]),
            ..PlayerState::default()
        };
        s.join_with("Alice", "Normal", stored);
        assert_eq!(s.state("Alice").unwrap().active_channel, "Global");
        assert!(s.state("Alice").unwrap().joined_channels.contains("normal"));

        // A re-join keeps prior state and only ensures the default channel.
        s.join_with("Alice", "Normal", PlayerState::default());
        assert_eq!(s.state("Alice").unwrap().active_channel, "Global");
    }

    /// A blank stored state falls back to the configuration default channel.
    #[test]
    fn join_with_falls_back_to_default_channel() {
        let mut s = SessionPlayers::default();
        s.join_with(
            "Bob",
            "Normal",
            PlayerState {
                player_uuid: "00112233-4455-6677-8899-aabbccddeeff".to_string(),
                ..PlayerState::default()
            },
        );
        assert_eq!(s.state("Bob").unwrap().active_channel, "Normal");
    }
}
