package me.arasple.mc.trchat.neoforge.placeholder;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.core.component.DataComponents;
import net.minecraft.resources.ResourceKey;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.EnchantmentHelper;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.storage.LevelResource;
import net.minecraft.world.level.storage.ServerLevelData;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import me.arasple.mc.trchat.neoforge.lang.LanguageService;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;

import java.io.IOException;
import java.lang.management.ManagementFactory;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.BasicFileAttributes;
import java.time.Duration;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.time.format.DateTimeParseException;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class PlaceholderResolver {

    private static final Pattern PLACEHOLDER = Pattern.compile("%([^%]+)%");
    private static final DateTimeFormatter PLAYER_DATE = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss");
    private static final long MEBIBYTE = 1_048_576L;

    private final MinecraftServer server;
    private final ServerMetrics metrics;
    private final PlayerStatsTracker playerStats;
    private final LanguageService languages;

    public PlaceholderResolver(
        MinecraftServer server,
        ServerMetrics metrics,
        PlayerStatsTracker playerStats,
        LanguageService languages
    ) {
        this.server = server;
        this.metrics = metrics;
        this.playerStats = playerStats;
        this.languages = languages;
    }

    public String resolve(String input, ServerPlayer player) {
        return resolve(input, player, Map.of());
    }

    public String resolve(String input, ServerPlayer player, Map<String, String> local) {
        if (input == null || input.isEmpty()) {
            return "";
        }
        Matcher matcher = PLACEHOLDER.matcher(input);
        StringBuilder output = new StringBuilder(input.length());
        while (matcher.find()) {
            String token = matcher.group(1);
            String value = local.get(token);
            if (value == null) {
                value = resolveToken(token, player);
                value = languages.translatePlaceholder(player, token, value);
            }
            matcher.appendReplacement(output, Matcher.quoteReplacement(value == null ? "" : value));
        }
        matcher.appendTail(output);
        return output.toString();
    }

    public String resolveToken(String token, ServerPlayer player) {
        if (token.startsWith("player_")) {
            return player == null ? "" : player(token.substring(7), player);
        }
        if (token.startsWith("server_")) {
            return server(token.substring(7));
        }
        return "";
    }

    private String server(String key) {
        Runtime runtime = Runtime.getRuntime();
        return switch (key) {
            case "name" -> TrChatConfig.SERVER_NAME.get();
            case "online" -> integer(server.getPlayerCount());
            case "version" -> server.getServerVersion();
            case "max_players" -> integer(server.getMaxPlayers());
            case "unique_joins" -> integer(uniqueJoins());
            case "uptime" -> duration(ManagementFactory.getRuntimeMXBean().getUptime() / 1000L);
            case "ram_used" -> integer((runtime.totalMemory() - runtime.freeMemory()) / MEBIBYTE);
            case "ram_free" -> integer(runtime.freeMemory() / MEBIBYTE);
            case "ram_total" -> integer(runtime.totalMemory() / MEBIBYTE);
            case "ram_max" -> integer(runtime.maxMemory() / MEBIBYTE);
            case "tps" -> decimal(Math.min(20.0D, 1_000_000_000.0D / Math.max(50_000_000.0D, server.getAverageTickTimeNanos())));
            case "tps_1" -> decimal(metrics.tps(1));
            case "tps_5" -> decimal(metrics.tps(5));
            case "tps_15" -> decimal(metrics.tps(15));
            case "tps_1_colored" -> coloredTps(metrics.tps(1));
            case "tps_5_colored" -> coloredTps(metrics.tps(5));
            case "tps_15_colored" -> coloredTps(metrics.tps(15));
            case "has_whitelist" -> bool(server.getPlayerList().isUsingWhitelist());
            case "total_chunks" -> integer(totalChunks());
            case "total_living_entities" -> integer(totalEntities(true));
            case "total_entities" -> integer(totalEntities(false));
            default -> dynamicServer(key);
        };
    }

    private String dynamicServer(String key) {
        if (key.startsWith("online_")) {
            String requested = key.substring("online_".length());
            for (ServerLevel level : server.getAllLevels()) {
                ResourceLocation id = level.dimension().location();
                if (id.toString().equalsIgnoreCase(requested) || id.getPath().equalsIgnoreCase(requested)) {
                    return integer(level.players().size());
                }
            }
            return "-1";
        }
        if (key.startsWith("time_")) {
            try {
                return ZonedDateTime.now().format(DateTimeFormatter.ofPattern(key.substring(5)));
            } catch (IllegalArgumentException exception) {
                return "";
            }
        }
        if (key.startsWith("countdown_")) {
            return countdown(key.substring("countdown_".length()));
        }
        return "";
    }

    private String player(String key, ServerPlayer player) {
        if (key.startsWith("ping_")) {
            ServerPlayer target = server.getPlayerList().getPlayerByName(key.substring(5));
            return target == null ? "0" : integer(target.connection.latency());
        }
        if (key.startsWith("has_permission_")) {
            return bool(TrChatPermissions.check(player, key.substring("has_permission_".length())));
        }
        if (key.startsWith("has_potioneffect_")) {
            return bool(hasEffect(player, key.substring("has_potioneffect_".length())));
        }
        if (key.startsWith("item_in_hand_level_")) {
            return integer(enchantmentLevel(player.getMainHandItem(), key.substring("item_in_hand_level_".length())));
        }
        if (key.startsWith("item_in_offhand_level_")) {
            return integer(enchantmentLevel(player.getOffhandItem(), key.substring("item_in_offhand_level_".length())));
        }

        BlockPos position = player.blockPosition();
        ServerLevel level = player.serverLevel();
        ItemStack mainHand = player.getMainHandItem();
        ItemStack offHand = player.getOffhandItem();
        BlockPos bed = player.getRespawnPosition();
        BlockPos compass = level.getSharedSpawnPos();
        Locale locale = locale(player);
        long firstPlayed = playerDataTime(player, true);
        long lastPlayed = playerDataTime(player, false);

        return switch (key) {
            case "allow_flight" -> bool(player.getAbilities().mayfly);
            case "armor_helmet_name" -> itemName(player.getItemBySlot(EquipmentSlot.HEAD));
            case "armor_helmet_data" -> itemData(player.getItemBySlot(EquipmentSlot.HEAD));
            case "armor_helmet_durability" -> itemDurability(player.getItemBySlot(EquipmentSlot.HEAD));
            case "armor_chestplate_name" -> itemName(player.getItemBySlot(EquipmentSlot.CHEST));
            case "armor_chestplate_data" -> itemData(player.getItemBySlot(EquipmentSlot.CHEST));
            case "armor_chestplate_durability" -> itemDurability(player.getItemBySlot(EquipmentSlot.CHEST));
            case "armor_leggings_name" -> itemName(player.getItemBySlot(EquipmentSlot.LEGS));
            case "armor_leggings_data" -> itemData(player.getItemBySlot(EquipmentSlot.LEGS));
            case "armor_leggings_durability" -> itemDurability(player.getItemBySlot(EquipmentSlot.LEGS));
            case "armor_boots_name" -> itemName(player.getItemBySlot(EquipmentSlot.FEET));
            case "armor_boots_data" -> itemData(player.getItemBySlot(EquipmentSlot.FEET));
            case "armor_boots_durability" -> itemDurability(player.getItemBySlot(EquipmentSlot.FEET));
            case "bed_x" -> bed == null ? "" : integer(bed.getX());
            case "bed_y" -> bed == null ? "" : integer(bed.getY());
            case "bed_z" -> bed == null ? "" : integer(bed.getZ());
            case "bed_world" -> bed == null ? "" : player.getRespawnDimension().location().getPath();
            case "biome" -> biome(level, position, false);
            case "biome_capitalized" -> biome(level, position, true);
            case "block_underneath" -> registryName(BuiltInRegistries.BLOCK.getKey(level.getBlockState(position.below()).getBlock()));
            case "can_pickup_items" -> bool(!player.isSpectator());
            case "colored_ping" -> coloredPing(player.connection.latency());
            case "compass_world" -> Level.OVERWORLD.location().getPath();
            case "compass_x" -> integer(compass.getX());
            case "compass_y" -> integer(compass.getY());
            case "compass_z" -> integer(compass.getZ());
            case "custom_name" -> player.getCustomName() == null ? player.getGameProfile().getName() : player.getCustomName().getString();
            case "current_exp" -> integer(totalExperienceAtCurrentLevel(player));
            case "direction" -> direction(player.getYRot());
            case "direction_xz" -> directionXz(player.getYRot());
            case "displayname" -> player.getDisplayName().getString();
            case "list_name" -> Optional.ofNullable(player.getTabListDisplayName()).orElse(player.getDisplayName()).getString();
            case "exp" -> decimal(player.experienceProgress);
            case "exp_to_level" -> integer(player.getXpNeededForNextLevel());
            case "first_played", "first_join" -> integer(firstPlayed);
            case "first_played_formatted", "first_join_date" -> formatEpoch(firstPlayed);
            case "fly_speed" -> decimal(player.getAbilities().getFlyingSpeed());
            case "food_level" -> integer(player.getFoodData().getFoodLevel());
            case "gamemode" -> player.gameMode.getGameModeForPlayer().getName().toUpperCase(Locale.ROOT);
            case "has_empty_slot" -> bool(player.getInventory().getFreeSlot() >= 0);
            case "has_played_before" -> bool(firstPlayed > 0);
            case "empty_slots" -> integer(emptySlots(player));
            case "has_health_boost" -> bool(player.hasEffect(MobEffects.HEALTH_BOOST));
            case "health" -> decimal(player.getHealth());
            case "health_boost" -> decimal(Math.max(0.0F, player.getMaxHealth() - 20.0F));
            case "health_rounded" -> integer(Math.round(player.getHealth()));
            case "health_scale" -> decimal(player.getMaxHealth());
            case "ip" -> player.getIpAddress();
            case "online" -> "yes";
            case "is_whitelisted" -> bool(server.getPlayerList().isWhiteListed(player.getGameProfile()));
            case "is_banned" -> bool(server.getPlayerList().getBans().isBanned(player.getGameProfile()));
            case "is_flying" -> bool(player.getAbilities().flying);
            case "is_sneaking" -> bool(player.isCrouching());
            case "is_sprinting" -> bool(player.isSprinting());
            case "is_sleeping" -> bool(player.isSleeping());
            case "is_inside_vehicle" -> bool(player.isPassenger());
            case "is_op" -> bool(player.hasPermissions(2));
            case "item_in_hand" -> itemType(mainHand);
            case "item_in_hand_name" -> itemName(mainHand);
            case "item_in_hand_data" -> itemData(mainHand);
            case "item_in_hand_durability" -> itemDurability(mainHand);
            case "item_in_offhand" -> itemType(offHand);
            case "item_in_offhand_name" -> itemName(offHand);
            case "item_in_offhand_data" -> itemData(offHand);
            case "item_in_offhand_durability" -> itemDurability(offHand);
            case "locale" -> player.clientInformation().language();
            case "locale_display_name" -> locale.getDisplayName();
            case "locale_short" -> locale.getLanguage();
            case "locale_country" -> locale.getCountry();
            case "locale_display_country" -> locale.getDisplayCountry();
            case "last_damage" -> decimal(playerStats.lastDamage(player));
            case "last_played", "last_join" -> integer(lastPlayed);
            case "last_played_formatted", "last_join_date" -> formatEpoch(lastPlayed);
            case "level" -> integer(player.experienceLevel);
            case "light_level" -> integer(level.getMaxLocalRawBrightness(position));
            case "max_air" -> integer(player.getMaxAirSupply());
            case "max_health" -> decimal(player.getMaxHealth());
            case "max_health_rounded" -> integer(Math.round(player.getMaxHealth()));
            case "max_no_damage_ticks" -> "20";
            case "minutes_lived" -> integer(player.tickCount / 1200);
            case "name" -> player.getGameProfile().getName();
            case "no_damage_ticks" -> integer(playerStats.noDamageTicks(player));
            case "ping" -> integer(player.connection.latency());
            case "remaining_air" -> integer(player.getAirSupply());
            case "saturation" -> decimal(player.getFoodData().getSaturationLevel());
            case "seconds_lived" -> integer(player.tickCount / 20);
            case "sleep_ticks" -> integer(player.getSleepTimer());
            case "thunder_duration" -> integer(weatherDuration(level, true));
            case "ticks_lived" -> integer(player.tickCount);
            case "time" -> integer(level.getDayTime());
            case "time_offset" -> "0";
            case "total_exp" -> integer(player.totalExperience);
            case "uuid" -> player.getUUID().toString();
            case "walk_speed" -> decimal(player.getAbilities().getWalkingSpeed());
            case "weather_duration" -> integer(weatherDuration(level, false));
            case "world" -> level.dimension().location().getPath();
            case "world_type" -> worldType(level.dimension());
            case "world_time_12" -> worldTime(level.getDayTime(), true);
            case "world_time_24" -> worldTime(level.getDayTime(), false);
            case "x" -> integer(position.getX());
            case "y" -> integer(position.getY());
            case "z" -> integer(position.getZ());
            case "yaw" -> decimal(player.getYRot());
            case "pitch" -> decimal(player.getXRot());
            case "absorption" -> integer((int) player.getAbsorptionAmount());
            default -> "";
        };
    }

    private boolean hasEffect(ServerPlayer player, String name) {
        ResourceLocation id = parseId(name);
        Optional<Holder.Reference<MobEffect>> effect = BuiltInRegistries.MOB_EFFECT.getHolder(id);
        return effect.isPresent() && player.hasEffect(effect.get());
    }

    private int enchantmentLevel(ItemStack stack, String name) {
        ResourceLocation id = parseId(name);
        Optional<Holder.Reference<Enchantment>> enchantment = server.registryAccess()
            .registryOrThrow(Registries.ENCHANTMENT)
            .getHolder(id);
        return enchantment.map(value -> EnchantmentHelper.getItemEnchantmentLevel(value, stack)).orElse(0);
    }

    private static ResourceLocation parseId(String value) {
        String normalized = value.toLowerCase(Locale.ROOT);
        return ResourceLocation.parse(normalized.contains(":") ? normalized : "minecraft:" + normalized);
    }

    private long playerDataTime(ServerPlayer player, boolean creation) {
        Path file = server.getWorldPath(LevelResource.PLAYER_DATA_DIR).resolve(player.getUUID() + ".dat");
        try {
            BasicFileAttributes attributes = Files.readAttributes(file, BasicFileAttributes.class);
            return (creation ? attributes.creationTime() : attributes.lastModifiedTime()).toMillis();
        } catch (IOException ignored) {
            return 0L;
        }
    }

    private int uniqueJoins() {
        Path directory = server.getWorldPath(LevelResource.PLAYER_DATA_DIR);
        if (!Files.isDirectory(directory)) {
            return 0;
        }
        try (var files = Files.list(directory)) {
            return Math.toIntExact(files.filter(path -> path.getFileName().toString().endsWith(".dat")).count());
        } catch (IOException exception) {
            return 0;
        }
    }

    private int totalChunks() {
        int total = 0;
        for (ServerLevel level : server.getAllLevels()) {
            total += level.getChunkSource().getLoadedChunksCount();
        }
        return total;
    }

    private int totalEntities(boolean livingOnly) {
        int total = 0;
        for (ServerLevel level : server.getAllLevels()) {
            for (var entity : level.getAllEntities()) {
                if (!livingOnly || entity instanceof LivingEntity) {
                    total++;
                }
            }
        }
        return total;
    }

    private static String countdown(String arguments) {
        int separator = arguments.indexOf('_');
        if (separator <= 0 || separator == arguments.length() - 1) {
            return "invalid format and time";
        }
        String pattern = arguments.substring(0, separator);
        String target = arguments.substring(separator + 1);
        try {
            DateTimeFormatter formatter = DateTimeFormatter.ofPattern(pattern);
            ZoneId zone = ZoneId.systemDefault();
            ZonedDateTime then;
            try {
                then = LocalDateTime.parse(target, formatter).atZone(zone);
            } catch (DateTimeParseException ignored) {
                then = LocalDate.parse(target, formatter).atStartOfDay(zone);
            }
            long seconds = Duration.between(ZonedDateTime.now(zone), then).getSeconds();
            return seconds <= 0 ? "0" : duration(seconds);
        } catch (IllegalArgumentException | DateTimeParseException exception) {
            return "invalid date";
        }
    }

    static String duration(long totalSeconds) {
        if (totalSeconds <= 0) {
            return "0s";
        }
        long weeks = totalSeconds / 604800;
        long days = totalSeconds / 86400 % 7;
        long hours = totalSeconds / 3600 % 24;
        long minutes = totalSeconds / 60 % 60;
        long seconds = totalSeconds % 60;
        List<String> values = new ArrayList<>();
        if (weeks > 0) values.add(weeks + "w");
        if (days > 0) values.add(days + "d");
        if (hours > 0) values.add(hours + "h");
        if (minutes > 0) values.add(minutes + "m");
        if (seconds > 0) values.add(seconds + "s");
        return String.join(" ", values);
    }

    private static String itemType(ItemStack stack) {
        return stack.isEmpty() ? "AIR" : registryName(BuiltInRegistries.ITEM.getKey(stack.getItem()));
    }

    private static String itemName(ItemStack stack) {
        return stack.isEmpty() || !stack.has(DataComponents.CUSTOM_NAME) ? "" : stack.getHoverName().getString();
    }

    private static String itemData(ItemStack stack) {
        return integer(stack.isEmpty() ? 0 : stack.getDamageValue());
    }

    private static String itemDurability(ItemStack stack) {
        return integer(stack.isEmpty() ? 0 : Math.max(0, stack.getMaxDamage() - stack.getDamageValue()));
    }

    private static int emptySlots(ServerPlayer player) {
        int empty = 0;
        for (int index = 0; index < 36; index++) {
            if (player.getInventory().getItem(index).isEmpty()) {
                empty++;
            }
        }
        return empty;
    }

    private static int totalExperienceAtCurrentLevel(ServerPlayer player) {
        int level = player.experienceLevel;
        int base = level <= 16
            ? level * level + 6 * level
            : level <= 31
                ? (int) (2.5D * level * level - 40.5D * level + 360)
                : (int) (4.5D * level * level - 162.5D * level + 2220);
        return base + Math.round(player.experienceProgress * player.getXpNeededForNextLevel());
    }

    private static String biome(ServerLevel level, BlockPos position, boolean capitalized) {
        String name = level.getBiome(position).unwrapKey()
            .map(key -> key.location().getPath())
            .orElse("unknown");
        if (!capitalized) {
            return name.toUpperCase(Locale.ROOT);
        }
        String[] words = name.split("_");
        for (int index = 0; index < words.length; index++) {
            words[index] = Character.toUpperCase(words[index].charAt(0)) + words[index].substring(1).toLowerCase(Locale.ROOT);
        }
        return String.join(" ", words);
    }

    private static Locale locale(ServerPlayer player) {
        return Locale.forLanguageTag(player.clientInformation().language().replace('_', '-'));
    }

    private static String direction(float yaw) {
        String[] directions = {"S", "SW", "W", "NW", "N", "NE", "E", "SE"};
        return directions[Math.round(yaw / 45.0F) & 7];
    }

    private static String directionXz(float yaw) {
        float rotation = yaw % 360.0F;
        if (rotation < 0) rotation += 360.0F;
        if (rotation <= 45 || rotation >= 315) return "+Z";
        if (rotation <= 135) return "-X";
        if (rotation <= 225) return "-Z";
        return "+X";
    }

    private static String worldType(ResourceKey<Level> dimension) {
        if (dimension.equals(Level.NETHER)) return "Nether";
        if (dimension.equals(Level.END)) return "The End";
        return "Overworld";
    }

    private static int weatherDuration(ServerLevel level, boolean thunder) {
        return level.getLevelData() instanceof ServerLevelData data
            ? thunder ? data.getThunderTime() : data.getRainTime()
            : 0;
    }

    private static String worldTime(long ticks, boolean twelveHour) {
        long adjusted = Math.floorMod(ticks - 6000L, 24000L);
        int hour = (int) (adjusted / 1000L);
        int minute = (int) ((adjusted % 1000L) * 60L / 1000L);
        if (!twelveHour) {
            return String.format(Locale.ENGLISH, "%02d:%02d", hour, minute);
        }
        String suffix = hour < 12 ? "AM" : "PM";
        int displayHour = hour % 12;
        if (displayHour == 0) displayHour = 12;
        return String.format(Locale.ENGLISH, "%d:%02d %s", displayHour, minute, suffix);
    }

    private static String coloredPing(int ping) {
        return (ping > 100 ? "&c" : ping > 50 ? "&e" : "&a") + ping;
    }

    private static String coloredTps(double tps) {
        return (tps < 15.0D ? "&c" : tps < 18.0D ? "&e" : "&a") + decimal(tps);
    }

    private static String formatEpoch(long epochMillis) {
        return epochMillis <= 0 ? "" : PLAYER_DATE.format(Instant.ofEpochMilli(epochMillis).atZone(ZoneId.systemDefault()));
    }

    private static String registryName(ResourceLocation id) {
        return id == null ? "" : id.getPath().toUpperCase(Locale.ROOT);
    }

    private static String bool(boolean value) {
        return value ? "yes" : "no";
    }

    private static String integer(long value) {
        return Long.toString(value);
    }

    private static String decimal(double value) {
        if (!Double.isFinite(value)) {
            return "0";
        }
        if (Math.rint(value) == value) {
            return Long.toString(Math.round(value));
        }
        return String.format(Locale.ROOT, "%.2f", value).replaceAll("0+$", "").replaceAll("\\.$", "");
    }
}
