package me.arasple.mc.trchat.redis;

import me.arasple.mc.trchat.config.TrChatConfig;

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
            TrChatConfig.REDIS_PORT.get(),
            TrChatConfig.REDIS_USERNAME.get(),
            TrChatConfig.REDIS_PASSWORD.get(),
            TrChatConfig.REDIS_DATABASE.get(),
            TrChatConfig.REDIS_CONNECT_TIMEOUT.get(),
            TrChatConfig.REDIS_SOCKET_TIMEOUT.get(),
            TrChatConfig.REDIS_RECONNECT_DELAY.get(),
            TrChatConfig.REDIS_CHANNEL.get()
        );
    }
}
