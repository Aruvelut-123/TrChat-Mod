//! `PlaceholderResolver` — the `%token%` placeholder pipeline.
//!
//! Upstream: `placeholder/PlaceholderResolver.java:92-118` plus the
//! `server_*` / `player_*` key tables. The resolution *semantics* are exact:
//!
//! * syntax is `%([^%]+)%`, so an unclosed `%` stays literal,
//! * the token is `trim()`ed and ASCII-lowercased (`%PLAYER_NAME%` ==
//!   `%player_name%`, `% player_name %` == `%player_name%`),
//! * `server_` / `player_` route to their own table,
//! * **an unknown or unsupported token resolves to the empty string** and is
//!   deleted from the output — never left as literal `%…%`.
//!
//! The WIT surface exposes a subset of the Mod's data (no armour slots, no
//! `ServerMetrics` history, no `playerdata` directory scan), so unsupported
//! keys fall into the documented "unknown → empty" path instead of guessing.

use pumpkin_plugin_api::player::Player;
use pumpkin_plugin_api::Server;

use crate::config::TrChatConfig;

/// Resolves every placeholder in `input` against `player` as the message
/// subject (§1.1 note: the viewer argument is unused upstream — all
/// `player_*` values describe the subject).
pub fn resolve(input: &str, player: &Player, server: &Server, config: &TrChatConfig) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut last = 0usize;

    while let Some((i, c)) = chars.next() {
        if c != '%' {
            continue;
        }
        // `%([^%]+)%` — scan forward for the closing `%`; an unclosed or
        // immediately-closed `%` is not a placeholder and stays literal.
        let mut end: Option<usize> = None;
        for (j, n) in chars.by_ref() {
            if n == '%' {
                end = Some(j);
                break;
            }
        }
        let Some(close) = end else { break };
        let raw = &input[i + 1..close];
        if raw.is_empty() || raw.contains('%') {
            continue;
        }
        out.push_str(&input[last..i]);
        out.push_str(&resolve_token(raw.trim().to_ascii_lowercase().as_str(), player, server, config));
        last = close + 1;
    }
    out.push_str(&input[last..]);
    out
}

/// `resolveToken` (§1.2) — routes by prefix; anything else is empty.
fn resolve_token(token: &str, player: &Player, server: &Server, config: &TrChatConfig) -> String {
    if let Some(rest) = token.strip_prefix("server_") {
        return server_token(rest, player, server, config);
    }
    if let Some(rest) = token.strip_prefix("player_") {
        return player_token(rest, player, server);
    }
    String::new()
}

/// `server(token)` (§1.3/§1.4).
fn server_token(key: &str, player: &Player, server: &Server, config: &TrChatConfig) -> String {
    match key {
        "name" => config.server_name().to_string(),
        "online" => server.get_player_count().to_string(),
        "max_players" => server.get_max_players().to_string(),
        "has_whitelist" => yes_no(server.has_whitelist()),
        "tps" => format_tps(server.get_tps()),
        "motd" => server.get_motd(),
        "version" => server.get_sys_info().pumpkin_version,
        "ram_used" | "ram_free" | "ram_total" | "ram_max" => {
            // `Runtime` memory is not exposed to the sandbox; the JVM value has
            // no WASM equivalent, so these stay unsupported → empty (§1.1).
            String::new()
        }
        _ => {
            // §1.4 dynamic keys.
            if let Some(dim) = key.strip_prefix("online_") {
                return server_online_in_dimension(dim, server);
            }
            // `time_<pattern>` / `countdown_*` need a full date formatter and
            // `ZonedDateTime`; not available in the sandbox.
            let _ = player;
            String::new()
        }
    }
}

/// `%server_online_<dim>%` — the player count of the matching dimension, or
/// `-1` when no world matches (case-insensitive, `id.toString()`/`getPath()`).
fn server_online_in_dimension(dim: &str, server: &Server) -> String {
    for world in server.get_all_worlds() {
        let name = world.get_name();
        let id = world.get_id();
        // Accept the world name and both the namespaced id and its path,
        // mirroring `id.toString()` / `id.getPath()`.
        let path = id.rsplit('/').next().unwrap_or(&id).to_string();
        if name.eq_ignore_ascii_case(dim) || id.eq_ignore_ascii_case(dim) || path.eq_ignore_ascii_case(dim)
        {
            let count = server.get_player_count_in_world(world);
            return count.to_string();
        }
    }
    "-1".to_string()
}

/// `player(token, player)` (§1.5/§1.6) — only the subset backed by WIT data.
fn player_token(key: &str, player: &Player, server: &Server) -> String {
    match key {
        // Identity / display.
        "name" => player.get_name(),
        "uuid" => uuid_to_string(&player.get_id()),
        "gamemode" => game_mode_name(player.get_gamemode()),
        "ip" => player.get_ip(),
        "locale" => player.get_locale(),
        "online" => "yes".to_string(),
        "is_op" => yes_no(is_op(player)),

        // Position / world.
        "world" => player.get_world().get_name(),
        "x" => format_number(player.get_position().0),
        "y" => format_number(player.get_position().1),
        "z" => format_number(player.get_position().2),
        "yaw" => format_number(player.get_yaw() as f64),
        "pitch" => format_number(player.get_pitch() as f64),
        "direction" => cardinal_direction(player.get_yaw()),

        // Health / hunger.
        "health" => format_number(player.get_health() as f64),
        "health_rounded" => (player.get_health().round() as i64).to_string(),
        "max_health" => format_number(player.get_max_health() as f64),
        "max_health_rounded" => (player.get_max_health().round() as i64).to_string(),
        "food_level" => player.get_food_level().to_string(),
        "saturation" => format_number(player.get_saturation() as f64),
        "absorption" => format_number(player.get_absorption() as f64),
        "exp" => format_number(player.get_experience_progress() as f64),

        // Session / connection.
        "ping" => player.get_ping().to_string(),

        _ => {
            // §1.5 dynamic prefixes.
            if let Some(name) = key.strip_prefix("ping_") {
                return player_ping(name, server);
            }
            if let Some(node) = key.strip_prefix("has_permission_") {
                return yes_no(player.has_permission(node.trim_start_matches('*')));
            }
            String::new()
        }
    }
}

/// `%player_ping_<name>%` — `0` for an unknown/offline player.
fn player_ping(name: &str, server: &Server) -> String {
    match server.get_player_by_name(name) {
        Some(target) => target.get_ping().to_string(),
        None => "0".to_string(),
    }
}

fn is_op(player: &Player) -> bool {
    use pumpkin_plugin_api::permission::PermissionLevel;
    matches!(
        player.get_permission_level(),
        PermissionLevel::Two | PermissionLevel::Three | PermissionLevel::Four
    )
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_string()
}

fn game_mode_name(mode: pumpkin_plugin_api::wit::pumpkin::plugin::common::GameMode) -> String {
    use pumpkin_plugin_api::wit::pumpkin::plugin::common::GameMode;
    match mode {
        GameMode::Survival => "SURVIVAL",
        GameMode::Creative => "CREATIVE",
        GameMode::Adventure => "ADVENTURE",
        GameMode::Spectator => "SPECTATOR",
    }
    .to_string()
}

/// Formats a coordinate/value the way the Mod's `String.valueOf(double)` does:
/// whole numbers lose the `.0` (PlaceholderAPI renders `123.0` as `123`).
fn format_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `min(20, tps)` rounded to two decimals, matching the `%server_tps%` shape.
fn format_tps(tps: f64) -> String {
    format!("{:.2}", tps.clamp(0.0, 20.0))
}

/// 16-way yaw → cardinal direction (`player_direction`).
fn cardinal_direction(yaw: f32) -> String {
    // Minecraft yaw 0 = south (+Z), increasing **clockwise**; the 16 sectors
    // are 22.5° apart, starting at "South" (index 0). `NAMES[4]` is therefore
    // the due-West sector at yaw 90°.
    const NAMES: [&str; 16] = [
        "South", "South-Southwest", "Southwest", "West-Southwest", "West", "West-Northwest", "Northwest",
        "North-Northwest", "North", "North-Northeast", "Northeast", "East-Northeast", "East",
        "East-Southeast", "Southeast", "South-Southeast",
    ];
    let index = (((yaw % 360.0) + 360.0) % 360.0 / 22.5).round() as usize % 16;
    NAMES[index].to_string()
}

fn uuid_to_string(id: &pumpkin_plugin_api::wit::pumpkin::plugin::uuid::Uuid) -> String {
    format!("{:016x}-{:04x}-{:04x}-{:04x}-{:012x}", id.high, id.high >> 48, (id.high >> 32) & 0xffff, (id.low >> 48) & 0xffff, id.low & 0xffffffffffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_number_drops_trailing_zero() {
        assert_eq!(format_number(123.0), "123");
        assert_eq!(format_number(-7.0), "-7");
        assert_eq!(format_number(12.5), "12.5");
    }

    #[test]
    fn tps_is_clamped_and_rounded() {
        assert_eq!(format_tps(20.0), "20.00");
        assert_eq!(format_tps(19.987), "19.99");
        assert_eq!(format_tps(0.0), "0.00");
    }

    #[test]
    fn cardinal_direction_wraps_and_handles_negatives() {
        // 22.5° sectors starting at South (yaw 0), clockwise.
        assert_eq!(cardinal_direction(0.0), "South");
        assert_eq!(cardinal_direction(90.0), "West");
        assert_eq!(cardinal_direction(180.0), "North");
        assert_eq!(cardinal_direction(-90.0), "East");
        assert_eq!(cardinal_direction(45.0), "Southwest");
        // Out-of-range yaw wraps into the same 16 sectors.
        assert_eq!(cardinal_direction(450.0), "West");
    }

    #[test]
    fn yes_no_shape() {
        assert_eq!(yes_no(true), "yes");
        assert_eq!(yes_no(false), "no");
    }
}
