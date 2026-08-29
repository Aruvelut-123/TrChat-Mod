package me.arasple.mc.trchat.config;

import java.util.List;
import java.util.Map;
import java.util.function.Function;

//? if neoforge {
import net.neoforged.neoforge.common.ModConfigSpec;

public final class TrChatConfig {

    public static final ModConfigSpec SPEC;

    public static final ModConfigSpec.IntValue SERVER_ID;
    public static final ModConfigSpec.ConfigValue<String> SERVER_NAME;
    public static final ModConfigSpec.ConfigValue<String> DEFAULT_LANGUAGE;
    public static final ModConfigSpec.ConfigValue<String> GLOBAL_PREFIX;
    public static final ModConfigSpec.IntValue MESSAGE_MAX_LENGTH;
    public static final ModConfigSpec.IntValue COOLDOWN_MILLIS;
    public static final ModConfigSpec.DoubleValue ANTI_REPEAT_SIMILARITY;
    public static final ModConfigSpec.ConfigValue<List<? extends String>> BLOCKED_WORDS;
    public static final ModConfigSpec.ConfigValue<String> FILTER_REPLACEMENT;
    public static final ModConfigSpec.ConfigValue<List<? extends String>> DISABLED_WORLDS;
    public static final ModConfigSpec.ConfigValue<String> LOG_NORMAL_FORMAT;
    public static final ModConfigSpec.ConfigValue<String> LOG_PRIVATE_FORMAT;
    public static final ModConfigSpec.IntValue LOG_RETENTION_DAYS;

    public static final ModConfigSpec.BooleanValue UPDATE_CHECK_ENABLED;
    public static final ModConfigSpec.IntValue UPDATE_CHECK_INTERVAL_MINUTES;

    public static final ModConfigSpec.BooleanValue REDIS_ENABLED;
    public static final ModConfigSpec.ConfigValue<String> REDIS_HOST;
    public static final ModConfigSpec.IntValue REDIS_PORT;
    public static final ModConfigSpec.ConfigValue<String> REDIS_USERNAME;
    public static final ModConfigSpec.ConfigValue<String> REDIS_PASSWORD;
    public static final ModConfigSpec.IntValue REDIS_DATABASE;
    public static final ModConfigSpec.IntValue REDIS_CONNECT_TIMEOUT;
    public static final ModConfigSpec.IntValue REDIS_SOCKET_TIMEOUT;
    public static final ModConfigSpec.IntValue REDIS_RECONNECT_DELAY;
    public static final ModConfigSpec.ConfigValue<String> REDIS_CHANNEL;

    static {
        ModConfigSpec.Builder builder = new ModConfigSpec.Builder();

        builder.comment("Chat behavior. Placeholders: %player%, %display_name%, %message%, %server%.")
            .push("chat");
        SERVER_ID = builder
            .comment("Numeric server identifier. Use the server port to match the Bukkit TrChat Redis protocol.")
            .defineInRange("serverId", 25565, 1, 65535);
        SERVER_NAME = builder
            .comment("Value exposed by %server_name%.")
            .define("serverName", "A Minecraft Server");
        DEFAULT_LANGUAGE = builder
            .comment("Fallback language file name from config/trchat/lang without .yml.")
            .define("defaultLanguage", "zh_CN");
        GLOBAL_PREFIX = builder
            .comment("Messages beginning with this prefix use Redis global chat. '!all' matches the Bukkit default.")
            .define("globalPrefix", "!all");
        MESSAGE_MAX_LENGTH = builder.defineInRange("messageMaxLength", 256, 1, 32767);
        COOLDOWN_MILLIS = builder.defineInRange("cooldownMillis", 2000, 0, 600000);
        ANTI_REPEAT_SIMILARITY = builder
            .comment("0 disables anti-repeat. Values from 0.0 to 1.0 compare against the player's previous message.")
            .defineInRange("antiRepeatSimilarity", 0.85D, 0.0D, 1.0D);
        BLOCKED_WORDS = builder.defineList(
            "blockedWords",
            () -> List.<String>of(),
            () -> "",
            value -> value instanceof String
        );
        FILTER_REPLACEMENT = builder.define("filterReplacement", "*");
        DISABLED_WORLDS = builder
            .comment("World names (regular expressions, case-insensitive) where chat is disabled.",
                "在此世界内禁用聊天（正则表达式，不区分大小写）。")
            .defineList(
                "disabledWorlds",
                () -> List.<String>of(),
                () -> "",
                value -> value instanceof String
            );
        builder.pop();

        builder.comment(
            "Daily plain-text chat logs under config/trchat/logs.",
            "每日纯文本聊天日志，保存于 config/trchat/logs。"
        ).push("logging");
        LOG_NORMAL_FORMAT = builder.define("normalMessageFormat", "[{0}] {1}: {2}");
        LOG_PRIVATE_FORMAT = builder.define("privateMessageFormat", "[{0}] {1} -> {2}: {3}");
        LOG_RETENTION_DAYS = builder
            .comment("Delete log files older than this many days. 0 disables deletion.")
            .defineInRange("retentionDays", 0, 0, 36500);
        builder.pop();

        builder.comment(
            "GitHub release update checker. It only notifies and never downloads files.",
            "GitHub Release 更新检查器：仅提醒，不会自动下载文件。"
        ).push("updates");
        UPDATE_CHECK_ENABLED = builder
            .comment("Check for updates and notify the console and online TrChat administrators.",
                "检查更新，并提醒后台及在线 TrChat 管理员。")
            .define("enabled", true);
        UPDATE_CHECK_INTERVAL_MINUTES = builder
            .comment("Minutes between checks. The Bukkit upstream default is 15 minutes.",
                "检查间隔（分钟）。Bukkit 上游默认值为 15 分钟。")
            .defineInRange("intervalMinutes", 15, 1, 1440);
        builder.pop();

        builder.comment("Redis is the only supported cross-server transport.")
            .push("redis");
        REDIS_ENABLED = builder.define("enabled", false);
        REDIS_HOST = builder.define("host", "127.0.0.1");
        REDIS_PORT = builder.defineInRange("port", 6379, 1, 65535);
        REDIS_USERNAME = builder
            .comment("Leave blank when Redis ACL usernames are not used.")
            .define("username", "");
        REDIS_PASSWORD = builder
            .comment("Leave blank when authentication is disabled.")
            .define("password", "");
        REDIS_DATABASE = builder.defineInRange("database", 0, 0, 15);
        REDIS_CONNECT_TIMEOUT = builder.defineInRange("connectTimeoutMillis", 3000, 100, 60000);
        REDIS_SOCKET_TIMEOUT = builder.defineInRange("socketTimeoutMillis", 0, 0, 600000);
        REDIS_RECONNECT_DELAY = builder.defineInRange("reconnectDelayMillis", 3000, 100, 60000);
        REDIS_CHANNEL = builder
            .comment("Do not change when interoperating with the Bukkit version.")
            .define("channel", "trchat-message");
        builder.pop();

        SPEC = builder.build();
    }

    private TrChatConfig() {
    }
}
//? } else {
import me.arasple.mc.trchat.platform.Platform;
import org.yaml.snakeyaml.Yaml;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;

public final class TrChatConfig {

    /** Fabric has no ModConfigSpec; a YAML file under config/trchat is read lazily once per field access. */
    private static volatile Map<String, Object> data;

    public static final Value<Integer> SERVER_ID = new Value<>("chat.serverId", Integer.valueOf(25565), v -> ((Number) v).intValue());
    public static final Value<String> SERVER_NAME = new Value<>("chat.serverName", "A Minecraft Server", Object::toString);
    public static final Value<String> DEFAULT_LANGUAGE = new Value<>("chat.defaultLanguage", "zh_CN", Object::toString);
    public static final Value<String> GLOBAL_PREFIX = new Value<>("chat.globalPrefix", "!all", Object::toString);
    public static final Value<Integer> MESSAGE_MAX_LENGTH = new Value<>("chat.messageMaxLength", Integer.valueOf(256), v -> ((Number) v).intValue());
    public static final Value<Integer> COOLDOWN_MILLIS = new Value<>("chat.cooldownMillis", Integer.valueOf(2000), v -> ((Number) v).intValue());
    public static final Value<Double> ANTI_REPEAT_SIMILARITY = new Value<>("chat.antiRepeatSimilarity", Double.valueOf(0.85D), v -> ((Number) v).doubleValue());
    public static final Value<List<? extends String>> BLOCKED_WORDS = new Value<>("chat.blockedWords", List.of(), v -> stringList(v));
    public static final Value<String> FILTER_REPLACEMENT = new Value<>("chat.filterReplacement", "*", Object::toString);
    public static final Value<List<? extends String>> DISABLED_WORLDS = new Value<>("chat.disabledWorlds", List.of(), v -> stringList(v));
    public static final Value<String> LOG_NORMAL_FORMAT = new Value<>("logging.normalMessageFormat", "[{0}] {1}: {2}", Object::toString);
    public static final Value<String> LOG_PRIVATE_FORMAT = new Value<>("logging.privateMessageFormat", "[{0}] {1} -> {2}: {3}", Object::toString);
    public static final Value<Integer> LOG_RETENTION_DAYS = new Value<>("logging.retentionDays", Integer.valueOf(0), v -> ((Number) v).intValue());

    public static final Value<Boolean> UPDATE_CHECK_ENABLED = new Value<>("updates.enabled", Boolean.TRUE, v -> ((Boolean) v));
    public static final Value<Integer> UPDATE_CHECK_INTERVAL_MINUTES = new Value<>("updates.intervalMinutes", Integer.valueOf(15), v -> ((Number) v).intValue());

    public static final Value<Boolean> REDIS_ENABLED = new Value<>("redis.enabled", Boolean.FALSE, v -> ((Boolean) v));
    public static final Value<String> REDIS_HOST = new Value<>("redis.host", "127.0.0.1", Object::toString);
    public static final Value<Integer> REDIS_PORT = new Value<>("redis.port", Integer.valueOf(6379), v -> ((Number) v).intValue());
    public static final Value<String> REDIS_USERNAME = new Value<>("redis.username", "", Object::toString);
    public static final Value<String> REDIS_PASSWORD = new Value<>("redis.password", "", Object::toString);
    public static final Value<Integer> REDIS_DATABASE = new Value<>("redis.database", Integer.valueOf(0), v -> ((Number) v).intValue());
    public static final Value<Integer> REDIS_CONNECT_TIMEOUT = new Value<>("redis.connectTimeoutMillis", Integer.valueOf(3000), v -> ((Number) v).intValue());
    public static final Value<Integer> REDIS_SOCKET_TIMEOUT = new Value<>("redis.socketTimeoutMillis", Integer.valueOf(0), v -> ((Number) v).intValue());
    public static final Value<Integer> REDIS_RECONNECT_DELAY = new Value<>("redis.reconnectDelayMillis", Integer.valueOf(3000), v -> ((Number) v).intValue());
    public static final Value<String> REDIS_CHANNEL = new Value<>("redis.channel", "trchat-message", Object::toString);

    private static Map<String, Object> data() {
        Map<String, Object> cached = data;
        if (cached == null) {
            synchronized (TrChatConfig.class) {
                cached = data;
                if (cached == null) {
                    cached = load();
                    data = cached;
                }
            }
        }
        return cached;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> load() {
        Path file = Platform.configDir().resolve("trchat/settings.yml");
        if (Files.exists(file)) {
            try (InputStream in = Files.newInputStream(file)) {
                Object parsed = new Yaml().load(in);
                if (parsed instanceof Map<?, ?> map) {
                    return (Map<String, Object>) map;
                }
            } catch (IOException ignored) {
            }
        }
        return Map.of();
    }

    @SuppressWarnings("unchecked")
    private static List<? extends String> stringList(Object value) {
        if (value instanceof List<?> list) {
            return (List<? extends String>) list;
        }
        return List.of();
    }

    private TrChatConfig() {
    }

    /** Simple typed accessor over the YAML data, with a default when missing. */
    public static final class Value<T> {
        private final String key;
        private final T defaultValue;
        private final Function<Object, T> converter;

        Value(String key, T defaultValue, Function<Object, T> converter) {
            this.key = key;
            this.defaultValue = defaultValue;
            this.converter = converter;
        }

        public T get() {
            Object raw = lookup(key);
            if (raw == null) {
                return defaultValue;
            }
            try {
                return converter.apply(raw);
            } catch (RuntimeException ignored) {
                return defaultValue;
            }
        }

        public int getAsInt() {
            Object value = get();
            return value instanceof Number number ? number.intValue() : 0;
        }

        public boolean getAsBoolean() {
            Object value = get();
            return value instanceof Boolean bool && bool;
        }
    }

    @SuppressWarnings("unchecked")
    private static Object lookup(String dotted) {
        Map<String, Object> current = data();
        String[] parts = dotted.split("\\.");
        for (int i = 0; i < parts.length - 1; i++) {
            Object next = current.get(parts[i]);
            if (!(next instanceof Map)) {
                return null;
            }
            current = (Map<String, Object>) next;
        }
        return current.get(parts[parts.length - 1]);
    }
}
//? }