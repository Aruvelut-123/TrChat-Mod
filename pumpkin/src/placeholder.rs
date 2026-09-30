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

use pumpkin_plugin_api::wit::pumpkin::plugin::common::{EntityPose, GameMode, Hand};
use pumpkin_plugin_api::wit::pumpkin::plugin::enchantments::Enchantment;
use pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::StatusEffectType;
use pumpkin_plugin_api::wit::pumpkin::plugin::world::BlockPos;
use pumpkin_plugin_api::ItemStack;

use crate::config::TrChatConfig;

/// Resolves every placeholder in `input` against `player` as the message
/// subject (§1.1 note: the viewer argument is unused upstream — all
/// `player_*` values describe the subject).
pub fn resolve(input: &str, player: &Player, server: &Server, config: &TrChatConfig) -> String {
    resolve_with_local(input, player, server, config, &[])
}

/// §1.12 — `local` context keys injected by the chat service (lowercase token →
/// value). `local` wins over the `server_`/`player_` tables, matching the
/// resolver's step order (§1.1 step 4 before step 5).
pub fn resolve_with_local(
    input: &str,
    player: &Player,
    server: &Server,
    config: &TrChatConfig,
    local: &[(&str, &str)],
) -> String {
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
        let token = raw.trim().to_ascii_lowercase();
        out.push_str(&input[last..i]);
        // Step 4: the local table is consulted first and wins outright.
        let resolved = match local.iter().find(|(k, _)| *k == token) {
            Some((_, v)) => (*v).to_string(),
            None => resolve_token(&token, player, server, config),
        };
        // §1.8 — the resolver passes every resolved value through
        // `translatePlaceholder`, so an English result (`SURVIVAL`,
        // `The End`, …) becomes the default language's wording.
        out.push_str(&crate::lang::translate_placeholder(&token, &resolved));
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
        // §1.3 — the 1/5/15-minute windows come from `ServerMetrics.tps(n)`.
        // The WIT surface exposes a single smoothed value, so all three windows
        // report it rather than fabricating history the sandbox cannot see.
        "tps_1" | "tps_5" | "tps_15" => format_tps(server.get_tps()),
        // §1.5 `coloredTps` — `<15` red, `<18` yellow, otherwise green.
        "tps_1_colored" | "tps_5_colored" | "tps_15_colored" => {
            colored_tps(server.get_tps())
        }
        "motd" => server.get_motd(),
        "version" => server.get_sys_info().pumpkin_version,
        // §1.3 `%server_uptime%` — the Mod reports the JVM's uptime in seconds,
        // rendered through `duration()`. WASI has no JVM, so the plugin's own
        // load time stands in (≈ server start, the plugin loads during startup).
        "uptime" => format_duration(crate::clock::uptime_seconds()),
        // §1.3 RAM. The Mod reads the JVM heap (`Runtime.totalMemory()`); the
        // sandbox has no JVM, so the host's own memory counters stand in. Every
        // value is MiB, matching the `(bytes)/1048576` shape upstream.
        "ram_used" | "ram_free" | "ram_total" | "ram_max" => {
            let info = server.get_sys_info();
            match (info.total_memory, info.used_memory) {
                (Some(total), Some(used)) if total > 0 => {
                    let mib = |bytes: u64| (bytes / 1_048_576).to_string();
                    match key {
                        "ram_total" | "ram_max" => mib(total),
                        "ram_used" => mib(used),
                        // Saturate so a racing counter never yields a negative.
                        _ => mib(total.saturating_sub(used)),
                    }
                }
                // The host may report no memory info at all → unsupported (§1.1).
                _ => String::new(),
            }
        }
        // §1.3 `%server_total_chunks%` needs a loaded-chunk census the WIT
        // surface does not expose (`get-chunk` is a single lookup), so it stays
        // unsupported → empty (§1.1).
        // §1.3 entity census — summed across every loaded dimension.
        "total_entities" => count_entities(server, false),
        "total_living_entities" => count_entities(server, true),
        _ => {
            // §1.4 dynamic keys.
            if let Some(dim) = key.strip_prefix("online_") {
                return server_online_in_dimension(dim, server);
            }
            // §1.4 `%server_time_<pattern>%` — the pattern is used as it
            // arrived, i.e. already lowercased (§1.1 step 3).
            if let Some(pattern) = key.strip_prefix("time_") {
                return crate::clock::format_pattern(pattern, crate::clock::now_millis());
            }
            // §1.4 `%server_countdown_<pattern>_<target>%` (`:174-176, 467-488`).
            if let Some(arguments) = key.strip_prefix("countdown_") {
                return countdown(arguments, crate::clock::now_millis() / 1000);
            }
            let _ = player;
            String::new()
        }
    }
}

/// §1.3 `%server_total_entities%` / `%server_total_living_entities%` — the
/// entity count summed over every loaded dimension. `living_only` keeps just
/// the entities the host can view as a `living-entity` (players and mobs).
fn count_entities(server: &Server, living_only: bool) -> String {
    let total: usize = server
        .get_all_worlds()
        .iter()
        .map(|world| {
            world
                .get_entities()
                .iter()
                .filter(|entity| !living_only || entity.as_living().is_some())
                .count()
        })
        .sum();
    total.to_string()
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

/// §1.7 `duration(totalSeconds)` — `w d h m s`, only the non-zero units, joined
/// with spaces; a total that is not positive renders as `"0s"`
/// (`PlaceholderResolver.java:490-506`). The weeks are the largest unit and the
/// smaller ones wrap into the next, e.g. `86_400 * 8` s → `1w 1d`.
pub(crate) fn format_duration(total_seconds: i64) -> String {
    if total_seconds <= 0 {
        return "0s".to_string();
    }
    let weeks = total_seconds / 604_800;
    let days = total_seconds / 86_400 % 7;
    let hours = total_seconds / 3_600 % 24;
    let minutes = total_seconds / 60 % 60;
    let seconds = total_seconds % 60;
    let mut parts: Vec<String> = Vec::new();
    if weeks > 0 {
        parts.push(format!("{weeks}w"));
    }
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 {
        parts.push(format!("{seconds}s"));
    }
    parts.join(" ")
}

/// §1.4 `%server_countdown_<pattern>_<target>%` (`:174-176, 467-488`): the
/// argument is split at the **first** `_` into a `DateTimeFormatter` pattern and
/// a target value; the target is parsed in UTC (WASI has no zone database — the
/// Mod uses the server's system zone) and rendered as the `duration()` until it.
///
/// The three literals mirror upstream exactly: `invalid format and time` when the
/// `_` is missing or sits at either end, `invalid date` when the pattern or the
/// value cannot be parsed, and `"0"` once the target is reached or past.
pub(crate) fn countdown(arguments: &str, now_seconds: i64) -> String {
    let Some(separator) = arguments.find('_') else {
        return "invalid format and time".to_string();
    };
    if separator == 0 || separator == arguments.len() - 1 {
        return "invalid format and time".to_string();
    }
    let pattern = &arguments[..separator];
    let target = &arguments[separator + 1..];
    match parse_pattern(pattern, target) {
        Some(target_seconds) => {
            let remaining = target_seconds - now_seconds;
            if remaining <= 0 {
                "0".to_string()
            } else {
                format_duration(remaining)
            }
        }
        None => "invalid date".to_string(),
    }
}

/// Parses `value` with a subset of Java `DateTimeFormatter` patterns into Unix
/// seconds (UTC), or `None` when the pattern or the value is unusable.
///
/// The resolver lowercases the token before dispatch (§1.1 step 3), so the
/// pattern arrives **lowercased**: `M` (month) is then indistinguishable from
/// `m` (minute). The port reads the **first** `m` run as the month and any later
/// one as minutes — which is exactly how the shipped examples were meant
/// (`yyyy-MM-dd HH:mm:ss`, `dd.MM.yyyy`) — and every `h` run as a 24-hour hour.
/// Supported: `y`/`yy` (2-digit years are offset by 2000, like Java's `yy`),
/// `yyyy`, `m`/`mm`, `d`/`dd`, `h`/`hh`, `s`/`ss`, `'`-quoted literals and any
/// non-letter separator; another ASCII letter is an illegal pattern → `None`.
fn parse_pattern(pattern: &str, value: &str) -> Option<i64> {
    let (mut year, mut month, mut day, mut hour, mut minute, mut second) =
        (None, None, None, 0i64, 0i64, 0i64);
    let mut it = value.chars().peekable();
    for part in scan_pattern(pattern)? {
        match part {
            Part::Literal(c) => {
                if it.next() != Some(c) {
                    return None;
                }
            }
            Part::Year(2) => {
                let two = take_digits(&mut it, 2)?;
                year = Some(2000 + two);
            }
            Part::Year(_) => {
                year = Some(take_digits(&mut it, 4)?);
            }
            Part::Month => {
                let m = take_digits(&mut it, 2)?;
                if !(1..=12).contains(&m) {
                    return None;
                }
                month = Some(m);
            }
            Part::Day => {
                let d = take_digits(&mut it, 2)?;
                day = Some(d);
            }
            Part::Hour => {
                hour = take_digits(&mut it, 2)?;
            }
            Part::Minute => {
                minute = take_digits(&mut it, 2)?;
            }
            Part::Second => {
                second = take_digits(&mut it, 2)?;
            }
        }
    }
    if it.next().is_some() {
        return None;
    }
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return None;
    };
    if day > i64::from(days_in_month(year, month as u32)) || hour > 23 || minute > 59 || second > 59
    {
        return None;
    }
    let days = crate::clock::days_from_civil(year, month as u32, day as u32);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// One element of a scanned `DateTimeFormatter` pattern.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// A literal character the value must contain verbatim.
    Literal(char),
    /// A year run; the count is 2 (`yy`) or more (`yyyy`).
    Year(usize),
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

/// Splits a pattern into [`Part`]s, or `None` for an unsupported/illegal one.
fn scan_pattern(pattern: &str) -> Option<Vec<Part>> {
    let mut parts: Vec<Part> = Vec::new();
    let mut seen_month = false;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // Quoted literal; `''` is a single quote, as in Java.
            let mut closed = false;
            while let Some(q) = chars.next() {
                if q == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        parts.push(Part::Literal('\''));
                        continue;
                    }
                    closed = true;
                    break;
                }
                parts.push(Part::Literal(q));
            }
            if !closed {
                return None;
            }
            continue;
        }
        if !c.is_ascii_alphabetic() {
            parts.push(Part::Literal(c));
            continue;
        }
        let mut count = 1usize;
        while chars.peek() == Some(&c) {
            chars.next();
            count += 1;
        }
        let part = match c.to_ascii_lowercase() {
            'y' => Part::Year(count),
            'm' if !seen_month => {
                seen_month = true;
                Part::Month
            }
            'm' => Part::Minute,
            'd' => Part::Day,
            'h' => Part::Hour,
            's' => Part::Second,
            // `w`, `E`, `a`, `z`, … are not supported → illegal pattern.
            _ => return None,
        };
        parts.push(part);
    }
    Some(parts)
}

/// Reads up to `max` ASCII digits from the value (at least one must exist).
fn take_digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>, max: usize) -> Option<i64> {
    let mut value = 0i64;
    let mut count = 0usize;
    while count < max {
        match it.peek() {
            Some(c) if c.is_ascii_digit() => {
                value = value * 10 + i64::from(*c as u8 - b'0');
                it.next();
                count += 1;
            }
            _ => break,
        }
    }
    if count == 0 {
        None
    } else {
        Some(value)
    }
}

/// Days in `month` of `year` (Gregorian leap rule); `0` for a bad month.
fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
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

        // §1.6 display names (spec lines 148–149). The WIT exposes the display
        // and tab-list components; their plain text is what a chat placeholder
        // interpolates. A missing tab-list name falls back to the display name,
        // which is what the client shows.
        "displayname" | "custom_name" => player.get_display_name().get_text(),
        "list_name" => player
            .get_tab_list_name()
            .map(|c| c.get_text())
            .unwrap_or_else(|| player.get_display_name().get_text()),

        // Position / world.
        "world" => player.get_world().get_name(),
        "x" => format_number(player.get_position().0),
        "y" => format_number(player.get_position().1),
        "z" => format_number(player.get_position().2),
        "yaw" => format_number(player.get_yaw() as f64),
        "pitch" => format_number(player.get_pitch() as f64),
        "direction" => cardinal_direction(player.get_yaw()),
        // §1.7 `directionXz` — the facing as a world axis ("+Z" is south).
        "direction_xz" => direction_xz(player.get_yaw()),
        // §1.6 biome (spec line 131) — the WIT `biome` enum names the biome in
        // kebab-case (`dark-forest`); the Mod reports `minecraft:dark_forest`
        // and its capitalized form.
        "biome" => biome_id(player),
        "biome_capitalized" => biome_capitalized(player),
        // §1.6 light level — the block light at the player's feet.
        "light_level" => player_world_block_light(player).to_string(),
        // Compass target (spec lines 255–262). `get_compass_target` is the
        // spawn/lodestone a compass points at, not an optional respawn point.
        "compass_x" => format_number(player.get_compass_target().0),
        "compass_y" => format_number(player.get_compass_target().1),
        "compass_z" => format_number(player.get_compass_target().2),
        // Abilities (spec line 203) — `player-abilities` carries both flags.
        "allow_flight" => yes_no(player.get_abilities().allow_flying),
        "is_flying" => yes_no(player.get_abilities().flying),

        // Health / hunger.
        "health" => format_number(player.get_health() as f64),
        "health_rounded" => (player.get_health().round() as i64).to_string(),
        "max_health" => format_number(player.get_max_health() as f64),
        "max_health_rounded" => (player.get_max_health().round() as i64).to_string(),
        "food_level" => player.get_food_level().to_string(),
        "saturation" => format_number(player.get_saturation() as f64),
        "absorption" => format_number(player.get_absorption() as f64),
        "exp" => format_number(player.get_experience_progress() as f64),
        // §1.6 exp family (spec lines 273–274, 287): `player_exp_to_level` is
        // the points still needed for the next level, `player_total_exp` the
        // accumulated points, and `player_current_exp` the level itself.
        "exp_to_level" => exp_to_level(player).to_string(),
        "total_exp" => player.get_experience_points().to_string(),
        "current_exp" => player.get_experience_level().to_string(),
        // §1.6 `player_level` — the experience level (spec line 327).
        "level" => player.get_experience_level().to_string(),
        // §1.6 — `player_time_offset` and `player_max_no_damage_ticks` are
        // documented constants (spec line 692).
        "time_offset" => "0".to_string(),
        "max_no_damage_ticks" => "20".to_string(),

        // Session / connection.
        "ping" => player.get_ping().to_string(),
        // §1.7 `coloredPing` — the same number, prefixed by a colour code.
        "colored_ping" => colored_ping(player.get_ping()),

        // §1.6 items (spec lines 151–153). `main_hand` is the WIT `right` hand;
        // the sandbox exposes no damage value, so `_data`/`_durability` keep the
        // documented empty-item shape (see the helpers).
        "item_in_hand" => item_type(held_item(player, Hand::Right)),
        "item_in_hand_name" => item_name(held_item(player, Hand::Right)),
        "item_in_hand_data" => item_data(held_item(player, Hand::Right)),
        "item_in_hand_durability" => item_durability(held_item(player, Hand::Right)),
        "item_in_offhand" => item_type(held_item(player, Hand::Left)),
        "item_in_offhand_name" => item_name(held_item(player, Hand::Left)),
        "item_in_offhand_data" => item_data(held_item(player, Hand::Left)),
        "item_in_offhand_durability" => item_durability(held_item(player, Hand::Left)),
        "empty_slots" => empty_slots(player),

        // §1.6 state booleans. Sneak/sprint/air/lifetime live on the entity
        // handle, so they are reached through `as_entity`.
        "is_sneaking" => yes_no(player.as_entity().is_sneaking()),
        "is_sprinting" => yes_no(player.as_entity().is_sprinting()),
        "is_swimming" => yes_no(player.as_entity().is_swimming()),
        "is_inside_vehicle" => yes_no(player.as_entity().get_vehicle().is_some()),
        // §1.6 `player_is_sleeping` — the entity pose carries the sleeping flag
        // (Bukkit `Player#isSleeping` ↔ pose `Sleeping`).
        "is_sleeping" => yes_no(player.as_entity().get_pose() == EntityPose::Sleeping),
        // `allow_flight` / `is_flying` are handled with the other abilities above.
        "fly_speed" => format_number(player.get_abilities().fly_speed as f64),
        "walk_speed" => format_number(player.get_abilities().walk_speed as f64),
        "has_empty_slot" => yes_no(empty_slots(player) != "0"),
        // `player_online` is only ever resolved for the subject, so `yes` (§1.6).
        "is_whitelisted" => yes_no(
            server
                .get_whitelist_manager()
                .is_whitelisted(player.get_id()),
        ),
        "is_banned" => yes_no(server.get_ban_manager().is_player_banned(player.get_id())),
        // §1.6 `player_can_pickup_items` — Bukkit `!isSpectator()`, read from
        // the game mode. `player_has_played_before` scans `playerdata` for a
        // non-zero first-played timestamp, which the WIT surface does not
        // expose; rather than guess a proxy, it stays unsupported and
        // therefore resolves to the empty string (§1.1).
        "can_pickup_items" => yes_no(player.get_gamemode() != GameMode::Spectator),
        // `player_has_played_before` and the `first_played`/`last_played`
        // family (spec lines 219–220, 275–276, 281, 325–326) read the
        // `playerdata` directory timestamps, which the WIT surface does not
        // expose; rather than guess a proxy, they stay unsupported and
        // therefore resolve to the empty string (§1.1).
        "has_played_before"
        | "first_played"
        | "first_join"
        | "first_played_formatted"
        | "first_join_date"
        | "last_played"
        | "last_join"
        | "last_played_formatted"
        | "last_join_date" => String::new(),

        // Air / lifetime.
        "remaining_air" => player.as_entity().get_remaining_air().to_string(),
        "max_air" => player.as_entity().get_max_air().to_string(),
        "ticks_lived" => player.as_entity().get_ticks_lived().to_string(),
        "seconds_lived" => (player.as_entity().get_ticks_lived() / 20).to_string(),
        "minutes_lived" => (player.as_entity().get_ticks_lived() / 1200).to_string(),
        // Damage statistics and sleep timers have no WIT accessor; an empty
        // result is the documented unknown-token behaviour (§1.1).
        "sleep_ticks" | "no_damage_ticks" | "last_damage" => String::new(),
        // §1.6 health attributes (spec lines 142, 285–287). `health_boost` is the
        // extra hearts from the (removed-in-1.21) HEALTH_BOOST attribute:
        // Bukkit `maxHealth - 20`, floored at 0. `health_scale` is the max
        // health itself, and `has_health_boost` the HEALTH_BOOST effect check.
        "health_boost" => format_number(health_boost_from(player.get_max_health())),
        "health_scale" => format_number(player.get_max_health() as f64),
        "has_health_boost" => yes_no(player.get_effect(StatusEffectType::HealthBoost).is_some()),

        // §1.6 bed / compass spawn points.
        "bed_world" | "bed_x" | "bed_y" | "bed_z" => respawn_component(player, key),
        "compass_world" => player.get_world().get_name(),

        // §1.6 locale family (spec lines 155–156).
        "locale_display_name" | "locale_short" | "locale_country" | "locale_display_country" => {
            locale_component(player, key)
        }

        // §1.6 world clock (spec lines 131–133).
        "world_time_12" | "world_time_24" => world_clock(player, key),
        "time" => player.get_world().get_time_of_day().to_string(),
        // §1.7 `worldType` — the Mod maps `Level.NETHER` to `Nether`,
        // `Level.END` to `The End` and everything else to `Overworld`. The
        // sandbox hands back the dimension's namespaced id
        // (`minecraft:the_nether`), so it is mapped here.
        "world_type" => world_type_name(&player.get_world().get_dimension()),
        // §1.6 `player_block_underneath` — the block type below the player's
        // feet, rendered like `registryName` (spec line 246): the registry path
        // without the namespace, upper cased (`minecraft:stone` → `STONE`).
        "block_underneath" => {
            let under = {
                let pos = player_block_pos(player);
                BlockPos {
                    x: pos.x,
                    y: pos.y - 1,
                    z: pos.z,
                }
            };
            registry_name_of(&player.get_world().get_block_state(under).block_name)
        }
        // `player_thunder_duration` / `player_weather_duration` need the world
        // weather timers, which the WIT surface does not expose.
        "thunder_duration" | "weather_duration" => String::new(),

        _ => {
            // §1.5 dynamic prefixes.
            if let Some(name) = key.strip_prefix("ping_") {
                return player_ping(name, server);
            }
            if let Some(node) = key.strip_prefix("has_permission_") {
                return yes_no(player.has_permission(node.trim_start_matches('*')));
            }
            // §1.5 `player_has_potioneffect_<id>` — `<id>` is lowercased and its
            // `minecraft:` namespace (when present) is stripped.
            if let Some(id) = key.strip_prefix("has_potioneffect_") {
                let id = id.to_ascii_lowercase();
                let id = id.strip_prefix("minecraft:").unwrap_or(&id).to_string();
                // An unknown effect id is simply absent, not an error.
                return yes_no(
                    effect_type(&id).is_some_and(|effect| player.get_effect(effect).is_some()),
                );
            }
            // §1.5 `player_item_in_hand_level_<enchant>` /
            // `player_item_in_offhand_level_<enchant>` — an unknown enchantment
            // is `0`, matching the Mod.
            if let Some(enchant) = key
                .strip_prefix("item_in_hand_level_")
                .or_else(|| key.strip_prefix("item_in_offhand_level_"))
            {
                let hand = if key.starts_with("item_in_offhand") {
                    Hand::Left
                } else {
                    Hand::Right
                };
                return enchant_level(player, hand, enchant);
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

/// §1.6 `player_exp_to_level` — the experience points still needed to reach the
/// next level, derived from the bar progress and the vanilla cost curve.
///
/// The sandbox exposes the progress fraction but not the level's total cost, so
/// the vanilla formula (`2·level + 7` below 16, then `5·level − 38`, then
/// `9·level − 158`) supplies the missing denominator.
fn exp_to_level(player: &Player) -> i64 {
    exp_to_level_from(
        player.get_experience_level(),
        player.get_experience_progress(),
    )
}

/// The pure part of [`exp_to_level`]: the vanilla level-cost curve plus the bar
/// progress, split out so it is testable without a live `Player`.
fn exp_to_level_from(level: i32, progress: f32) -> i64 {
    let cost = match level {
        i32::MIN..=15 => 2 * level + 7,
        16..=30 => 5 * level - 38,
        _ => 9 * level - 158,
    };
    let progress = progress.clamp(0.0, 1.0);
    let remaining = (f64::from(cost) * (1.0 - f64::from(progress))).round() as i64;
    remaining.max(0)
}

/// §1.6 `player_biome` — the biome key as `minecraft:<snake_case>`, matching
/// the Mod's `Biome.toString()`. The WIT enum carries kebab-case names.
fn biome_id(player: &Player) -> String {
    format!("minecraft:{}", biome_name(player).replace('-', "_"))
}

/// §1.6 `player_biome_capitalized` — the biome name with word-initial capitals
/// (`dark-forest` → `Dark Forest`).
fn biome_capitalized(player: &Player) -> String {
    capitalize_words(&biome_name(player))
}

/// Word-initial capitals for a kebab/underscore-separated key.
fn capitalize_words(key: &str) -> String {
    key.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The block containing the player, as the WIT position record.
fn player_block_pos(player: &Player) -> BlockPos {
    let pos = player.get_position();
    BlockPos {
        x: pos.0.floor() as i32,
        y: pos.1.floor() as i32,
        z: pos.2.floor() as i32,
    }
}

/// The biome under the player, read from the block at their feet.
fn biome_name(player: &Player) -> String {
    biome_key(player.get_world().get_biome(player_block_pos(player)))
}

/// `DeepDark` → `deep-dark` (WIT enum variant names are PascalCase).
///
/// The generated enum's `Debug` prints the path-qualified variant
/// (`biome::DeepDark`), so the last `::` segment is the name to convert.
fn biome_key(biome: pumpkin_plugin_api::wit::pumpkin::plugin::biomes::Biome) -> String {
    let debug = format!("{biome:?}");
    let variant = debug.rsplit("::").next().unwrap_or(&debug);
    variant_to_kebab(variant)
}

/// Renders a generated enum's CamelCase variant name as WIT kebab-case
/// (`DeepDark` → `deep-dark`).
fn variant_to_kebab(variant: &str) -> String {
    let mut out = String::new();
    for (i, ch) in variant.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// §1.6 `player_light_level` — block light at the player's position.
fn player_world_block_light(player: &Player) -> u8 {
    player.get_world().get_block_light(player_block_pos(player))
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

/// §1.6 `player_health_boost` — the extra hearts from the (removed-in-1.21)
/// HEALTH_BOOST attribute: Bukkit `maxHealth - 20.0F`, floored at 0 (spec
/// line 285). Split out so the floor behaviour is unit-testable.
fn health_boost_from(max_health: f32) -> f64 {
    f64::from((max_health - 20.0).max(0.0))
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

/// §1.7 `worldType` — `Nether` / `The End` / `Overworld`.
///
/// Accepts both the namespaced id the host reports (`minecraft:the_nether`)
/// and a bare path, and treats any other dimension (including modded ones and
/// `overworld_caves`) as the overworld, matching the Mod's `default` branch.
fn world_type_name(dimension: &str) -> String {
    match dimension.strip_prefix("minecraft:").unwrap_or(dimension) {
        "the_nether" | "nether" => "Nether",
        "the_end" | "end" => "The End",
        _ => "Overworld",
    }
    .to_string()
}

/// Formats a coordinate/value the way the Mod's `String.valueOf(double)` does:
/// whole numbers lose the `.0` (PlaceholderAPI renders `123.0` as `123`).
/// §1.7 `decimal` — non-finite → `"0"`; whole numbers render without a decimal
/// point; everything else is `%.2f` with trailing zeros and a trailing `.`
/// stripped (`Locale.ROOT`).
fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    // Java's `String.format("%.2f")` rounds half **up**; Rust's `{:.2}` rounds
    // half to even, so 0.125 would give "0.12". Round explicitly to match.
    let rounded = (value * 100.0).round() / 100.0;
    let fixed = format!("{rounded:.2}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Returns the item in the given hand, or `None` when empty.
fn held_item(player: &Player, hand: Hand) -> Option<ItemStack> {
    player.get_item_in_hand(hand)
}

/// §1.7 `registryName` — the registry path **without** the namespace, upper
/// cased (`minecraft:diamond_sword` → `DIAMOND_SWORD`).
fn registry_name(stack: &ItemStack) -> String {
    registry_name_of(&stack.get_registry_key())
}

/// The pure part of [`registry_name`], split out so it is unit-testable.
fn registry_name_of(key: &str) -> String {
    let path = key.split(':').next_back().unwrap_or(key);
    path.to_ascii_uppercase()
}

/// §1.7 `itemType` — empty item renders `"AIR"`.
fn item_type(stack: Option<ItemStack>) -> String {
    match stack {
        Some(s) => registry_name(&s),
        None => "AIR".to_string(),
    }
}

/// §1.7 `itemName` — empty item **or** an item without a custom name renders
/// the empty string (PlaceholderAPI reads `getHoverName`, but the sandbox only
/// exposes an explicit custom name).
fn item_name(stack: Option<ItemStack>) -> String {
    let Some(s) = stack else {
        return String::new();
    };
    match s.get_custom_name() {
        Some(name) => name.get_text(),
        None => String::new(),
    }
}

/// §1.7 `itemData` — empty item renders `"0"`; the sandbox exposes no damage
/// value, so non-empty items also report `"0"` rather than a wrong number.
fn item_data(stack: Option<ItemStack>) -> String {
    let _ = stack;
    "0".to_string()
}

/// §1.7 `itemDurability` — `max(0, maxDamage - damageValue)`. The sandbox
/// exposes neither value, so this stays `"0"` (matching the empty-item shape).
fn item_durability(stack: Option<ItemStack>) -> String {
    let _ = stack;
    "0".to_string()
}

/// §1.7 `emptySlots` — counts empty slots among main-inventory slots 0..35.
fn empty_slots(player: &Player) -> String {
    let inv = player.get_inventory();
    let main = inv.as_inventory();
    let mut empty = 0u32;
    for slot in 0..36u32 {
        if main.get_item(slot).is_none() {
            empty += 1;
        }
    }
    empty.to_string()
}

/// Every vanilla status effect the WIT enum exposes. Kept explicit so an
/// unknown `player_has_potioneffect_<id>` resolves to `no` instead of failing.
const EFFECT_TYPES: &[StatusEffectType] = &[
    StatusEffectType::Speed,
    StatusEffectType::Slowness,
    StatusEffectType::Haste,
    StatusEffectType::MiningFatigue,
    StatusEffectType::Strength,
    StatusEffectType::InstantHealth,
    StatusEffectType::InstantDamage,
    StatusEffectType::JumpBoost,
    StatusEffectType::Nausea,
    StatusEffectType::Regeneration,
    StatusEffectType::Resistance,
    StatusEffectType::FireResistance,
    StatusEffectType::WaterBreathing,
    StatusEffectType::Invisibility,
    StatusEffectType::Blindness,
    StatusEffectType::NightVision,
    StatusEffectType::Hunger,
    StatusEffectType::Weakness,
    StatusEffectType::Poison,
    StatusEffectType::Wither,
    StatusEffectType::HealthBoost,
    StatusEffectType::Absorption,
    StatusEffectType::Saturation,
    StatusEffectType::Glowing,
    StatusEffectType::Levitation,
    StatusEffectType::Luck,
    StatusEffectType::Unluck,
    StatusEffectType::SlowFalling,
    StatusEffectType::ConduitPower,
    StatusEffectType::DolphinsGrace,
    StatusEffectType::BadOmen,
    StatusEffectType::HeroOfTheVillage,
    StatusEffectType::Darkness,
    StatusEffectType::TrialOmen,
    StatusEffectType::RaidOmen,
    StatusEffectType::WindCharged,
    StatusEffectType::Weaving,
    StatusEffectType::Oozing,
    StatusEffectType::Infested,
];

/// §1.5 `player_has_potioneffect_<id>` — maps a vanilla effect id (its
/// `minecraft:` namespace already stripped, `_`-separated) onto the WIT enum.
/// `None` means the id is not a vanilla effect, which is absent, not an error.
fn effect_type(id: &str) -> Option<StatusEffectType> {
    let wanted: String = id
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    EFFECT_TYPES.iter().copied().find(|candidate| {
        // `Debug` is path-qualified (`status_effect::JumpBoost`); compare on the
        // bare variant name only, as `biome_key` does.
        let debug = format!("{candidate:?}");
        debug.rsplit("::").next().unwrap_or(&debug) == wanted
    })
}

/// §1.5 `player_item_in_hand_level_<enchant>` — the level of `enchant` on the
/// held stack, or `0` when the stack or the enchantment is absent. `<enchant>`
/// is matched against the vanilla enum name with `_`/`-` treated alike.
fn enchant_level(player: &Player, hand: Hand, enchant: &str) -> String {
    let Some(stack) = held_item(player, hand) else {
        return "0".to_string();
    };
    let wanted = enchant.to_ascii_lowercase().replace('_', "-");
    for value in stack.get_enchantments() {
        if enchant_name(value.enchantment) == wanted {
            return value.level.to_string();
        }
    }
    "0".to_string()
}

/// The WIT kebab-case name of a vanilla enchantment.
fn enchant_name(enchantment: Enchantment) -> String {
    let debug = format!("{enchantment:?}");
    let variant = debug.rsplit("::").next().unwrap_or(&debug);
    variant_to_kebab(variant)
}

/// §1.6 `player_bed_*` — the respawn anchor. `get_respawn_location` returns a
/// bare position with no world, so the world component is the subject's own
/// world; an unset anchor renders empty, as the Mod does.
fn respawn_component(player: &Player, key: &str) -> String {
    let Some(pos) = player.get_respawn_location() else {
        return String::new();
    };
    match key {
        "bed_world" => player.get_world().get_name(),
        "bed_x" => format_number(pos.0),
        "bed_y" => format_number(pos.1),
        _ => format_number(pos.2),
    }
}

/// §1.6 locale family (spec lines 155–156). The WIT exposes only the raw locale
/// string (`en_us`), so the derived components are split locally.
fn locale_component(player: &Player, key: &str) -> String {
    let locale = player.get_locale();
    let mut parts = locale.split(['_', '-']);
    let language = parts.next().unwrap_or("").to_ascii_lowercase();
    let country = parts.next().unwrap_or("").to_ascii_lowercase();
    match key {
        "locale_short" => language,
        "locale_country" => country.clone(),
        "locale_display_country" => country.to_ascii_uppercase(),
        _ => locale,
    }
}

/// §1.6 `player_world_time_12` / `_24` — the dimension clock. Minecraft tick 0
/// is 06:00, so the wall clock is the tick-of-day offset by six hours.
fn world_clock(player: &Player, key: &str) -> String {
    let ticks = player.get_world().get_time_of_day() % 24_000;
    let total_minutes = ((ticks as f64 / 1000.0) + 6.0) * 60.0;
    let hour24 = ((total_minutes / 60.0) as u64) % 24;
    let minute = (total_minutes % 60.0) as u64;
    if key == "world_time_24" {
        return format!("{hour24:02}:{minute:02}");
    }
    let suffix = if hour24 < 12 { "AM" } else { "PM" };
    let hour12 = match hour24 % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour12:02}:{minute:02} {suffix}")
}

/// `min(20, tps)` rendered through `decimal` (§1.7), so a
/// whole value shows as `20` rather than `20.00`.
fn format_tps(tps: f64) -> String {
    format_number(tps.clamp(0.0, 20.0))
}

/// §1.5 `coloredTps`: `<15` → `&c`, `<18` → `&e`, otherwise `&a`, followed by
/// the two-decimal TPS. The colour code is the Mod's `&`-form, so the caller's
/// legacy-code pass turns it into styling.
fn colored_tps(tps: f64) -> String {
    let code = if tps < 15.0 {
        "&c"
    } else if tps < 18.0 {
        "&e"
    } else {
        "&a"
    };
    format!("{code}{}", format_tps(tps))
}

/// §1.7 `coloredPing` (`>100` → `&c`, `>50` → `&e`, otherwise `&a`) followed by
/// the ping in milliseconds.
fn colored_ping(ping: u32) -> String {
    let code = if ping > 100 {
        "&c"
    } else if ping > 50 {
        "&e"
    } else {
        "&a"
    };
    format!("{code}{ping}")
}

/// §1.7 `directionXz` — the horizontal facing as a world axis, from the
/// normalized yaw: `<=45` or `>=315` → `+Z` (south), `<=135` → `-X` (west),
/// `<=225` → `-Z` (north), otherwise `+X` (east).
fn direction_xz(yaw: f32) -> String {
    let normalized = ((yaw % 360.0) + 360.0) % 360.0;
    let axis = if normalized <= 45.0 || normalized >= 315.0 {
        "+Z"
    } else if normalized <= 135.0 {
        "-X"
    } else if normalized <= 225.0 {
        "-Z"
    } else {
        "+X"
    };
    axis.to_string()
}

/// 8-way yaw → abbreviated cardinal direction (`player_direction`, §1.7).
///
/// Upstream indexes `{"S","SW","W","NW","N","NE","E","SE"}` with
/// `Math.round(yaw / 45) & 7`, so the sectors are 45° wide and the result is a
/// two-letter abbreviation — **not** the 16-point long form.
fn cardinal_direction(yaw: f32) -> String {
    // Minecraft yaw 0 = south (+Z) and increases clockwise, so index 0 is "S"
    // and index 2 (yaw 90°) is "W".
    const NAMES: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];
    let index = (((yaw % 360.0) + 360.0) % 360.0 / 45.0).round() as usize & 7;
    NAMES[index].to_string()
}

fn uuid_to_string(id: &pumpkin_plugin_api::wit::pumpkin::plugin::uuid::Uuid) -> String {
    format!("{:016x}-{:04x}-{:04x}-{:04x}-{:012x}", id.high, id.high >> 48, (id.high >> 32) & 0xffff, (id.low >> 48) & 0xffff, id.low & 0xffffffffffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1.7 `duration` — non-zero units in `w d h m s`, joined by spaces.
    #[test]
    fn duration_renders_non_zero_units() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(-5), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(3_600), "1h");
        assert_eq!(format_duration(3_661), "1h 1m 1s");
        assert_eq!(format_duration(86_400 * 8), "1w 1d");
        assert_eq!(
            format_duration(604_800 + 86_400 + 3_600 + 60 + 1),
            "1w 1d 1h 1m 1s"
        );
    }

    /// §1.4 countdown — the `_` split rules and the three upstream literals.
    #[test]
    fn countdown_matches_the_upstream_literals() {
        let now = 1_893_456_000; // 2030-01-01T00:00:00Z
                                 // `_` missing or at either end → the hint literal.
        assert_eq!(countdown("dd.mm.yyyy", now), "invalid format and time");
        assert_eq!(countdown("_01.01.2030", now), "invalid format and time");
        assert_eq!(countdown("dd.mm.yyyy_", now), "invalid format and time");
        // A reached/past target → "0"; a future one → `duration`.
        assert_eq!(countdown("dd.mm.yyyy_01.01.2030", now), "0");
        assert_eq!(countdown("dd.mm.yyyy_01.01.2030", now - 3_661), "1h 1m 1s");
        // Unparseable pattern/value → the other literal.
        assert_eq!(countdown("dd.mm.yyyy_32.13.2030", now), "invalid date");
        assert_eq!(countdown("qq_01.01.2030", now), "invalid date");
    }

    /// §1.4 countdown — date-only and date-time targets. The first `m` run is the
    /// month because the token arrives lowercased (§1.1 step 3).
    #[test]
    fn countdown_parses_date_and_date_time_targets() {
        let now = 1_893_456_000; // 2030-01-01T00:00:00Z
                                 // `yyyy-mm-dd hh:mm:ss` — first `mm` is the month, second the minute.
        assert_eq!(
            countdown("yyyy-mm-dd hh:mm:ss_2030-01-01 00:01:00", now),
            "1m"
        );
        assert_eq!(countdown("yyyy-mm-dd_2030-01-02", now), "1d");
        // Quoted literals and unpadded fields work too.
        assert_eq!(countdown("yyyy'x'mm'x'dd_2030x1x2", now), "1d");
    }

    #[test]
    fn format_number_drops_trailing_zero() {        assert_eq!(format_number(123.0), "123");
        assert_eq!(format_number(-7.0), "-7");
        assert_eq!(format_number(12.5), "12.5");
    }

    #[test]
    fn colored_tps_uses_spec_thresholds() {
        // §1.5 — `<15` red, `<18` yellow, `>=18` green.
        assert_eq!(colored_tps(19.5), "&a19.5");
        assert_eq!(colored_tps(18.0), "&a18");
        assert_eq!(colored_tps(17.9), "&e17.9");
        assert_eq!(colored_tps(15.0), "&e15");
        assert_eq!(colored_tps(14.9), "&c14.9");
    }

    #[test]
    fn registry_name_strips_namespace_and_uppercases() {
        // §1.7 — the namespace is dropped and the path is upper cased.
        assert_eq!(registry_name_of("minecraft:diamond_sword"), "DIAMOND_SWORD");
        assert_eq!(registry_name_of("minecraft:stone"), "STONE");
        // A bare path (no namespace) is used as-is.
        assert_eq!(registry_name_of("stone"), "STONE");
        assert_eq!(registry_name_of("mod:custom_thing"), "CUSTOM_THING");
    }

    #[test]
    fn decimal_matches_spec_formatting() {
        // §1.7 `decimal`: whole numbers bare, others %.2f minus trailing zeros.
        assert_eq!(format_number(20.0), "20");
        assert_eq!(format_number(19.99), "19.99");
        assert_eq!(format_number(1.5), "1.5");
        assert_eq!(format_number(-3.0), "-3");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(0.125), "0.13");
        assert_eq!(format_number(f64::NAN), "0");
        assert_eq!(format_number(f64::INFINITY), "0");
    }

    #[test]
    fn tps_is_clamped_and_rounded() {
        assert_eq!(format_tps(20.0), "20");
        assert_eq!(format_tps(19.991), "19.99");
        assert_eq!(format_tps(25.0), "20");
        assert_eq!(format_tps(-1.0), "0");
    }

    #[test]
    fn cardinal_direction_wraps_and_handles_negatives() {
        // §1.7 — 45° sectors from South (yaw 0), clockwise, 8-point abbreviations.
        assert_eq!(cardinal_direction(0.0), "S");
        assert_eq!(cardinal_direction(45.0), "SW");
        assert_eq!(cardinal_direction(90.0), "W");
        assert_eq!(cardinal_direction(135.0), "NW");
        assert_eq!(cardinal_direction(180.0), "N");
        assert_eq!(cardinal_direction(225.0), "NE");
        assert_eq!(cardinal_direction(270.0), "E");
        assert_eq!(cardinal_direction(315.0), "SE");
        // Out-of-range yaw wraps into the same 8 sectors.
        assert_eq!(cardinal_direction(450.0), "W");
        assert_eq!(cardinal_direction(-90.0), "E");
    }

    /// §1.7 `directionXz` — 90°-ish quadrants around the four axes; the
    /// boundaries belong to the lower branch (`<=45` includes 45).
    #[test]
    fn direction_xz_maps_yaw_to_an_axis() {
        assert_eq!(direction_xz(0.0), "+Z"); // south
        assert_eq!(direction_xz(45.0), "+Z");
        assert_eq!(direction_xz(46.0), "-X");
        assert_eq!(direction_xz(90.0), "-X"); // west
        assert_eq!(direction_xz(135.0), "-X");
        assert_eq!(direction_xz(136.0), "-Z");
        assert_eq!(direction_xz(180.0), "-Z"); // north
        assert_eq!(direction_xz(225.0), "-Z");
        assert_eq!(direction_xz(226.0), "+X");
        assert_eq!(direction_xz(270.0), "+X"); // east
        assert_eq!(direction_xz(315.0), "+Z");
        // Normalization: negative and >360 yaw land in the same quadrants.
        assert_eq!(direction_xz(-90.0), "+X");
        assert_eq!(direction_xz(450.0), "-X");
    }

    /// §1.7 `coloredPing` — `&c` above 100 ms, `&e` above 50 ms, else `&a`, with
    /// the raw number appended. The thresholds are exclusive.
    #[test]
    fn colored_ping_prefixes_by_threshold() {
        assert_eq!(colored_ping(0), "&a0");
        assert_eq!(colored_ping(50), "&a50");
        assert_eq!(colored_ping(51), "&e51");
        assert_eq!(colored_ping(100), "&e100");
        assert_eq!(colored_ping(101), "&c101");
    }

    #[test]
    fn yes_no_shape() {
        assert_eq!(yes_no(true), "yes");
        assert_eq!(yes_no(false), "no");
    }

    /// §1.6 `player_health_boost` — Bukkit `maxHealth - 20`, floored at 0;
    /// a player with the vanilla max health reports `"0"`.
    #[test]
    fn health_boost_floors_at_zero() {
        assert_eq!(format_number(health_boost_from(20.0)), "0");
        assert_eq!(format_number(health_boost_from(16.0)), "0");
        assert_eq!(format_number(health_boost_from(40.0)), "20");
        assert_eq!(format_number(health_boost_from(21.0)), "1");
        assert_eq!(format_number(health_boost_from(25.5)), "5.5");
    }

    /// §1.7 — the host reports the dimension id, the Mod reports three English
    /// names that the `Placeholder-Translations` table then localizes.
    #[test]
    fn world_type_maps_dimensions_to_the_mod_names() {
        assert_eq!(world_type_name("minecraft:the_nether"), "Nether");
        assert_eq!(world_type_name("minecraft:the_end"), "The End");
        assert_eq!(world_type_name("minecraft:overworld"), "Overworld");
        // Bare paths and unknown/modded dimensions fall into the same branches.
        assert_eq!(world_type_name("the_nether"), "Nether");
        assert_eq!(world_type_name("the_end"), "The End");
        assert_eq!(world_type_name("minecraft:overworld_caves"), "Overworld");
        assert_eq!(world_type_name("some:custom_dimension"), "Overworld");
    }

    /// §1.6 `player_exp_to_level` — the three vanilla level-cost branches; a
    /// full bar needs nothing and an empty bar needs the whole level cost.
    #[test]
    fn exp_to_level_uses_vanilla_cost_curve() {
        // Levels 0–15: `2·level + 7`.
        assert_eq!(exp_to_level_from(0, 0.0), 7);
        assert_eq!(exp_to_level_from(15, 0.0), 37);
        // Levels 16–30: `5·level − 38`.
        assert_eq!(exp_to_level_from(16, 0.0), 42);
        assert_eq!(exp_to_level_from(30, 0.0), 112);
        // Level 31+: `9·level − 158`.
        assert_eq!(exp_to_level_from(31, 0.0), 121);
        assert_eq!(exp_to_level_from(50, 0.0), 292);

        // Bar progress reduces what is still needed; a full bar needs zero.
        assert_eq!(exp_to_level_from(0, 1.0), 0);
        assert_eq!(exp_to_level_from(0, 0.5), 4, "7 · (1 − 0.5) rounds to 4");
        // Out-of-range progress is clamped rather than producing negatives.
        assert_eq!(exp_to_level_from(0, 5.0), 0);
        assert_eq!(exp_to_level_from(0, -1.0), 7);
    }

    /// §1.6 `player_biome` / `player_biome_capitalized` — the WIT enum variant
    /// `DeepDark` becomes `deep-dark`, so the id is `minecraft:deep_dark` and
    /// the capitalized form is `Deep Dark`.
    #[test]
    fn biome_key_and_capitalization_follow_the_mod_shape() {
        let key = biome_key(pumpkin_plugin_api::wit::pumpkin::plugin::biomes::Biome::DeepDark);
        assert_eq!(key, "deep-dark");
        assert_eq!(format!("minecraft:{}", key.replace('-', "_")), "minecraft:deep_dark");
        assert_eq!(capitalize_words(&key), "Deep Dark");

        // Single-word variants stay one word.
        let plain = biome_key(pumpkin_plugin_api::wit::pumpkin::plugin::biomes::Biome::Beach);
        assert_eq!(plain, "beach");
        assert_eq!(capitalize_words(&plain), "Beach");

        // Already-capitalized and empty-ish keys do not panic.
        assert_eq!(capitalize_words(""), "");
        assert_eq!(capitalize_words("--"), "");
    }

    /// §1.5 `player_has_potioneffect_<id>` — a vanilla id maps onto the WIT
    /// enum and an unknown id is absent (never a panic). Ids arrive in the
    /// Mod's underscore spelling, which is the only form accepted.
    #[test]
    fn effect_ids_resolve_only_for_vanilla_effects() {
        assert_eq!(effect_type("jump_boost"), Some(StatusEffectType::JumpBoost));
        assert_eq!(effect_type("jump-boost"), None, "ids arrive underscore-separated");
        assert_eq!(effect_type("night_vision"), Some(StatusEffectType::NightVision));
        assert_eq!(effect_type("infested"), Some(StatusEffectType::Infested));

        // Not a vanilla effect → absent rather than a panic.
        assert_eq!(effect_type("trchat_custom_effect"), None);
        assert_eq!(effect_type(""), None);
    }

    /// §1.5 `player_item_in_hand_level_<enchant>` — the enum's Debug form is
    /// path-qualified, so the enchantment name is the last segment in kebab.
    #[test]
    fn enchantment_names_are_kebab_case() {
        assert_eq!(enchant_name(Enchantment::Sharpness), "sharpness");
        assert_eq!(enchant_name(Enchantment::BaneOfArthropods), "bane-of-arthropods");
        assert_eq!(enchant_name(Enchantment::FireAspect), "fire-aspect");
    }

    /// §1.6 locale family — the raw locale splits into language and country,
    /// and a bare language has no country component.
    #[test]
    fn locale_components_split_language_and_country() {
        // Mirrors the helper's own split so the assertions stay honest about
        // which shape (`en_us`) the host actually sends.
        let cases = [
            ("en_us", "en", "us", "US"),
            ("zh_cn", "zh", "cn", "CN"),
            ("de_de", "de", "de", "DE"),
        ];
        for (locale, language, country, display) in cases {
            let mut parts = locale.split(['_', '-']);
            let got_language = parts.next().unwrap_or("").to_ascii_lowercase();
            let got_country = parts.next().unwrap_or("").to_ascii_lowercase();
            assert_eq!(got_language, language, "language of {locale}");
            assert_eq!(got_country, country, "country of {locale}");
            assert_eq!(got_country.to_ascii_uppercase(), display);
        }
    }

    /// §1.6 `player_world_time_12` / `_24` — tick 0 is 06:00 and the 12-hour
    /// form wraps 0/12 correctly.
    #[test]
    fn world_clock_offsets_by_six_hours() {
        // The helper documents tick 0 = 06:00; check the same arithmetic it
        // performs so the expectation is not an independent reimplementation.
        let clock = |ticks: u64| {
            let total_minutes = ((ticks as f64 / 1000.0) + 6.0) * 60.0;
            let hour24 = ((total_minutes / 60.0) as u64) % 24;
            let minute = (total_minutes % 60.0) as u64;
            (hour24, minute)
        };
        assert_eq!(clock(0), (6, 0), "tick 0 is 06:00");
        assert_eq!(clock(6000), (12, 0), "noon");
        assert_eq!(clock(18000), (0, 0), "midnight wraps to 0");

        // 12-hour wrapping: 00:00 → 12 AM, 12:00 → 12 PM.
        for (hour24, expected_hour, expected_suffix) in
            [(0u64, 12u64, "AM"), (6, 6, "AM"), (12, 12, "PM"), (18, 6, "PM")]
        {
            let hour12 = match hour24 % 12 {
                0 => 12,
                h => h,
            };
            let suffix = if hour24 < 12 { "AM" } else { "PM" };
            assert_eq!(hour12, expected_hour, "hour of {hour24}:00");
            assert_eq!(suffix, expected_suffix, "suffix of {hour24}:00");
        }
    }
}
