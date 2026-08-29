package me.arasple.mc.trchat.chat;

import me.arasple.mc.trchat.TrChatMod;
import me.arasple.mc.trchat.channel.ChannelDefinition;
import me.arasple.mc.trchat.channel.ChannelManager;
import me.arasple.mc.trchat.channel.ChannelRenderer;
import me.arasple.mc.trchat.channel.ConditionEvaluator;
import me.arasple.mc.trchat.config.TrChatConfig;
import me.arasple.mc.trchat.config.SpecialChars;
import me.arasple.mc.trchat.data.PlayerDataStore;
import me.arasple.mc.trchat.data.ChatLogService;
import me.arasple.mc.trchat.function.ChatFunctionService;
import me.arasple.mc.trchat.filter.FilterService;
import me.arasple.mc.trchat.lang.LanguageService;
import me.arasple.mc.trchat.moderation.ModerationService;
import me.arasple.mc.trchat.permission.TrChatPermissions;
import me.arasple.mc.trchat.placeholder.PlaceholderResolver;
import me.arasple.mc.trchat.placeholder.PlayerStatsTracker;
import me.arasple.mc.trchat.placeholder.ServerMetrics;
import me.arasple.mc.trchat.protocol.TrChatMessage;
import me.arasple.mc.trchat.protocol.TrChatProtocol;
import me.arasple.mc.trchat.redis.RedisBridge;
import me.arasple.mc.trchat.redis.RedisSettings;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.level.chunk.LevelChunk;

import java.time.Duration;
import java.util.ArrayList;
import java.util.Collection;
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
    private final ChatLogService chatLogs = new ChatLogService();
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
        SpecialChars.reload();
        this.moderation = new ModerationService();
        this.placeholders = new PlaceholderResolver(server, metrics, playerStats, moderation.languages());
        this.renderer = new ChannelRenderer(placeholders);
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
        if (message.isEmpty() || channel.options().privateChannel()) {
            return;
        }
        sendPublic(player, channel, message);
    }

    public int executeChannel(ServerPlayer player, ChannelDefinition channel, String message) {
        if (channel.options().privateChannel()) {
            sendLang(player, "Channel-Private-Target");
            return 0;
        }
        if (message == null || message.isBlank()) {
            return toggleChannel(player, channel);
        }
        return sendPublic(player, channel, message) ? 1 : 0;
    }

    public int sendConsole(String message) {
        ChannelDefinition channel = channels.autoJoin().orElse(null);
        if (channel == null || message == null || message.isBlank()) {
            return 0;
        }
        String normalized = message.trim();
        Map<String, String> defaultContext = consoleMessageContext(null, normalized);
        ChannelRenderer.Rendered rendered = renderer.render(
            channel, ChannelRenderer.Audience.CHAT, null, (ServerPlayer) null, normalized, defaultContext
        );

        if (channel.options().redis() && redis != null) {
            TrChatMessage packet = TrChatMessage.of(
                "BroadcastRaw",
                TrChatProtocol.formatUuid(TrChatProtocol.NIL_UUID),
                ComponentJson.serialize(rendered.component(), server),
                channel.options().listenPermission(),
                Boolean.toString(channel.options().doubleTransfer()),
                String.join(";", channel.options().ports()),
                rendered.fallback()
            );
            if (redis.publish(packet)) {
                chatLogs.logNormal(moderation.languages().text(null, "Console-Name"), normalized);
                return 1;
            }
            if (channel.options().forceRedis()) {
                return 0;
            }
        }

        broadcastConsoleLocal(channel, normalized);
        chatLogs.logNormal(moderation.languages().text(null, "Console-Name"), normalized);
        TrChatMod.LOGGER.info("{}", rendered.component().getString());
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

        Map<String, String> local = messageContext(sender, message);
        local.put("trchat_toplayer", exactTarget);
        ChatFunctionService.ProcessedMessage processed = functions.process(
            sender, message, channel.options().disabledFunctions()
        );
        ChannelRenderer.Rendered senderView = renderer.render(
            channel, ChannelRenderer.Audience.SENDER, sender, processed.component(), message, local
        );
        ChannelRenderer.Rendered receiverView = renderer.render(
            channel,
            ChannelRenderer.Audience.RECEIVER,
            sender,
            localTarget,
            processed.component(),
            message,
            local
        );
        boolean shadowMuted = moderation.shadowMuted(sender);
        if (!shadowMuted && localTarget == null && !processed.crossServerSafe()) {
            sendLang(sender, "Redis-Unsafe-Item");
            return 0;
        }
        sender.sendSystemMessage(senderView.component());

        if (shadowMuted) {
            logToConsole(channel, sender, message, local);
            return 1;
        }

        if (localTarget != null) {
            List<String> mentioned = processed.mentionedPlayers();
            if (!moderation.hasIgnored(localTarget, sender.getUUID())) {
                localTarget.sendSystemMessage(receiverView.component());
                lastPrivateSender.put(localTarget.getUUID(), sender.getGameProfile().getName());
                if (mentioned.stream().anyMatch(name -> localTarget.getGameProfile().getName().equalsIgnoreCase(name))) {
                    functions.notifyMention(localTarget, sender.getGameProfile().getName());
                }
            }
            notifyPrivateSpies(
                sender, sender.getGameProfile().getName(), exactTarget, message, localTarget
            );
            logToConsole(channel, sender, message, local);
            return 1;
        }

        if (!channel.options().redis() || redis == null || !redis.publish(TrChatProtocol.forwardPrivate(
            exactTarget,
            sender.getGameProfile().getName(),
            ComponentJson.serialize(receiverView.component(), server),
            receiverView.fallback(),
            ComponentJson.serialize(processed.component(), server)
        ))) {
            sendLang(sender, "Redis-Private-Unavailable");
            return 0;
        }
        if (processed.mentionedPlayers().stream().anyMatch(name -> exactTarget.equalsIgnoreCase(name))) {
            redis.publish(TrChatMessage.of(
                "SendLang",
                exactTarget,
                "Function-Mention-Notify",
                sender.getGameProfile().getName()
            ));
        }
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

    public int setIgnored(ServerPlayer player, String targetName, Boolean requested) {
        PlayerIdentity target = findKnownPlayer(targetName);
        if (target == null) {
            target = moderation.ignoredPlayers(player).stream()
                .filter(ignored -> ignored.name().equalsIgnoreCase(targetName))
                .map(ignored -> new PlayerIdentity(ignored.uuid(), ignored.name()))
                .findFirst()
                .orElse(null);
        }
        if (target == null) {
            sendLang(player, "General-Player-Not-Found", targetName);
            return 0;
        }
        if (target.uuid().equals(player.getUUID())) {
            sendLang(player, "Ignore-Self");
            return 0;
        }
        boolean ignored = requested == null
            ? moderation.toggleIgnored(player, target.uuid(), target.name())
            : moderation.setIgnored(player, target.uuid(), target.name(), requested);
        sendLang(player, ignored ? "Ignore-Ignored-Player" : "Ignore-Cancel-Player", target.name());
        return 1;
    }

    public List<String> ignoredPlayerNames(ServerPlayer player) {
        return moderation.ignoredPlayers(player).stream()
            .map(PlayerDataStore.IgnoredPlayer::name)
            .sorted(String.CASE_INSENSITIVE_ORDER)
            .toList();
    }

    public List<String> knownPlayerNames() {
        expireRemotePlayers();
        List<String> names = new ArrayList<>(server.getPlayerList().getPlayers().stream()
            .map(player -> player.getGameProfile().getName())
            .toList());
        remotePlayers.values().stream()
            .flatMap(snapshot -> snapshot.players().stream())
            .map(RemotePlayer::name)
            .filter(name -> names.stream().noneMatch(existing -> existing.equalsIgnoreCase(name)))
            .forEach(names::add);
        return names.stream().sorted(String.CASE_INSENSITIVE_ORDER).toList();
    }

    public List<String> availableChatColors(ServerPlayer player) {
        return "0123456789abcdef".chars()
            .mapToObj(code -> Character.toString((char) code))
            //? if >=1.21.11 {
            .filter(code -> player.permissions().hasPermission(
                new net.minecraft.server.permissions.Permission.HasCommandLevel(
                    net.minecraft.server.permissions.PermissionLevel.byId(2)
                )
            ) || hasPermission(player, "trchat.color." + code))
            //? } else {
            .filter(code -> player.hasPermissions(2) || hasPermission(player, "trchat.color." + code))
            //? }
            .toList();
    }

    public int setChatColor(ServerPlayer player, String requested) {
        String color = requested == null ? "" : requested.trim().toLowerCase(Locale.ROOT);
        if (color.equals("reset") || color.equals("null") || color.equals("default")) {
            moderation.setChatColor(player, "");
            sendLang(player, "Color-Reset");
            return 1;
        }
        if (!color.matches("[0-9a-f]")) {
            sendLang(player, "Color-Invalid", requested);
            return 0;
        }
        //? if >=1.21.11 {
        if (!player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        ) && !hasPermission(player, "trchat.color." + color)) {
        //? } else {
        if (!player.hasPermissions(2) && !hasPermission(player, "trchat.color." + color)) {
        //? }
            sendLang(player, "General-No-Permission");
            return 0;
        }
        moderation.setChatColor(player, color);
        sendLang(player, "Color-Selected", "&" + color + color);
        return 1;
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
        TrChatMod.LOGGER.info(LegacyText.stripLegacyCodes(moderation.languages().text(null, key)));
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
        SpecialChars.reload();
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

    public int setChannel(ServerPlayer player, ChannelDefinition channel) {
        if (!channel.isJoinable() || !hasPermission(player, channel.options().joinPermission())) {
            sendLang(player, "Channel-No-Join-Permission", channel.id());
            return 0;
        }
        Set<String> joined = joinedChannels.computeIfAbsent(player.getUUID(), ignored -> new HashSet<>());
        activeChannels.put(player.getUUID(), channel.id());
        joined.add(channel.id().toLowerCase(Locale.ROOT));
        persistChannelMembership(player);
        sendLang(player, "Channel-Join", channel.id());
        return 1;
    }

    public int quitChannel(ServerPlayer player) {
        String previousId = activeChannels.getOrDefault(player.getUUID(), channels.normal().id());
        ChannelDefinition previous = channels.byId(previousId).orElseGet(channels::normal);
        Set<String> joined = joinedChannels.computeIfAbsent(player.getUUID(), ignored -> new HashSet<>());
        if (!previous.options().alwaysListen()) {
            joined.remove(previous.id().toLowerCase(Locale.ROOT));
        }
        ChannelDefinition fallback = channels.autoJoin()
            .filter(channel -> hasPermission(player, channel.options().joinPermission()))
            .orElseGet(channels::normal);
        joined.add(fallback.id().toLowerCase(Locale.ROOT));
        activeChannels.put(player.getUUID(), fallback.id());
        persistChannelMembership(player);
        sendLang(player, "Channel-Quit", previous.id());
        if (!previous.id().equalsIgnoreCase(fallback.id())) {
            sendLang(player, "Channel-Join", fallback.id());
        }
        return 1;
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
        chatLogs.tick();
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
        chatLogs.close();
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
        List<String> mentioned = processed.mentionedPlayers();
        Map<String, String> local = messageContext(player, message);
        ChannelRenderer.Rendered rendered = renderer.render(
            channel,
            ChannelRenderer.Audience.CHAT,
            player,
            null,
            processed.component(),
            message,
            local
        );

        if (moderation.shadowMuted(player)) {
            player.sendSystemMessage(renderer.render(
                channel,
                ChannelRenderer.Audience.CHAT,
                player,
                player,
                processed.component(),
                message,
                local
            ).component());
            logToConsole(channel, player, message, local);
            return true;
        }

        if (channel.options().redis() && redis != null && processed.crossServerSafe()) {
            TrChatMessage packet = TrChatMessage.of(
                "BroadcastRaw",
                TrChatProtocol.formatUuid(player.getUUID()),
                ComponentJson.serialize(rendered.component(), server),
                channel.options().listenPermission(),
                Boolean.toString(channel.options().doubleTransfer()),
                String.join(";", channel.options().ports()),
                rendered.fallback(),
                player.getGameProfile().getName(),
                String.join(",", mentioned)
            );
            if (redis.publish(packet)) {
                return true;
            }
            if (channel.options().forceRedis()) {
                sendLang(player, "Redis-Force-Unavailable");
                return false;
            }
            sendLang(player, "Redis-Fallback");
        }

        List<ServerPlayer> receivers = broadcastLocal(channel, player, processed.component(), message, local);
        notifyMentioned(receivers, mentioned, player.getGameProfile().getName());
        logToConsole(channel, player, message, local);
        return true;
    }

    private Map<String, String> messageContext(ServerPlayer player, String message) {
        Map<String, String> local = new HashMap<>();
        local.put("message", message);
        String selected = moderation.chatColor(player);
        if (!selected.isBlank()
            //? if >=1.21.11 {
            && (player.permissions().hasPermission(
                new net.minecraft.server.permissions.Permission.HasCommandLevel(
                    net.minecraft.server.permissions.PermissionLevel.byId(2)
                )
            ) || hasPermission(player, "trchat.color." + selected))) {
            //? } else {
            && (player.hasPermissions(2) || hasPermission(player, "trchat.color." + selected))) {
            //? }
            local.put(ChannelRenderer.MESSAGE_COLOR, selected);
        }
        return local;
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
        //? if >=1.21.11 {
        if (globalMute && !player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        )) {
        //? } else {
        if (globalMute && !player.hasPermissions(2)) {
        //? }
            sendLang(player, "General-Global-Muting");
            return null;
        }
        if (moderation.isMuted(player)) {
            sendLang(player, "General-Muted", moderation.muteExpiry(player), moderation.muteReason(player));
            return null;
        }

        long now = System.currentTimeMillis();
        ChatState previous = chatStates.get(player.getUUID());
        //? if >=1.21.11 {
        if (!player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        ) && previous != null) {
        //? } else {
        if (!player.hasPermissions(2) && previous != null) {
        //? }
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
        if (!channel.isJoinable()) {
            sendLang(player, "Channel-Private-Target");
            return 0;
        }
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

    private List<ServerPlayer> broadcastLocal(
        ChannelDefinition channel,
        ServerPlayer sender,
        Component messageComponent,
        String message,
        Map<String, String> local
    ) {
        String id = channel.id().toLowerCase(Locale.ROOT);
        String target = channel.options().target();
        String[] range = target.split(";", 2);
        int distance = range.length == 2 ? parsePositiveInt(range[1]) : -1;
        List<ServerPlayer> receivers = new ArrayList<>();

        for (ServerPlayer receiver : server.getPlayerList().getPlayers()) {
            if (moderation.hasIgnored(receiver, sender.getUUID())) {
                continue;
            }
            Set<String> joined = joinedChannels.getOrDefault(receiver.getUUID(), Set.of());
            boolean listening = channel.options().alwaysListen()
                || joined.contains(id);
            if (!listening || !canListen(receiver, channel)) {
                continue;
            }
            boolean inRange = switch (range[0]) {
                case "SELF" -> receiver.getUUID().equals(sender.getUUID());
                //? if >=1.21.11 {
                case "SINGLE_WORLD", "WORLD" -> receiver.level() == sender.level();
                case "DISTANCE" -> receiver.level() == sender.level()
                    && distance >= 0
                    && receiver.distanceToSqr(sender) <= (double) distance * distance;
                //? } else {
                case "SINGLE_WORLD", "WORLD" -> receiver.serverLevel() == sender.serverLevel();
                case "DISTANCE" -> receiver.serverLevel() == sender.serverLevel()
                    && distance >= 0
                    && receiver.distanceToSqr(sender) <= (double) distance * distance;
                //? }
                default -> true;
            };
            if (inRange) {
                receiver.sendSystemMessage(renderer.render(
                    channel,
                    ChannelRenderer.Audience.CHAT,
                    sender,
                    receiver,
                    messageComponent,
                    message,
                    local
                ).component());
                receivers.add(receiver);
            }
        }
        return receivers;
    }

    /** 只有真正收到（能看到）该消息且被 @ 的玩家才会收到提示 */
    private void notifyMentioned(
        Collection<ServerPlayer> receivers,
        List<String> mentioned,
        String senderName
    ) {
        if (mentioned == null || mentioned.isEmpty()) {
            return;
        }
        for (ServerPlayer receiver : receivers) {
            if (mentioned.stream().anyMatch(name -> receiver.getGameProfile().getName().equalsIgnoreCase(name))) {
                functions.notifyMention(receiver, senderName);
            }
        }
    }

    private void broadcastConsoleLocal(ChannelDefinition channel, String message) {
        String id = channel.id().toLowerCase(Locale.ROOT);
        for (ServerPlayer receiver : server.getPlayerList().getPlayers()) {
            Set<String> joined = joinedChannels.getOrDefault(receiver.getUUID(), Set.of());
            boolean listening = channel.options().alwaysListen() || joined.contains(id);
            if (!listening || !canListen(receiver, channel)) {
                continue;
            }
            receiver.sendSystemMessage(renderer.render(
                channel,
                ChannelRenderer.Audience.CHAT,
                null,
                receiver,
                message,
                consoleMessageContext(receiver, message)
            ).component());
        }
    }

    private Map<String, String> consoleMessageContext(ServerPlayer viewer, String message) {
        String consoleName = moderation.languages().text(viewer, "Console-Name");
        return Map.of(
            "message", message,
            "player_name", consoleName,
            "player_displayname", consoleName
        );
    }

    private void restoreChannelMembership(ServerPlayer player) {
        Set<String> joined = new HashSet<>(moderation.joinedChannels(player));
        joined.removeIf(id -> channels.byId(id)
            .map(channel -> !channel.isJoinable())
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
            .filter(ChannelDefinition::isJoinable)
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
        if (channel.options().privateChannel()) {
            chatLogs.logPrivate(
                player.getGameProfile().getName(),
                local.getOrDefault("trchat_toplayer", "?"),
                message
            );
        } else {
            chatLogs.logNormal(player.getGameProfile().getName(), message);
        }
        if (channel.consoleFormats().isEmpty()) {
            TrChatMod.LOGGER.info("{}", renderer.render(
                channel, ChannelRenderer.Audience.CHAT, player, (ServerPlayer) null, message, local
            ).component().getString());
            return;
        }
        Component console = renderer.render(
            channel, ChannelRenderer.Audience.CONSOLE, player, (ServerPlayer) null, message, local
        ).component();
        TrChatMod.LOGGER.info("{}", console.getString());
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
                default -> TrChatMod.LOGGER.debug("Ignoring unsupported Bukkit Redis action '{}'", data.getFirst());
            }
        } catch (RuntimeException exception) {
            TrChatMod.LOGGER.warn("Ignoring invalid TrChat Redis action '{}': {}",
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
        UUID senderUuid = TrChatProtocol.parseUuid(data.get(1));
        String senderName = data.size() > 7 ? data.get(7) : "";
        Set<String> mentioned = data.size() > 8 && !data.get(8).isBlank()
            ? new HashSet<>(List.of(data.get(8).split(",")))
            : Set.of();
        List<ServerPlayer> receivers = new ArrayList<>();
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            if (hasPermission(player, permission) && !moderation.hasIgnored(player, senderUuid)) {
                player.sendSystemMessage(component);
                receivers.add(player);
            }
        }
        if (!mentioned.isEmpty() && !senderName.isBlank()) {
            notifyMentioned(receivers, new ArrayList<>(mentioned), senderName);
        }
        TrChatMod.LOGGER.info(component.getString());
    }

    private void receivePrivate(List<String> data) {
        if (data.size() < 4) {
            return;
        }
        String targetName = data.get(1);
        ServerPlayer target = server.getPlayerList().getPlayerByName(targetName);
        String from = data.get(2);
        String fallback = data.size() > 4 ? data.get(4) : "";
        Component component = ComponentJson.deserialize(data.get(3), fallback, server);
        RemotePlayer remoteSender = findRemotePlayer(from);
        boolean ignored = target != null && remoteSender != null
            && moderation.hasIgnored(target, remoteSender.uuid());
        if (target != null && !ignored) {
            target.sendSystemMessage(component);
        }
        if (!from.isBlank()) {
            if (target != null && !ignored) {
                lastPrivateSender.put(target.getUUID(), from);
            }
            Component spyMessage = data.size() > 5 && !data.get(5).isBlank()
                ? ComponentJson.deserialize(data.get(5), component.getString(), server)
                : component;
            notifyPrivateSpies(null, from, targetName, spyMessage.getString(), target);
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
                || target != null && observer.getUUID().equals(target.getUUID())
                || observer.getGameProfile().getName().equalsIgnoreCase(senderName)
                || observer.getGameProfile().getName().equalsIgnoreCase(targetName)) {
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
            UUID uuid = index < uuids.length ? TrChatProtocol.parseUuid(uuids[index]) : null;
            if (uuid == null) {
                continue;
            }
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
            String key = data.get(2);
            String senderName = data.size() > 3 ? String.join(", ", data.subList(3, data.size())) : "";
            if ("Function-Mention-Notify".equals(key)) {
                functions.notifyMention(target, senderName);
            } else {
                target.sendSystemMessage(Component.literal(key + (senderName.isEmpty() ? "" : ": " + senderName)));
            }
        }
    }

    private void publishPlayerNames() {
        if (redis == null) {
            return;
        }
        List<ServerPlayer> players = server.getPlayerList().getPlayers();
        // Bukkit 2.4.x treats an empty names field as one blank entry and then
        // parses the UUID at index zero. A nil UUID safely clears the previous
        // server snapshot without producing FastUUID warnings or a real player.
        if (players.isEmpty()) {
            redis.publish(TrChatProtocol.emptyPlayerNames(
                Integer.toString(TrChatConfig.SERVER_ID.getAsInt())
            ));
            return;
        }
        String names = String.join(",", players.stream().map(player -> player.getGameProfile().getName()).toList());
        String displayNames = String.join(",", players.stream()
            .map(player -> {
                String display = player.getDisplayName().getString();
                return display.isBlank() ? "#" : display.replace(",", "");
            })
            .toList());
        String uuids = String.join(",", players.stream()
            .map(player -> TrChatProtocol.formatUuid(player.getUUID()))
            .toList());
        redis.publish(TrChatMessage.of(
            "UpdateNames",
            Integer.toString(TrChatConfig.SERVER_ID.getAsInt()),
            names,
            displayNames,
            uuids
        ));
    }

    private String exactRemoteName(String requested) {
        RemotePlayer player = findRemotePlayer(requested);
        return player == null ? null : player.name();
    }

    private RemotePlayer findRemotePlayer(String requested) {
        expireRemotePlayers();
        for (RemoteServerPlayers serverPlayers : remotePlayers.values()) {
            for (RemotePlayer player : serverPlayers.players()) {
                if (player.name().equalsIgnoreCase(requested)
                    || player.displayName().equalsIgnoreCase(requested)) {
                    return player;
                }
            }
        }
        return null;
    }

    private PlayerIdentity findKnownPlayer(String requested) {
        ServerPlayer local = server.getPlayerList().getPlayerByName(requested);
        if (local != null) {
            return new PlayerIdentity(local.getUUID(), local.getGameProfile().getName());
        }
        RemotePlayer remote = findRemotePlayer(requested);
        return remote == null ? null : new PlayerIdentity(remote.uuid(), remote.name());
    }

    private void expireRemotePlayers() {
        long expiryNanos = System.nanoTime() - REMOTE_PLAYER_TTL.toNanos();
        remotePlayers.entrySet().removeIf(entry -> entry.getValue().updatedAtNanos() < expiryNanos);
    }

    private void broadcast(Component component) {
        server.getPlayerList().broadcastSystemMessage(component, false);
        TrChatMod.LOGGER.info(component.getString());
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

    private record ChatState(long sentAt, String message) {
    }

    private record RemotePlayer(String name, String displayName, UUID uuid) {
    }

    private record PlayerIdentity(UUID uuid, String name) {
    }

    private record RemoteServerPlayers(long updatedAtNanos, List<RemotePlayer> players) {
    }

    public record ReloadResult(boolean success, int channelCount, List<String> failedSections) {
    }
}
