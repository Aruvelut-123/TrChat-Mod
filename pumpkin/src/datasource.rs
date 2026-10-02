//! `datasource.yml` — where moderation / ignore state survives.
//!
//! Ports the configuration branch of `data/PlayerDataStore.java` `initialize`
//! (fact spec: `docs/spec/data-redis-update.md` §1.1-§1.4): the `Type` switch,
//! the SQLite file resolution and the exact `CREATE TABLE` / `SELECT` /
//! `UPDATE` / `INSERT` / `DELETE` SQL texts the Mod runs against the four tables
//! (`player_state`, `player_channels`, `player_ignored`, `player_preferences`).
//!
//! This port runs the embedded engine **turso_core** (limbo) instead of a JDBC
//! driver, so only `Type: SQLite` / `Type: Local` is supported: the WASM sandbox
//! has no JDBC driver, and turso_core speaks SQLite (its Postgres dialect is
//! experimental and not exposed through the WASM component — see
//! `docs/spec/data-redis-update.md` §7 for the probe that ruled the network
//! backends out). The network `Type` values (MySQL, MariaDB, PostgreSQL) and the
//! generic JDBC branch are **rejected** by [`Datasource::resolve`]; the Mod's
//! fallback JDBC branch is itself broken (spec §7 conclusion 9 —
//! `ignoredTable` / `preferenceTable` stay `null` and the save paths throw
//! `NPE`), so there is nothing to mirror.

use std::path::{Component, Path, PathBuf};

use crate::config::DataSourceConfig;

/// The data-source backend this port supports. Only SQLite survives: the
/// sandbox runs turso_core (limbo), which is a SQLite engine (spec §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// `Type: SQLite` / `Type: Local` — a local `data.db` file.
    Sqlite,
}

/// The four table names (`PlayerDataStore.java:73-76`): SQLite uses the fixed
/// Mod names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableNames {
    pub state: String,
    pub channels: String,
    pub ignored: String,
    pub preferences: String,
}

/// A resolved data source: the backend, the table names and the absolute
/// SQLite database path. [`crate::playerdata::PlayerStore`] executes the SQL
/// through turso_core.
#[derive(Debug, Clone)]
pub struct Datasource {
    pub backend: Backend,
    pub tables: TableNames,
    /// Absolute SQLite database path (`PlayerDataStore.java:68`
    /// `folder.resolve(configured).normalize()`).
    pub sqlite_file: PathBuf,
}

impl Datasource {
    /// Resolves `cfg` against the plugin data folder, mirroring
    /// `PlayerDataStore.initialize` `:64-93` — restricted to the SQLite branch.
    pub fn resolve(cfg: &DataSourceConfig, data_folder: &Path) -> Result<Datasource, String> {
        match cfg.kind().as_str() {
            "sqlite" | "local" => Ok(sqlite(cfg, data_folder)),
            _ => Err(format!(
                "datasource Type '{}': this port supports SQLite/Local only \
                 (turso_core is a SQLite engine; the network JDBC branches of the Mod \
                 are out of scope — spec §7)",
                cfg.data_type
            )),
        }
    }

    /// The four `CREATE TABLE IF NOT EXISTS` statements, in the Mod's order and
    /// text (`PlayerDataStore.java:97-128`).
    pub fn ddl(&self) -> Vec<String> {
        vec![
            DDL_STATE.replace("%s", &self.tables.state),
            DDL_CHANNELS.replace("%s", &self.tables.channels),
            DDL_IGNORED.replace("%s", &self.tables.ignored),
            DDL_PREFERENCES.replace("%s", &self.tables.preferences),
        ]
    }

    /// `PlayerState.load` (`:161`).
    pub fn load_state_sql(&self) -> String {
        format!(
            "SELECT mute_until,mute_reason,shadow_muted,private_spy FROM {} WHERE uuid=?",
            self.tables.state
        )
    }

    /// `PlayerState.save` — `(update, insert)` (`:194-197`); the Mod runs the
    /// UPDATE first and only inserts when it touched no row.
    pub fn save_state_sql(&self) -> (String, String) {
        (
            format!(
                "UPDATE {} SET player_name=?,mute_until=?,mute_reason=?,shadow_muted=?,private_spy=? WHERE uuid=?",
                self.tables.state
            ),
            format!(
                "INSERT INTO {} (uuid,player_name,mute_until,mute_reason,shadow_muted,private_spy) VALUES (?,?,?,?,?,?)",
                self.tables.state
            ),
        )
    }

    /// `loadMembership` (`:234`).
    pub fn load_membership_sql(&self) -> String {
        format!(
            "SELECT channel_id,is_active FROM {} WHERE uuid=?",
            self.tables.channels
        )
    }

    /// `saveMembership` — `(delete, insert)` (`:256`, `:264`); the Mod deletes
    /// the player's rows first, then inserts the current joined set.
    pub fn save_membership_sql(&self) -> (String, String) {
        (
            format!("DELETE FROM {} WHERE uuid=?", self.tables.channels),
            format!(
                "INSERT INTO {} (uuid,channel_id,is_active) VALUES (?,?,?)",
                self.tables.channels
            ),
        )
    }

    /// `loadIgnoredPlayers` (`:277`).
    pub fn load_ignored_sql(&self) -> String {
        format!(
            "SELECT ignored_uuid,ignored_name FROM {} WHERE uuid=?",
            self.tables.ignored
        )
    }

    /// `saveIgnoredPlayers` — `(delete, insert)` (`:295`, `:303`).
    pub fn save_ignored_sql(&self) -> (String, String) {
        (
            format!("DELETE FROM {} WHERE uuid=?", self.tables.ignored),
            format!(
                "INSERT INTO {} (uuid,ignored_uuid,ignored_name) VALUES (?,?,?)",
                self.tables.ignored
            ),
        )
    }

    /// `loadChatColor` (`:316`).
    pub fn load_chat_color_sql(&self) -> String {
        format!(
            "SELECT chat_color FROM {} WHERE uuid=?",
            self.tables.preferences
        )
    }

    /// `savePreferences` — `(update, insert)` (`:326`, `:334`); the Mod runs
    /// the UPDATE first and inserts only when it touched no row.
    pub fn save_preferences_sql(&self) -> (String, String) {
        (
            format!(
                "UPDATE {} SET chat_color=? WHERE uuid=?",
                self.tables.preferences
            ),
            format!(
                "INSERT INTO {} (uuid,chat_color) VALUES (?,?)",
                self.tables.preferences
            ),
        )
    }
}

/// The fixed SQLite table names (`PlayerDataStore.java:73-76`).
fn sqlite_table_names() -> TableNames {
    TableNames {
        state: "trchat_player_state".to_string(),
        channels: "trchat_player_channels".to_string(),
        ignored: "trchat_player_ignored".to_string(),
        preferences: "trchat_player_preferences".to_string(),
    }
}

/// `Type: SQLite` / `Type: Local` (`PlayerDataStore.java:65-77`).
///
/// A blank `SQLite.File` falls back to the template default `data.db` (the
/// bundled `datasource.yml` ships it; the parser hands over `""` when the
/// section is missing entirely).
fn sqlite(cfg: &DataSourceConfig, data_folder: &Path) -> Datasource {
    let file = if cfg.sqlite_file.is_empty() {
        "data.db".to_string()
    } else {
        cfg.sqlite_file.clone()
    };
    let configured = Path::new(&file);
    let database = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        normalize_path(&data_folder.join(configured))
    };
    Datasource {
        backend: Backend::Sqlite,
        tables: sqlite_table_names(),
        sqlite_file: database,
    }
}

/// Lexical `Path::normalize` (`PlayerDataStore.java:68`): resolves `.` and `..`
/// components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `CREATE TABLE IF NOT EXISTS %s` for `player_state` (`PlayerDataStore.java:97-106`).
const DDL_STATE: &str = "CREATE TABLE IF NOT EXISTS %s (\n\
  uuid VARCHAR(36) PRIMARY KEY,\n\
  player_name VARCHAR(64) NOT NULL,\n\
  mute_until BIGINT NOT NULL DEFAULT 0,\n\
  mute_reason VARCHAR(512) NOT NULL DEFAULT '',\n\
  shadow_muted INTEGER NOT NULL DEFAULT 0,\n\
  private_spy INTEGER NOT NULL DEFAULT 0\n\
)";

/// `CREATE TABLE IF NOT EXISTS %s` for `player_channels` (`:107-114`).
const DDL_CHANNELS: &str = "CREATE TABLE IF NOT EXISTS %s (\n\
  uuid VARCHAR(36) NOT NULL,\n\
  channel_id VARCHAR(128) NOT NULL,\n\
  is_active INTEGER NOT NULL DEFAULT 0,\n\
  PRIMARY KEY (uuid, channel_id)\n\
)";

/// `CREATE TABLE IF NOT EXISTS %s` for `player_ignored` (`:115-122`).
const DDL_IGNORED: &str = "CREATE TABLE IF NOT EXISTS %s (\n\
  uuid VARCHAR(36) NOT NULL,\n\
  ignored_uuid VARCHAR(36) NOT NULL,\n\
  ignored_name VARCHAR(64) NOT NULL,\n\
  PRIMARY KEY (uuid, ignored_uuid)\n\
)";

/// `CREATE TABLE IF NOT EXISTS %s` for `player_preferences` (`:123-128`).
const DDL_PREFERENCES: &str = "CREATE TABLE IF NOT EXISTS %s (\n\
  uuid VARCHAR(36) PRIMARY KEY,\n\
  chat_color VARCHAR(32) NOT NULL DEFAULT ''\n\
)";

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_cfg() -> DataSourceConfig {
        DataSourceConfig {
            data_type: "SQLite".to_string(),
            ..DataSourceConfig::default()
        }
    }

    #[test]
    fn sqlite_resolves_fixed_tables_and_absolutized_file() {
        let ds = Datasource::resolve(&sqlite_cfg(), Path::new("C:\\server\\plugins")).unwrap();
        assert_eq!(ds.backend, Backend::Sqlite);
        assert_eq!(
            ds.tables,
            TableNames {
                state: "trchat_player_state".to_string(),
                channels: "trchat_player_channels".to_string(),
                ignored: "trchat_player_ignored".to_string(),
                preferences: "trchat_player_preferences".to_string(),
            }
        );
        // relative File + folder → normalized absolute (Mod :68).
        assert_eq!(ds.sqlite_file, PathBuf::from("C:\\server\\plugins\\data.db"));
    }

    #[test]
    fn sqlite_respects_absolute_and_relative_file() {
        let mut cfg = sqlite_cfg();
        cfg.sqlite_file = "D:\\db\\trchat.db".to_string();
        let ds = Datasource::resolve(&cfg, Path::new("C:\\server\\plugins")).unwrap();
        assert_eq!(
            ds.sqlite_file,
            PathBuf::from("D:\\db\\trchat.db"),
            "absolute File stays as-is"
        );

        cfg.sqlite_file = "data/../nested.db".to_string();
        let ds = Datasource::resolve(&cfg, Path::new("C:\\server\\plugins")).unwrap();
        assert_eq!(
            ds.sqlite_file,
            PathBuf::from("C:\\server\\plugins\\nested.db"),
            ".. is resolved lexically"
        );
    }

    #[test]
    fn network_types_are_rejected() {
        for data_type in ["MySQL", "MariaDB", "PostgreSQL", "Postgres"] {
            let err = Datasource::resolve(
                &DataSourceConfig {
                    data_type: data_type.to_string(),
                    ..DataSourceConfig::default()
                },
                Path::new("C:\\server"),
            )
            .unwrap_err();
            assert!(
                err.contains("SQLite") && err.contains(data_type),
                "{data_type}: err = {err}"
            );
        }
    }

    #[test]
    fn unsupported_type_is_rejected_with_reason() {
        let err = Datasource::resolve(
            &DataSourceConfig {
                data_type: "JDBC".to_string(),
                ..DataSourceConfig::default()
            },
            Path::new("C:\\server"),
        )
        .unwrap_err();
        assert!(err.contains("JDBC"), "err = {err}");
    }

    #[test]
    fn ddl_text_matches_mod_verbatim() {
        let ds = Datasource::resolve(&sqlite_cfg(), Path::new("C:\\server")).unwrap();
        let ddl = ds.ddl();
        assert_eq!(ddl.len(), 4);
        assert_eq!(
            ddl[0],
            "CREATE TABLE IF NOT EXISTS trchat_player_state (\n\
             uuid VARCHAR(36) PRIMARY KEY,\n\
             player_name VARCHAR(64) NOT NULL,\n\
             mute_until BIGINT NOT NULL DEFAULT 0,\n\
             mute_reason VARCHAR(512) NOT NULL DEFAULT '',\n\
             shadow_muted INTEGER NOT NULL DEFAULT 0,\n\
             private_spy INTEGER NOT NULL DEFAULT 0\n\
             )"
        );
        assert_eq!(
            ddl[1],
            "CREATE TABLE IF NOT EXISTS trchat_player_channels (\n\
             uuid VARCHAR(36) NOT NULL,\n\
             channel_id VARCHAR(128) NOT NULL,\n\
             is_active INTEGER NOT NULL DEFAULT 0,\n\
             PRIMARY KEY (uuid, channel_id)\n\
             )"
        );
        assert_eq!(
            ddl[2],
            "CREATE TABLE IF NOT EXISTS trchat_player_ignored (\n\
             uuid VARCHAR(36) NOT NULL,\n\
             ignored_uuid VARCHAR(36) NOT NULL,\n\
             ignored_name VARCHAR(64) NOT NULL,\n\
             PRIMARY KEY (uuid, ignored_uuid)\n\
             )"
        );
        assert_eq!(
            ddl[3],
            "CREATE TABLE IF NOT EXISTS trchat_player_preferences (\n\
             uuid VARCHAR(36) PRIMARY KEY,\n\
             chat_color VARCHAR(32) NOT NULL DEFAULT ''\n\
             )"
        );
    }

    #[test]
    fn crud_sql_matches_mod_verbatim() {
        let ds = Datasource::resolve(&sqlite_cfg(), Path::new("C:\\server")).unwrap();
        let t = &ds.tables;
        assert_eq!(
            ds.load_state_sql(),
            format!("SELECT mute_until,mute_reason,shadow_muted,private_spy FROM {} WHERE uuid=?", t.state)
        );
        assert_eq!(
            ds.save_state_sql(),
            (
                format!("UPDATE {} SET player_name=?,mute_until=?,mute_reason=?,shadow_muted=?,private_spy=? WHERE uuid=?", t.state),
                format!("INSERT INTO {} (uuid,player_name,mute_until,mute_reason,shadow_muted,private_spy) VALUES (?,?,?,?,?,?)", t.state),
            )
        );
        assert_eq!(
            ds.load_membership_sql(),
            format!("SELECT channel_id,is_active FROM {} WHERE uuid=?", t.channels)
        );
        assert_eq!(
            ds.save_membership_sql(),
            (
                format!("DELETE FROM {} WHERE uuid=?", t.channels),
                format!("INSERT INTO {} (uuid,channel_id,is_active) VALUES (?,?,?)", t.channels),
            )
        );
        assert_eq!(
            ds.load_ignored_sql(),
            format!("SELECT ignored_uuid,ignored_name FROM {} WHERE uuid=?", t.ignored)
        );
        assert_eq!(
            ds.save_ignored_sql(),
            (
                format!("DELETE FROM {} WHERE uuid=?", t.ignored),
                format!("INSERT INTO {} (uuid,ignored_uuid,ignored_name) VALUES (?,?,?)", t.ignored),
            )
        );
        assert_eq!(
            ds.load_chat_color_sql(),
            format!("SELECT chat_color FROM {} WHERE uuid=?", t.preferences)
        );
        assert_eq!(
            ds.save_preferences_sql(),
            (
                format!("UPDATE {} SET chat_color=? WHERE uuid=?", t.preferences),
                format!("INSERT INTO {} (uuid,chat_color) VALUES (?,?)", t.preferences),
            )
        );
    }
}
