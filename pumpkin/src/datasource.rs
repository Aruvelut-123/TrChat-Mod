//! `datasource.yml` — where moderation / ignore state survives.
//!
//! Ports the configuration branch of `data/PlayerDataStore.java` `initialize`
//! (fact spec: `docs/spec/data-redis-update.md` §1.1-§1.4): the `Type` switch,
//! the JDBC URLs, the table-name derivation and the exact `CREATE TABLE` /
//! `SELECT` / `UPDATE` / `INSERT` / `DELETE` SQL texts the Mod runs against the
//! four tables (`player_state`, `player_channels`, `player_ignored`,
//! `player_preferences`).
//!
//! The WASM sandbox has no JDBC driver and cannot open a database connection
//! (see the crate docs and `docs/spec/data-redis-update.md` §7 for the probe
//! that ruled out an embedded pure-Rust engine), so this module is the
//! **semantics layer** only: it resolves the configuration and generates the
//! SQL. The storage layer for this port is the file-backed store in
//! [`crate::playerdata`] (`PlayerStore`), which persists the same field set the
//! four tables hold — recorded as a porting deviation.
//!
//! `Type` values other than SQLite/Local, MySQL, MariaDB and PostgreSQL are
//! rejected by [`Datasource::resolve`]: the Mod's fallback JDBC branch itself is
//! broken (spec §7 conclusion 9 — `ignoredTable` / `preferenceTable` stay `null`
//! and the save paths throw `NPE`), so there is nothing to mirror.

use std::path::{Component, Path, PathBuf};

use crate::config::{DataSourceConfig, JdbcDatabase, NetworkDatabase};

/// The data-source backends the Mod's `Type` switch can land on
/// (`PlayerDataStore.java:64-93`). Unsupported types surface as
/// [`Datasource::resolve`] errors instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// `Type: SQLite` / `Type: Local` — `jdbc:sqlite:<file>`.
    Sqlite,
    /// `Type: MySQL` — `jdbc:mysql://host:port/db`.
    Mysql,
    /// `Type: MariaDB` — `jdbc:mariadb://host:port/db`.
    Mariadb,
    /// `Type: PostgreSQL` / `Type: Postgres` — `jdbc:postgresql://host:port/db`.
    Postgresql,
}

/// The four table names (`PlayerDataStore.java:73-76`, `:152-156`).
///
/// SQLite uses the fixed Mod names; the network backends derive them as
/// `safeIdentifier(prefix + suffix)` with the default prefix `trchat_`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // spec-locked SQL semantics; see `docs/spec` §1.6
pub struct TableNames {
    pub state: String,
    pub channels: String,
    pub ignored: String,
    pub preferences: String,
}

/// A resolved data source: the backend, the table names and the connection
/// parameters the Mod builds. The file-backed [`crate::playerdata::PlayerStore`]
/// is the execution layer for this port.
#[derive(Debug, Clone)]
#[allow(dead_code)] // spec-locked connection semantics; file store executes today
pub struct Datasource {
    pub backend: Backend,
    pub tables: TableNames,
    /// Absolute SQLite database path (`PlayerDataStore.java:68`
    /// `folder.resolve(configured).normalize()`), when [`Backend::Sqlite`].
    pub sqlite_file: Option<PathBuf>,
    /// JDBC URL for the network backends (`PlayerDataStore.java:147-148`).
    pub jdbc_url: Option<String>,
    #[allow(dead_code)] // connection credentials for a future engine; spec §7
    pub user: String,
    #[allow(dead_code)] // connection credentials for a future engine; spec §7
    pub password: String,
}

/// Default database name for the network backends (`:145`).
pub const DEFAULT_DATABASE: &str = "trchat";
/// Default table prefix (`:152`).
pub const DEFAULT_TABLE_PREFIX: &str = "trchat_";
/// Default host (`:143`).
pub const DEFAULT_HOST: &str = "127.0.0.1";

#[allow(dead_code)] // SQL semantics for the future engine; file store executes today
impl Datasource {
    /// Resolves `cfg` against the plugin data folder, mirroring
    /// `PlayerDataStore.initialize` `:64-93`.
    pub fn resolve(cfg: &DataSourceConfig, data_folder: &Path) -> Result<Datasource, String> {
        match cfg.kind().as_str() {
            "sqlite" | "local" => Ok(sqlite(cfg, data_folder)),
            "mysql" => Ok(network(
                "MySQL",
                "jdbc:mysql",
                3306,
                &cfg.mysql,
                &cfg.jdbc,
            )),
            "mariadb" => Ok(network(
                "MariaDB",
                "jdbc:mariadb",
                3306,
                &cfg.mariadb,
                &cfg.jdbc,
            )),
            "postgresql" | "postgres" => Ok(network(
                "PostgreSQL",
                "jdbc:postgresql",
                5432,
                &cfg.postgresql,
                &cfg.jdbc,
            )),
            other => Err(format!(
                "datasource Type '{other}': this port supports SQLite, MySQL, MariaDB and \
                 PostgreSQL only (the Mod's generic JDBC branch is itself broken — spec \
                 conclusion 9)"
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
        sqlite_file: Some(database),
        jdbc_url: None,
        user: String::new(),
        password: String::new(),
    }
}

/// One of the network branches (`configureNetworkDatabase`, `:135-158`).
fn network(
    _section: &str,
    scheme: &str,
    default_port: u16,
    database: &NetworkDatabase,
    jdbc: &JdbcDatabase,
) -> Datasource {
    let host = if database.host.is_empty() {
        DEFAULT_HOST
    } else {
        &database.host
    };
    let port = if database.port == 0 {
        default_port.to_string()
    } else {
        database.port.to_string()
    };
    let name = if database.database.is_empty() {
        DEFAULT_DATABASE
    } else {
        &database.database
    };
    let parameters = if database.parameters.is_empty() {
        String::new()
    } else {
        format!("?{}", database.parameters)
    };
    let url = format!("{scheme}://{host}:{port}/{name}{parameters}");
    let prefix = if jdbc.table_prefix.is_empty() {
        DEFAULT_TABLE_PREFIX
    } else {
        jdbc.table_prefix.as_str()
    };
    Datasource {
        backend: network_backend(scheme),
        tables: TableNames {
            state: safe_identifier(&format!("{prefix}player_state")),
            channels: safe_identifier(&format!("{prefix}player_channels")),
            ignored: safe_identifier(&format!("{prefix}player_ignored")),
            preferences: safe_identifier(&format!("{prefix}player_preferences")),
        },
        sqlite_file: None,
        jdbc_url: Some(url),
        user: database.user.clone(),
        password: database.password.clone(),
    }
}

fn network_backend(scheme: &str) -> Backend {
    match scheme {
        "jdbc:mysql" => Backend::Mysql,
        "jdbc:mariadb" => Backend::Mariadb,
        _ => Backend::Postgresql,
    }
}

/// `safeIdentifier` (`PlayerDataStore.java:361-364`): the prefix-plus-suffix
/// table name must be `[A-Za-z0-9_]+`, otherwise the Mod throws.
pub fn safe_identifier(value: &str) -> String {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        panic!("Invalid table prefix: {value:?}");
    }
    value.to_string()
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

    fn mysql_cfg() -> DataSourceConfig {
        DataSourceConfig {
            data_type: "MySQL".to_string(),
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
        assert_eq!(
            ds.sqlite_file,
            Some(PathBuf::from("C:\\server\\plugins\\data.db"))
        );
        assert_eq!(ds.jdbc_url, None);
    }

    #[test]
    fn sqlite_respects_absolute_and_relative_file() {
        let mut cfg = sqlite_cfg();
        cfg.sqlite_file = "D:\\db\\trchat.db".to_string();
        let ds = Datasource::resolve(&cfg, Path::new("C:\\server\\plugins")).unwrap();
        assert_eq!(
            ds.sqlite_file,
            Some(PathBuf::from("D:\\db\\trchat.db")),
            "absolute File stays as-is"
        );

        cfg.sqlite_file = "data/../nested.db".to_string();
        let ds = Datasource::resolve(&cfg, Path::new("C:\\server\\plugins")).unwrap();
        assert_eq!(
            ds.sqlite_file,
            Some(PathBuf::from("C:\\server\\plugins\\nested.db")),
            ".. is resolved lexically"
        );
    }

    #[test]
    fn mysql_url_and_tables_match_mod() {
        let mut cfg = mysql_cfg();
        cfg.mysql.host = "db.example.com".to_string();
        cfg.mysql.port = 3307;
        cfg.mysql.database = "trchat2".to_string();
        cfg.mysql.parameters = "useSSL=false".to_string();
        cfg.jdbc.table_prefix = "tc_".to_string();
        let ds = Datasource::resolve(&cfg, Path::new("C:\\server")).unwrap();
        assert_eq!(ds.backend, Backend::Mysql);
        assert_eq!(
            ds.jdbc_url.as_deref(),
            Some("jdbc:mysql://db.example.com:3307/trchat2?useSSL=false")
        );
        assert_eq!(
            ds.tables,
            TableNames {
                state: "tc_player_state".to_string(),
                channels: "tc_player_channels".to_string(),
                ignored: "tc_player_ignored".to_string(),
                preferences: "tc_player_preferences".to_string(),
            }
        );
    }

    #[test]
    fn network_defaults_match_mod() {
        let ds = Datasource::resolve(&mysql_cfg(), Path::new("C:\\server")).unwrap();
        assert_eq!(
            ds.jdbc_url.as_deref(),
            Some("jdbc:mysql://127.0.0.1:3306/trchat")
        );
        let pg = Datasource::resolve(
            &DataSourceConfig {
                data_type: "Postgres".to_string(),
                ..DataSourceConfig::default()
            },
            Path::new("C:\\server"),
        )
        .unwrap();
        assert_eq!(pg.backend, Backend::Postgresql);
        assert_eq!(
            pg.jdbc_url.as_deref(),
            Some("jdbc:postgresql://127.0.0.1:5432/trchat")
        );
        let maria = Datasource::resolve(
            &DataSourceConfig {
                data_type: "MariaDB".to_string(),
                ..DataSourceConfig::default()
            },
            Path::new("C:\\server"),
        )
        .unwrap();
        assert_eq!(maria.backend, Backend::Mariadb);
        assert_eq!(
            maria.jdbc_url.as_deref(),
            Some("jdbc:mariadb://127.0.0.1:3306/trchat")
        );
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

    #[test]
    fn safe_identifier_accepts_alnum_underscore_only() {
        assert_eq!(safe_identifier("trchat_player_state"), "trchat_player_state");
        assert_eq!(safe_identifier("tc_player_channels"), "tc_player_channels");
        assert!(std::panic::catch_unwind(|| safe_identifier("trchat-player"))
            .is_err(), "hyphen must panic like the Mod's IllegalArgumentException");
    }
}
