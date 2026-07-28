package me.arasple.mc.trchat.neoforge.chat;

import me.arasple.mc.trchat.neoforge.TrChatNeoForge;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import me.arasple.mc.trchat.neoforge.protocol.TrChatMessage;
import me.arasple.mc.trchat.neoforge.redis.RedisBridge;
import me.arasple.mc.trchat.neoforge.redis.RedisSettings;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;

import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

public final class ChatService implements AutoCloseable {

    private static final Duration REMOTE_PLAYER_TTL = Duration.ofSeconds(35);

    private final MinecraftServer server;
    private final Map<UUID, ChatState> chatStates = new HashMap<>();
    private final Map<String, RemoteServerPlayers> remotePlayers = new ConcurrentHashMap<>();
    private final Map<UUID, String> lastPrivateSender = new HashMap<>();
    private RedisBridge redis;
    private boolean globalMute;
    private int tickCounter;

    public ChatService(MinecraftServer server) {
        this.server = server;
        reconnectRedis();
    }

    public void handleChat(ServerPlayer player, String originalMessage) {
        String message = originalMessage.trim();
        if (message.isEmpty()) {
            return;
        }
        if (message.length() > TrChatConfig.MESSAGE_MAX_LENGTH.getAsInt()) {
            player.sendSystemMessage(Component.literal("Message is too long (maximum "
                + TrChatConfig.MESSAGE_MAX_LENGTH.getAsInt() + " characters)."));
            return;
        }
        if (globalMute && !player.hasPermissions(2)) {
            player.sendSystemMessage(Component.literal("Global chat is currently muted."));
            return;
        }

        long now = System.currentTimeMillis();
        ChatState previous = chatStates.get(player.getUUID());
        if (!player.hasPermissions(2) && previous != null) {
            long remaining = TrChatConfig.COOLDOWN_MILLIS.getAsInt() - (now - previous.sentAt());
            if (remaining > 0) {
                player.sendSystemMessage(Component.literal("Please wait " + remaining + " ms before chatting again."));
                return;
            }
            double threshold = TrChatConfig.ANTI_REPEAT_SIMILARITY.get();
            if (threshold > 0 && MessageGuard.similarity(previous.message(), message) >= threshold) {
                player.sendSystemMessage(Component.literal("Please do not repeat similar messages."));
                return;
            }
        }

        message = MessageGuard.filter(
            message,
            TrChatConfig.BLOCKED_WORDS.get(),
            TrChatConfig.FILTER_REPLACEMENT.get()
        );
        chatStates.put(player.getUUID(), new ChatState(now, message));

        String prefix = TrChatConfig.GLOBAL_PREFIX.get();
        boolean global = !prefix.isEmpty() && message.startsWith(prefix);
        if (global) {
            message = message.substring(prefix.length()).stripLeading();
            if (message.isEmpty()) {
                return;
            }
        }

        String format = global ? TrChatConfig.GLOBAL_FORMAT.get() : TrChatConfig.CHAT_FORMAT.get();
        String rendered = LegacyText.render(
            format,
            player.getGameProfile().getName(),
            player.getDisplayName().getString(),
            message,
            TrChatConfig.SERVER_ID.getAsInt()
        );
        Component component = LegacyText.parse(rendered);

        if (global && redis != null) {
            TrChatMessage packet = TrChatMessage.of(
                "BroadcastRaw",
                player.getUUID().toString(),
                ComponentJson.serialize(component, server),
                "",
                "true",
                "",
                rendered
            );
            if (redis.publish(packet)) {
                return;
            }
            player.sendSystemMessage(Component.literal("Redis is unavailable; sent to this server only."));
        }
        broadcast(component);
    }

    public int sendPrivate(ServerPlayer sender, String targetName, String rawMessage) {
        String message = rawMessage.trim();
        if (message.isEmpty()) {
            return 0;
        }
        message = MessageGuard.filter(
            message,
            TrChatConfig.BLOCKED_WORDS.get(),
            TrChatConfig.FILTER_REPLACEMENT.get()
        );

        ServerPlayer localTarget = server.getPlayerList().getPlayerByName(targetName);
        String exactTarget = localTarget != null ? localTarget.getGameProfile().getName() : exactRemoteName(targetName);
        if (exactTarget == null) {
            sender.sendSystemMessage(Component.literal("Player not found: " + targetName));
            return 0;
        }

        String rendered = LegacyText.render(
            TrChatConfig.PRIVATE_FORMAT.get(),
            sender.getGameProfile().getName(),
            sender.getDisplayName().getString(),
            message,
            TrChatConfig.SERVER_ID.getAsInt()
        );
        Component component = LegacyText.parse(rendered);
        sender.sendSystemMessage(LegacyText.parse("&8[&dPM -> " + exactTarget + "&8] &f" + message));

        if (localTarget != null) {
            localTarget.sendSystemMessage(component);
            lastPrivateSender.put(localTarget.getUUID(), sender.getGameProfile().getName());
            return 1;
        }

        if (redis == null || !redis.publish(TrChatMessage.of(
            "ForwardMessage",
            "SendPrivateRaw",
            exactTarget,
            sender.getGameProfile().getName(),
            ComponentJson.serialize(component, server),
            rendered,
            ""
        ))) {
            sender.sendSystemMessage(Component.literal("Redis is unavailable; private message was not delivered."));
            return 0;
        }
        return 1;
    }

    public int sendGlobal(ServerPlayer sender, String message) {
        handleChat(sender, TrChatConfig.GLOBAL_PREFIX.get() + message);
        return 1;
    }

    public int reply(ServerPlayer sender, String message) {
        String target = lastPrivateSender.get(sender.getUUID());
        if (target == null) {
            sender.sendSystemMessage(Component.literal("There is nobody to reply to."));
            return 0;
        }
        return sendPrivate(sender, target, message);
    }

    public void setGlobalMute(boolean muted, boolean publish) {
        globalMute = muted;
        if (publish && redis != null) {
            redis.publish(TrChatMessage.of("GlobalMute", muted ? "on" : "off"));
        }
        broadcast(Component.literal("Global chat has been " + (muted ? "muted." : "unmuted.")));
    }

    public boolean isGlobalMute() {
        return globalMute;
    }

    public void reconnectRedis() {
        if (redis != null) {
            redis.close();
            redis = null;
        }
        if (TrChatConfig.REDIS_ENABLED.getAsBoolean()) {
            redis = new RedisBridge(RedisSettings.fromConfig(), message ->
                server.execute(() -> handleRedisMessage(message)));
            redis.start();
        }
    }

    public boolean isRedisEnabled() {
        return redis != null;
    }

    public boolean isRedisConnected() {
        return redis != null && redis.isConnected();
    }

    public void tick() {
        tickCounter++;
        if (tickCounter % 200 == 0) {
            publishPlayerNames();
            expireRemotePlayers();
        }
    }

    public void playerListChanged() {
        server.execute(this::publishPlayerNames);
    }

    @Override
    public void close() {
        publishPlayerNames();
        if (redis != null) {
            redis.close();
            redis = null;
        }
        chatStates.clear();
        remotePlayers.clear();
        lastPrivateSender.clear();
    }

    private void handleRedisMessage(TrChatMessage message) {
        List<String> data = unwrap(message.data());
        if (data.isEmpty()) {
            return;
        }
        try {
            switch (data.getFirst()) {
                case "BroadcastRaw" -> receiveBroadcast(data);
                case "SendPrivateRaw" -> receivePrivate(data);
                case "UpdateNames" -> receivePlayerNames(data);
                case "GlobalMute" -> {
                    if (data.size() >= 2) {
                        globalMute = "on".equalsIgnoreCase(data.get(1));
                    }
                }
                case "SendLang" -> receiveLanguageNotice(data);
                default -> TrChatNeoForge.LOGGER.debug("Ignoring unsupported Bukkit Redis action '{}'", data.getFirst());
            }
        } catch (RuntimeException exception) {
            TrChatNeoForge.LOGGER.warn("Ignoring invalid TrChat Redis action '{}': {}",
                data.getFirst(), exception.getMessage());
        }
    }

    private void receiveBroadcast(List<String> data) {
        if (data.size() < 3) {
            return;
        }
        if (data.size() > 5 && !data.get(5).isBlank()) {
            String serverId = Integer.toString(TrChatConfig.SERVER_ID.getAsInt());
            boolean accepted = List.of(data.get(5).split(";")).contains(serverId);
            if (!accepted) {
                return;
            }
        }
        String fallback = data.size() > 6 ? data.get(6) : "";
        broadcast(ComponentJson.deserialize(data.get(2), fallback, server));
    }

    private void receivePrivate(List<String> data) {
        if (data.size() < 4) {
            return;
        }
        ServerPlayer target = server.getPlayerList().getPlayerByName(data.get(1));
        if (target == null) {
            return;
        }
        String from = data.get(2);
        String fallback = data.size() > 4 ? data.get(4) : "";
        target.sendSystemMessage(ComponentJson.deserialize(data.get(3), fallback, server));
        if (!from.isBlank()) {
            lastPrivateSender.put(target.getUUID(), from);
        }
    }

    private void receivePlayerNames(List<String> data) {
        if (data.size() < 5) {
            return;
        }
        String serverId = data.get(1);
        if (serverId.equals(Integer.toString(TrChatConfig.SERVER_ID.getAsInt()))) {
            return;
        }
        String[] names = splitProtocolList(data.get(2));
        String[] displayNames = splitProtocolList(data.get(3));
        String[] uuids = splitProtocolList(data.get(4));
        List<RemotePlayer> players = new ArrayList<>();
        for (int index = 0; index < names.length; index++) {
            if (names[index].isBlank()) {
                continue;
            }
            String displayName = index < displayNames.length && !"#".equals(displayNames[index])
                ? displayNames[index]
                : names[index];
            UUID uuid = index < uuids.length ? parseUuid(uuids[index]) : new UUID(0, 0);
            players.add(new RemotePlayer(names[index], displayName, uuid));
        }
        remotePlayers.put(serverId, new RemoteServerPlayers(System.nanoTime(), players));
    }

    private void receiveLanguageNotice(List<String> data) {
        if (data.size() < 4) {
            return;
        }
        ServerPlayer target = server.getPlayerList().getPlayerByName(data.get(1));
        if (target != null) {
            String args = data.size() > 3 ? String.join(", ", data.subList(3, data.size())) : "";
            target.sendSystemMessage(Component.literal(data.get(2) + (args.isEmpty() ? "" : ": " + args)));
        }
    }

    private void publishPlayerNames() {
        if (redis == null) {
            return;
        }
        List<ServerPlayer> players = server.getPlayerList().getPlayers();
        String names = String.join(",", players.stream().map(player -> player.getGameProfile().getName()).toList());
        String displayNames = String.join(",", players.stream()
            .map(player -> {
                String display = player.getDisplayName().getString();
                return display.isBlank() ? "#" : display.replace(",", "");
            })
            .toList());
        String uuids = String.join(",", players.stream().map(player -> player.getUUID().toString()).toList());
        redis.publish(TrChatMessage.of(
            "UpdateNames",
            Integer.toString(TrChatConfig.SERVER_ID.getAsInt()),
            names,
            displayNames,
            uuids
        ));
    }

    private String exactRemoteName(String requested) {
        expireRemotePlayers();
        for (RemoteServerPlayers serverPlayers : remotePlayers.values()) {
            for (RemotePlayer player : serverPlayers.players()) {
                if (player.name().equalsIgnoreCase(requested)
                    || player.displayName().equalsIgnoreCase(requested)) {
                    return player.name();
                }
            }
        }
        return null;
    }

    private void expireRemotePlayers() {
        long expiryNanos = System.nanoTime() - REMOTE_PLAYER_TTL.toNanos();
        remotePlayers.entrySet().removeIf(entry -> entry.getValue().updatedAtNanos() < expiryNanos);
    }

    private void broadcast(Component component) {
        server.getPlayerList().broadcastSystemMessage(component, false);
        TrChatNeoForge.LOGGER.info(component.getString());
    }

    private static List<String> unwrap(List<String> data) {
        List<String> current = data;
        while (current.size() > 1 && "ForwardMessage".equals(current.getFirst())) {
            current = current.subList(1, current.size());
        }
        return current;
    }

    private static String[] splitProtocolList(String value) {
        return value.isEmpty() ? new String[0] : value.split(",", -1);
    }

    private static UUID parseUuid(String value) {
        try {
            return UUID.fromString(value);
        } catch (IllegalArgumentException ignored) {
            if (value.length() == 32) {
                String dashed = value.substring(0, 8) + '-'
                    + value.substring(8, 12) + '-'
                    + value.substring(12, 16) + '-'
                    + value.substring(16, 20) + '-'
                    + value.substring(20);
                return UUID.fromString(dashed);
            }
            return new UUID(0, 0);
        }
    }

    private record ChatState(long sentAt, String message) {
    }

    private record RemotePlayer(String name, String displayName, UUID uuid) {
    }

    private record RemoteServerPlayers(long updatedAtNanos, List<RemotePlayer> players) {
    }
}
