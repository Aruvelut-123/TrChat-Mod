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

use pumpkin_plugin_api::wit::pumpkin::plugin::common::Hand;
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
        match local.iter().find(|(k, _)| *k == token) {
            Some((_, v)) => out.push_str(v),
            None => out.push_str(&resolve_token(&token, player, server, config)),
        }
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
        // §1.6 — `player_time_offset` and `player_max_no_damage_ticks` are
        // documented constants (spec line 692).
        "time_offset" => "0".to_string(),
        "max_no_damage_ticks" => "20".to_string(),

        // Session / connection.
        "ping" => player.get_ping().to_string(),

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

    #[test]
    fn yes_no_shape() {
        assert_eq!(yes_no(true), "yes");
        assert_eq!(yes_no(false), "no");
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
}
