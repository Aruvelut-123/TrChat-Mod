package me.arasple.mc.trchat.neoforge.config;

import net.neoforged.neoforge.common.ModConfigSpec;

import java.util.List;

public final class TrChatConfig {

    public static final ModConfigSpec SPEC;

    public static final ModConfigSpec.IntValue SERVER_ID;
    public static final ModConfigSpec.ConfigValue<String> SERVER_NAME;
    public static final ModConfigSpec.ConfigValue<String> CHAT_FORMAT;
    public static final ModConfigSpec.ConfigValue<String> GLOBAL_FORMAT;
    public static final ModConfigSpec.ConfigValue<String> PRIVATE_FORMAT;
    public static final ModConfigSpec.ConfigValue<String> GLOBAL_PREFIX;
    public static final ModConfigSpec.IntValue MESSAGE_MAX_LENGTH;
    public static final ModConfigSpec.IntValue COOLDOWN_MILLIS;
    public static final ModConfigSpec.DoubleValue ANTI_REPEAT_SIMILARITY;
    public static final ModConfigSpec.ConfigValue<List<? extends String>> BLOCKED_WORDS;
    public static final ModConfigSpec.ConfigValue<String> FILTER_REPLACEMENT;

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
        CHAT_FORMAT = builder.define("format", "&7<%player%>&f %message%");
        GLOBAL_FORMAT = builder.define("globalFormat", "&8[&bGlobal&8] &7<%player%>&f %message%");
        PRIVATE_FORMAT = builder.define("privateFormat", "&8[&dPM&8] &7%player%&8: &f%message%");
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
