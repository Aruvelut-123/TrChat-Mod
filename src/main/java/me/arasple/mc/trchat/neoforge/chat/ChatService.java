package me.arasple.mc.trchat.neoforge.chat;

import me.arasple.mc.trchat.neoforge.TrChatNeoForge;
import me.arasple.mc.trchat.neoforge.channel.ChannelDefinition;
import me.arasple.mc.trchat.neoforge.channel.ChannelManager;
import me.arasple.mc.trchat.neoforge.channel.ChannelRenderer;
import me.arasple.mc.trchat.neoforge.channel.ConditionEvaluator;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import me.arasple.mc.trchat.neoforge.function.ChatFunctionService;
import me.arasple.mc.trchat.neoforge.filter.FilterService;
import me.arasple.mc.trchat.neoforge.lang.LanguageService;
import me.arasple.mc.trchat.neoforge.moderation.ModerationService;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;
import me.arasple.mc.trchat.neoforge.placeholder.PlaceholderResolver;
import me.arasple.mc.trchat.neoforge.placeholder.PlayerStatsTracker;
import me.arasple.mc.trchat.neoforge.placeholder.ServerMetrics;
import me.arasple.mc.trchat.neoforge.protocol.TrChatMessage;
import me.arasple.mc.trchat.neoforge.redis.RedisBridge;
import me.arasple.mc.trchat.neoforge.redis.RedisSettings;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.level.chunk.LevelChunk;

import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

public final class ChatService implements AutoCloseable {

    private static final Duration REMOTE_PLAYER_TTL = Duration.ofSeconds(35);

    private final MinecraftServer server;
    private final ChannelManager channels;
    private final ServerMetrics metrics = new ServerMetrics();
    private final PlayerStatsTracker playerStats = new PlayerStatsTracker();
    private final PlaceholderResolver placeholders;
    private final ChannelRenderer renderer;
    private final ChatFunctionService functions;
    private final FilterService filters;
    private final ModerationService moderation;
    private final Map<UUID, ChatState> chatStates = new HashMap<>();
    private final Map<UUID, String> activeChannels = new HashMap<>();
    private final Map<UUID, Set<String>> joinedChannels = new HashMap<>();
    private final Map<String, RemoteServerPlayers> remotePlayers = new ConcurrentHashMap<>();
    private final Map<UUID, String> lastPrivateSender = new HashMap<>();
    private RedisBridge redis;
    private boolean globalMute;
    private int tickCounter;

    public ChatService(MinecraftServer server, ChannelManager channels) {
        this.server = server;
        this.channels = channels;
        channels.reload();
        this.placeholders = new PlaceholderResolver(server, metrics, playerStats);
        this.renderer = new ChannelRenderer(placeholders);
        this.moderation = new ModerationService();
        this.functions = new ChatFunctionService(server, moderation.languages());
        this.functions.reload();
        this.filters = new FilterService(server, moderation.languages());
        this.filters.reload();
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            playerJoined(player);
        }
        reconnectRedis();
    }

    public void handleChat(ServerPlayer player, String originalMessage) {
        String message = originalMessage.trim();
        if (message.isEmpty()) {
            return;
        }
        ChannelDefinition channel;
        ChannelManager.PrefixMatch match = channels.byPrefix(message);
        if (match != null) {
            channel = match.channel();
            message = message.substring(match.prefix().length()).stripLeading();
        } else {
            channel = channels.byId(activeChannels.getOrDefault(player.getUUID(), "Normal"))
                .orElseGet(channels::normal);
        }
        if (message.isEmpty() || channel.options().privateChannel() || channel.id().equalsIgnoreCase("Server")) {
            return;
        }
        sendPublic(player, channel, message);
    }

    public int executeChannel(ServerPlayer player, ChannelDefinition channel, String message) {
        if (channel.options().privateChannel()) {
            sendLang(player, "Channel-Private-Target");
            return 0;
        }
        if (channel.id().equalsIgnoreCase("Server")) {
            sendLang(player, "Channel-Server-Say-Only");
            return 0;
        }
        if (message == null || message.isBlank()) {
            return toggleChannel(player, channel);
        }
        return sendPublic(player, channel, message) ? 1 : 0;
    }

    public int sendServer(String message, ServerPlayer sourcePlayer) {
        ChannelDefinition channel = channels.byId("Server").orElse(null);
        if (channel == null || message == null || message.isBlank()) {
            return 0;
        }
        ChannelRenderer.Rendered rendered = renderer.render(
            channel,
            ChannelRenderer.Audience.CONSOLE,
            sourcePlayer,
            message.trim(),
            Map.of("message", message.trim())
        );
        // Server is a deliberately local-only channel. Never publish it to Redis.
        server.getPlayerList().broadcastSystemMessage(rendered.component(), false);
        TrChatNeoForge.LOGGER.info("[Server] {}", rendered.component().getString());
        return 1;
    }

    public int sendPrivate(ServerPlayer sender, String targetName, String rawMessage) {
        ChannelDefinition channel = channels.byId("Private").orElse(null);
        if (channel == null || !canSpeak(sender, channel)) {
            sendLang(sender, "Channel-No-Speak-Permission");
            return 0;
        }
        String message = guardMessage(sender, rawMessage);
        if (message == null) {
            return 0;
        }

        ServerPlayer localTarget = server.getPlayerList().getPlayerByName(targetName);
        String exactTarget = localTarget != null ? localTarget.getGameProfile().getName() : exactRemoteName(targetName);
        if (exactTarget == null) {
            sendLang(sender, "General-Player-Not-Found", targetName);
            return 0;
        }

        Map<String, String> local = Map.of("trchat_toplayer", exactTarget, "message", message);
        ChatFunctionService.ProcessedMessage processed = functions.process(
            sender, message, channel.options().disabledFunctions()
        );
        ChannelRenderer.Rendered senderView = renderer.render(
            channel, ChannelRenderer.Audience.SENDER, sender, processed.component(), message, local
        );
        ChannelRenderer.Rendered receiverView = renderer.render(
            channel, ChannelRenderer.Audience.RECEIVER, sender, processed.component(), message, local
        );
        sender.sendSystemMessage(senderView.component());
        lastPrivateSender.put(sender.getUUID(), exactTarget);

        if (moderation.shadowMuted(sender)) {
            logToConsole(channel, sender, message, local);
            return 1;
        }

        if (localTarget != null) {
            localTarget.sendSystemMessage(receiverView.component());
            lastPrivateSender.put(localTarget.getUUID(), sender.getGameProfile().getName());
            notifyPrivateSpies(
                sender, sender.getGameProfile().getName(), exactTarget, message, localTarget
            );
            logToConsole(channel, sender, message, local);
            return 1;
        }

        if (!channel.options().redis() || redis == null || !redis.publish(TrChatMessage.of(
            "ForwardMessage",
            "SendPrivateRaw",
            exactTarget,
            sender.getGameProfile().getName(),
            ComponentJson.serialize(receiverView.component(), server),
            receiverView.fallback(),
            ""
        ))) {
            sendLang(sender, "Redis-Private-Unavailable");
            return 0;
        }
        notifyPrivateSpies(sender, sender.getGameProfile().getName(), exactTarget, message, null);
        logToConsole(channel, sender, message, local);
        return 1;
    }

    public int reply(ServerPlayer sender, String message) {
        String target = lastPrivateSender.get(sender.getUUID());
        if (target == null) {
            sendLang(sender, "Private-Message-No-Reply");
            return 0;
        }
        return sendPrivate(sender, target, message);
    }

    public void setGlobalMute(boolean muted, boolean publish) {
        globalMute = muted;
        if (publish && redis != null) {
            redis.publish(TrChatMessage.of("GlobalMute", muted ? "on" : "off"));
        }
        String key = muted ? "Global-Mute-On" : "Global-Mute-Off";
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            player.sendSystemMessage(moderation.languages().component(player, key));
        }
        TrChatNeoForge.LOGGER.info(LegacyText.stripLegacyCodes(moderation.languages().text(null, key)));
    }

    public boolean isGlobalMute() {
        return globalMute;
    }

    public ReloadResult reloadConfiguration() {
        int loaded = channels.reload();
        if (loaded < 0) {
            return new ReloadResult(false, -1, List.of("channels"));
        }
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            restoreChannelMembership(player);
        }

        List<String> failed = new ArrayList<>();
        if (!functions.reload()) failed.add("function.yml");
        if (!filters.reload()) failed.add("filter.yml");
        if (!moderation.reloadLanguages()) failed.add("lang");
        reconnectRedis();
        return new ReloadResult(failed.isEmpty(), loaded, List.copyOf(failed));
    }

    public int channelCount() {
        return channels.all().size();
    }

    public boolean openFunctionSnapshot(ServerPlayer player, String id) {
        return functions.openSnapshot(player, id);
    }

    public boolean checkCommand(ServerPlayer player, String commandLine) {
        return functions.checkCommand(player, commandLine);
    }

    public boolean isCommandManaged(String commandLine) {
        return functions.isCommandManaged(commandLine);
    }

    public boolean isCommandControllerEnabled() {
        return functions.commandControllerEnabled();
    }

    public int commandRuleCount() {
        return functions.commandRuleCount();
    }

    public String autoJoinChannel() {
        return channels.autoJoin().map(ChannelDefinition::id).orElse("-");
    }

    public void chunkLoaded(LevelChunk chunk) {
        filters.chunkLoaded(chunk);
    }

    public void chunkUnloaded(LevelChunk chunk) {
        filters.chunkUnloaded(chunk);
    }

    public boolean checkAnvil(ServerPlayer player, String name) {
        return filters.checkAnvil(player, name);
    }

    public LanguageService languages() {
        return moderation.languages();
    }

    public void mutePlayer(ServerPlayer player, long durationMillis, String reason) {
        moderation.mute(player, durationMillis, reason);
    }

    public void unmutePlayer(ServerPlayer player) {
        moderation.unmute(player);
    }

    public boolean isShadowMuted(ServerPlayer player) {
        return moderation.shadowMuted(player);
    }

    public boolean isMuted(ServerPlayer player) {
        return moderation.isMuted(player);
    }

    public String muteExpiry(ServerPlayer player) {
        return moderation.muteExpiry(player);
    }

    public String muteReason(ServerPlayer player) {
        return moderation.muteReason(player);
    }

    public boolean isPrivateSpy(ServerPlayer player) {
        return moderation.privateSpy(player);
    }

    public String activeChannel(ServerPlayer player) {
        String active = moderation.activeChannel(player);
        return active.isBlank() ? "-" : active;
    }

    public int joinedChannelCount(ServerPlayer player) {
        return moderation.joinedChannels(player).size();
    }

    public void setShadowMuted(ServerPlayer player, boolean value) {
        moderation.setShadowMuted(player, value);
    }

    public boolean setPrivateSpy(ServerPlayer player, Boolean value) {
        return value == null ? moderation.togglePrivateSpy(player) : moderation.setPrivateSpy(player, value);
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
        metrics.tick();
        filters.tick();
        tickCounter++;
        if (tickCounter % 200 == 0) {
            publishPlayerNames();
            expireRemotePlayers();
        }
    }

    public void playerJoined(ServerPlayer player) {
        moderation.playerJoined(player);
        restoreChannelMembership(player);
        playerListChanged();
    }

    public void playerLeft(ServerPlayer player) {
        persistChannelMembership(player);
        moderation.playerLeft(player);
        playerStats.remove(player.getUUID());
        activeChannels.remove(player.getUUID());
        joinedChannels.remove(player.getUUID());
        playerListChanged();
    }

    public void recordDamage(ServerPlayer player, float damage) {
        playerStats.recordDamage(player, damage);
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
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            persistChannelMembership(player);
        }
        chatStates.clear();
        activeChannels.clear();
        joinedChannels.clear();
        remotePlayers.clear();
        lastPrivateSender.clear();
        moderation.close();
    }

    private boolean sendPublic(ServerPlayer player, ChannelDefinition channel, String rawMessage) {
        if (!canSpeak(player, channel)) {
            sendLang(player, "Channel-No-Speak-Permission");
            return false;
        }
        String message = guardMessage(player, rawMessage);
        if (message == null) {
            return false;
        }

        ChatFunctionService.ProcessedMessage processed = functions.process(
            player, message, channel.options().disabledFunctions()
        );
        ChannelRenderer.Rendered rendered = renderer.render(
            channel,
            ChannelRenderer.Audience.CHAT,
            player,
            processed.component(),
            message,
            Map.of("message", message)
        );

        if (moderation.shadowMuted(player)) {
            player.sendSystemMessage(rendered.component());
            logToConsole(channel, player, message, Map.of("message", message));
            return true;
        }

        if (channel.options().redis() && redis != null) {
            TrChatMessage packet = TrChatMessage.of(
                "BroadcastRaw",
                player.getUUID().toString(),
                ComponentJson.serialize(rendered.component(), server),
                channel.options().listenPermission(),
                Boolean.toString(channel.options().doubleTransfer()),
                String.join(";", channel.options().ports()),
                rendered.fallback()
            );
            if (redis.publish(packet)) {
                logToConsole(channel, player, message, Map.of("message", message));
                return true;
            }
            if (channel.options().forceRedis()) {
                sendLang(player, "Redis-Force-Unavailable");
                return false;
            }
            sendLang(player, "Redis-Fallback");
        }

        broadcastLocal(channel, player, rendered.component());
        logToConsole(channel, player, message, Map.of("message", message));
        return true;
    }

    private String guardMessage(ServerPlayer player, String rawMessage) {
        String message = rawMessage == null ? "" : rawMessage.trim();
        if (message.isEmpty()) {
            return null;
        }
        if (message.length() > TrChatConfig.MESSAGE_MAX_LENGTH.getAsInt()) {
            sendLang(player, "General-Too-Long", message.length(), TrChatConfig.MESSAGE_MAX_LENGTH.getAsInt());
            return null;
        }
        if (globalMute && !player.hasPermissions(2)) {
            sendLang(player, "General-Global-Muting");
            return null;
        }
        if (moderation.isMuted(player)) {
            sendLang(player, "General-Muted", moderation.muteExpiry(player), moderation.muteReason(player));
            return null;
        }

        long now = System.currentTimeMillis();
        ChatState previous = chatStates.get(player.getUUID());
        if (!player.hasPermissions(2) && previous != null) {
            long remaining = TrChatConfig.COOLDOWN_MILLIS.getAsInt() - (now - previous.sentAt());
            if (remaining > 0) {
                sendLang(player, "Cooldowns-Chat", remaining);
                return null;
            }
            double threshold = TrChatConfig.ANTI_REPEAT_SIMILARITY.get();
            if (threshold > 0 && MessageGuard.similarity(previous.message(), message) >= threshold) {
                sendLang(player, "General-Too-Similar");
                return null;
            }
        }
        message = filters.filterChat(player, message);
        message = MessageGuard.filter(message, TrChatConfig.BLOCKED_WORDS.get(), TrChatConfig.FILTER_REPLACEMENT.get());
        chatStates.put(player.getUUID(), new ChatState(now, message));
        return message;
    }

    private int toggleChannel(ServerPlayer player, ChannelDefinition channel) {
        String id = channel.id().toLowerCase(Locale.ROOT);
        Set<String> joined = joinedChannels.computeIfAbsent(player.getUUID(), ignored -> new HashSet<>());
        if (activeChannels.getOrDefault(player.getUUID(), "Normal").equalsIgnoreCase(channel.id())) {
            if (!channel.options().alwaysListen()) {
                joined.remove(id);
            }
            String fallback = fallbackChannel(player, joined);
            joined.add(fallback.toLowerCase(Locale.ROOT));
            activeChannels.put(player.getUUID(), fallback);
            persistChannelMembership(player);
            sendLang(player, "Channel-Quit", channel.id());
            return 1;
        }
        if (!hasPermission(player, channel.options().joinPermission())) {
            sendLang(player, "Channel-No-Join-Permission", channel.id());
            return 0;
        }
        activeChannels.put(player.getUUID(), channel.id());
        joined.add(id);
        persistChannelMembership(player);
        sendLang(player, "Channel-Join", channel.id());
        return 1;
    }

    private boolean canSpeak(ServerPlayer player, ChannelDefinition channel) {
        String condition = channel.options().speakCondition();
        return condition.isBlank()
            ? hasPermission(player, channel.options().joinPermission())
            : ConditionEvaluator.test(condition, player);
    }

    private boolean canListen(ServerPlayer player, ChannelDefinition channel) {
        String permission = channel.options().listenPermission().isBlank()
            ? channel.options().joinPermission()
            : channel.options().listenPermission();
        return hasPermission(player, permission);
    }

    private static boolean hasPermission(ServerPlayer player, String permission) {
        return permission == null || permission.isBlank() || TrChatPermissions.check(player, permission);
    }

    private void broadcastLocal(ChannelDefinition channel, ServerPlayer sender, Component component) {
        String id = channel.id().toLowerCase(Locale.ROOT);
        String target = channel.options().target();
        String[] range = target.split(";", 2);
        int distance = range.length == 2 ? parsePositiveInt(range[1]) : -1;

        for (ServerPlayer receiver : server.getPlayerList().getPlayers()) {
            Set<String> joined = joinedChannels.getOrDefault(receiver.getUUID(), Set.of());
            boolean listening = channel.options().alwaysListen()
                || joined.contains(id);
            if (!listening || !canListen(receiver, channel)) {
                continue;
            }
            boolean inRange = switch (range[0]) {
                case "SELF" -> receiver.getUUID().equals(sender.getUUID());
                case "SINGLE_WORLD", "WORLD" -> receiver.serverLevel() == sender.serverLevel();
                case "DISTANCE" -> receiver.serverLevel() == sender.serverLevel()
                    && distance >= 0
                    && receiver.distanceToSqr(sender) <= (double) distance * distance;
                default -> true;
            };
            if (inRange) {
                receiver.sendSystemMessage(component);
            }
        }
    }

    private void restoreChannelMembership(ServerPlayer player) {
        Set<String> joined = new HashSet<>(moderation.joinedChannels(player));
        joined.removeIf(id -> channels.byId(id)
            .map(channel -> channel.options().privateChannel() || channel.id().equalsIgnoreCase("Server"))
            .orElse(true));

        if (joined.isEmpty()) {
            ChannelDefinition initial = channels.autoJoin()
                .filter(channel -> hasPermission(player, channel.options().joinPermission()))
                .orElseGet(channels::normal);
            joined.add(initial.id().toLowerCase(Locale.ROOT));
        }

        String savedActive = moderation.activeChannel(player);
        String active = channels.byId(savedActive)
            .filter(channel -> joined.contains(channel.id().toLowerCase(Locale.ROOT)))
            .filter(channel -> hasPermission(player, channel.options().joinPermission()))
            .map(ChannelDefinition::id)
            .orElseGet(() -> fallbackChannel(player, joined));
        joined.add(active.toLowerCase(Locale.ROOT));

        joinedChannels.put(player.getUUID(), joined);
        activeChannels.put(player.getUUID(), active);
        persistChannelMembership(player);
    }

    private String fallbackChannel(ServerPlayer player, Set<String> joined) {
        if (joined.contains("normal")) {
            return channels.normal().id();
        }
        return joined.stream()
            .sorted()
            .map(channels::byId)
            .flatMap(java.util.Optional::stream)
            .filter(channel -> !channel.options().privateChannel())
            .filter(channel -> !channel.id().equalsIgnoreCase("Server"))
            .filter(channel -> hasPermission(player, channel.options().joinPermission()))
            .map(ChannelDefinition::id)
            .findFirst()
            .orElseGet(() -> channels.normal().id());
    }

    private void persistChannelMembership(ServerPlayer player) {
        Set<String> joined = joinedChannels.get(player.getUUID());
        String active = activeChannels.get(player.getUUID());
        if (joined != null && active != null) {
            moderation.setChannels(player, active, Set.copyOf(joined));
        }
    }

    private void logToConsole(
        ChannelDefinition channel,
        ServerPlayer player,
        String message,
        Map<String, String> local
    ) {
        if (channel.consoleFormats().isEmpty()) {
            TrChatNeoForge.LOGGER.info("[{}] {}", channel.id(), renderer.render(
                channel, ChannelRenderer.Audience.CHAT, player, message, local
            ).component().getString());
            return;
        }
        Component console = renderer.render(
            channel, ChannelRenderer.Audience.CONSOLE, player, message, local
        ).component();
        TrChatNeoForge.LOGGER.info("[{}] {}", channel.id(), console.getString());
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
        Component component = ComponentJson.deserialize(data.get(2), fallback, server);
        String permission = data.size() > 3 ? data.get(3) : "";
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            if (hasPermission(player, permission)) {
                player.sendSystemMessage(component);
            }
        }
        TrChatNeoForge.LOGGER.info(component.getString());
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
        Component component = ComponentJson.deserialize(data.get(3), fallback, server);
        target.sendSystemMessage(component);
        if (!from.isBlank()) {
            lastPrivateSender.put(target.getUUID(), from);
            notifyPrivateSpies(null, from, target.getGameProfile().getName(), component.getString(), target);
        }
    }

    private void notifyPrivateSpies(
        ServerPlayer sender,
        String senderName,
        String targetName,
        String message,
        ServerPlayer target
    ) {
        for (ServerPlayer observer : server.getPlayerList().getPlayers()) {
            if (!moderation.privateSpy(observer)
                || sender != null && observer.getUUID().equals(sender.getUUID())
                || target != null && observer.getUUID().equals(target.getUUID())) {
                continue;
            }
            sendLang(observer, "Private-Message-Spy-Format", senderName, targetName, message);
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

    private void sendLang(ServerPlayer player, String key, Object... arguments) {
        player.sendSystemMessage(moderation.languages().component(player, key, arguments));
    }

    private static List<String> unwrap(List<String> data) {
        List<String> current = data;
        while (current.size() > 1 && "ForwardMessage".equals(current.getFirst())) {
            current = current.subList(1, current.size());
        }
        return current;
    }

    private static int parsePositiveInt(String value) {
        try {
            return Math.max(0, Integer.parseInt(value));
        } catch (NumberFormatException ignored) {
            return -1;
        }
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

    public record ReloadResult(boolean success, int channelCount, List<String> failedSections) {
    }
}
