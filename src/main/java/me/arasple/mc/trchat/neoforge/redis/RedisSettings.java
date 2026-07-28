package me.arasple.mc.trchat.neoforge.redis;

import me.arasple.mc.trchat.neoforge.config.TrChatConfig;

public record RedisSettings(
    String host,
    int port,
    String username,
    String password,
    int database,
    int connectTimeoutMillis,
    int socketTimeoutMillis,
    int reconnectDelayMillis,
    String channel
) {

    public static RedisSettings fromConfig() {
        return new RedisSettings(
            TrChatConfig.REDIS_HOST.get(),
            TrChatConfig.REDIS_PORT.getAsInt(),
            TrChatConfig.REDIS_USERNAME.get(),
            TrChatConfig.REDIS_PASSWORD.get(),
            TrChatConfig.REDIS_DATABASE.getAsInt(),
            TrChatConfig.REDIS_CONNECT_TIMEOUT.getAsInt(),
            TrChatConfig.REDIS_SOCKET_TIMEOUT.getAsInt(),
            TrChatConfig.REDIS_RECONNECT_DELAY.getAsInt(),
            TrChatConfig.REDIS_CHANNEL.get()
        );
    }
}
