package me.arasple.mc.trchat.neoforge.placeholder;

import org.junit.jupiter.api.Test;

import java.util.Arrays;

import static org.junit.jupiter.api.Assertions.assertTrue;

class PlaceholderCatalogTest {

    @Test
    void supportsEveryRequestedServerPlaceholder() {
        assertSupported("""
            server_name server_online server_version server_max_players server_unique_joins
            server_uptime server_ram_used server_ram_free server_ram_total server_ram_max
            server_tps server_tps_1 server_tps_5 server_tps_15 server_tps_1_colored
            server_tps_5_colored server_tps_15_colored server_online_overworld
            server_has_whitelist server_total_chunks server_total_living_entities
            server_total_entities server_time_HH:mm:ss
            server_countdown_dd.MM.yyyy_01.01.2030
            """);
    }

    @Test
    void supportsEveryRequestedPlayerPlaceholder() {
        assertSupported("""
            player_allow_flight player_armor_helmet_name player_armor_helmet_data
            player_armor_helmet_durability player_armor_chestplate_name
            player_armor_chestplate_data player_armor_chestplate_durability
            player_armor_leggings_name player_armor_leggings_data
            player_armor_leggings_durability player_armor_boots_name player_armor_boots_data
            player_armor_boots_durability player_bed_x player_bed_y player_bed_z player_bed_world
            player_biome player_biome_capitalized player_block_underneath player_can_pickup_items
            player_colored_ping player_compass_world player_compass_x player_compass_y
            player_compass_z player_custom_name player_current_exp player_direction
            player_direction_xz player_displayname player_list_name player_exp player_exp_to_level
            player_first_join_date player_first_played player_first_join
            player_first_played_formatted player_fly_speed player_food_level player_gamemode
            player_has_empty_slot player_has_played_before player_empty_slots
            player_has_health_boost player_has_potioneffect_speed
            player_has_permission_trchat.global player_health player_health_boost
            player_health_rounded player_health_scale player_ip player_online
            player_is_whitelisted player_is_banned player_is_flying player_is_sneaking
            player_is_sprinting player_is_sleeping player_is_inside_vehicle player_is_op
            player_item_in_hand player_item_in_hand_name player_item_in_hand_data
            player_item_in_hand_durability player_item_in_hand_level_sharpness
            player_item_in_offhand player_item_in_offhand_name player_item_in_offhand_data
            player_item_in_offhand_durability player_item_in_offhand_level_mending
            player_locale player_locale_display_name player_locale_short player_locale_country
            player_locale_display_country player_last_damage player_last_played player_last_join
            player_last_played_formatted player_last_join_date player_level player_light_level
            player_max_air player_max_health player_max_health_rounded
            player_max_no_damage_ticks player_minutes_lived player_name player_no_damage_ticks
            player_ping player_ping_Alex player_remaining_air player_saturation player_seconds_lived
            player_sleep_ticks player_thunder_duration player_ticks_lived player_time
            player_time_offset player_total_exp player_uuid player_walk_speed
            player_weather_duration player_world player_world_type player_world_time_12
            player_world_time_24 player_x player_y player_z player_yaw player_pitch
            player_absorption
            """);
    }

    private static void assertSupported(String placeholders) {
        Arrays.stream(placeholders.split("\\s+"))
            .filter(value -> !value.isBlank())
            .forEach(value -> assertTrue(PlaceholderCatalog.supports(value), value));
    }
}
