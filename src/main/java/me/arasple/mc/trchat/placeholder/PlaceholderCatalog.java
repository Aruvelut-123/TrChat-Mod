package me.arasple.mc.trchat.placeholder;

import java.util.Set;

public final class PlaceholderCatalog {

    public static final Set<String> SERVER = Set.of(
        "name", "online", "version", "max_players", "unique_joins", "uptime",
        "ram_used", "ram_free", "ram_total", "ram_max", "tps", "tps_1", "tps_5", "tps_15",
        "tps_1_colored", "tps_5_colored", "tps_15_colored", "has_whitelist",
        "total_chunks", "total_living_entities", "total_entities"
    );

    public static final Set<String> PLAYER = Set.of(
        "allow_flight",
        "armor_helmet_name", "armor_helmet_data", "armor_helmet_durability",
        "armor_chestplate_name", "armor_chestplate_data", "armor_chestplate_durability",
        "armor_leggings_name", "armor_leggings_data", "armor_leggings_durability",
        "armor_boots_name", "armor_boots_data", "armor_boots_durability",
        "bed_x", "bed_y", "bed_z", "bed_world", "biome", "biome_capitalized",
        "block_underneath", "can_pickup_items", "colored_ping",
        "compass_world", "compass_x", "compass_y", "compass_z", "custom_name",
        "current_exp", "direction", "direction_xz", "displayname", "list_name", "exp",
        "exp_to_level", "first_join_date", "first_played", "first_join",
        "first_played_formatted", "fly_speed", "food_level", "gamemode", "has_empty_slot",
        "has_played_before", "empty_slots", "has_health_boost", "health", "health_boost",
        "health_rounded", "health_scale", "ip", "online", "is_whitelisted", "is_banned",
        "is_flying", "is_sneaking", "is_sprinting", "is_sleeping", "is_inside_vehicle",
        "is_op", "item_in_hand", "item_in_hand_name", "item_in_hand_data",
        "item_in_hand_durability", "item_in_offhand", "item_in_offhand_name",
        "item_in_offhand_data", "item_in_offhand_durability", "locale",
        "locale_display_name", "locale_short", "locale_country", "locale_display_country",
        "last_damage", "last_played", "last_join", "last_played_formatted",
        "last_join_date", "level", "light_level", "max_air", "max_health",
        "max_health_rounded", "max_no_damage_ticks", "minutes_lived", "name",
        "no_damage_ticks", "ping", "remaining_air", "saturation", "seconds_lived",
        "sleep_ticks", "thunder_duration", "ticks_lived", "time", "time_offset",
        "total_exp", "uuid", "walk_speed", "weather_duration", "world", "world_type",
        "world_time_12", "world_time_24", "x", "y", "z", "yaw", "pitch", "absorption"
    );

    private static final Set<String> LOCALIZABLE = Set.of(
        "server_has_whitelist",
        "player_allow_flight",
        "player_can_pickup_items",
        "player_gamemode",
        "player_has_empty_slot",
        "player_has_played_before",
        "player_has_health_boost",
        "player_online",
        "player_is_whitelisted",
        "player_is_banned",
        "player_is_flying",
        "player_is_sneaking",
        "player_is_sprinting",
        "player_is_sleeping",
        "player_is_inside_vehicle",
        "player_is_op",
        "player_direction",
        "player_world_type"
    );

    private PlaceholderCatalog() {
    }

    public static boolean supports(String token) {
        if (token.startsWith("server_")) {
            String key = token.substring(7);
            return SERVER.contains(key)
                || key.startsWith("online_")
                || key.startsWith("time_")
                || key.startsWith("countdown_");
        }
        if (token.startsWith("player_")) {
            String key = token.substring(7);
            return PLAYER.contains(key)
                || key.startsWith("ping_")
                || key.startsWith("has_permission_")
                || key.startsWith("has_potioneffect_")
                || key.startsWith("item_in_hand_level_")
                || key.startsWith("item_in_offhand_level_");
        }
        return false;
    }

    public static boolean isLocalizable(String token) {
        if (token == null) {
            return false;
        }
        if (token.startsWith("server_countdown_")) {
            return true;
        }
        if (token.startsWith("player_has_permission_") || token.startsWith("player_has_potioneffect_")) {
            return true;
        }
        return LOCALIZABLE.contains(token);
    }
}
