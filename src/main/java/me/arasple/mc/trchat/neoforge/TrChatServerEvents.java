package me.arasple.mc.trchat.neoforge;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import me.arasple.mc.trchat.neoforge.channel.ChannelDefinition;
import me.arasple.mc.trchat.neoforge.channel.ChannelManager;
import me.arasple.mc.trchat.neoforge.chat.ChatService;
import me.arasple.mc.trchat.neoforge.chat.CommandInvocation;
import me.arasple.mc.trchat.neoforge.chat.LegacyText;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import me.arasple.mc.trchat.neoforge.moderation.ModerationService;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;
import me.arasple.mc.trchat.neoforge.update.UpdateChecker;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.commands.SharedSuggestionProvider;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.HoverEvent;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.neoforged.bus.api.SubscribeEvent;
import net.neoforged.fml.ModList;
import net.neoforged.neoforge.event.RegisterCommandsEvent;
import net.neoforged.neoforge.event.ServerChatEvent;
import net.neoforged.neoforge.event.CommandEvent;
import net.neoforged.neoforge.event.AnvilUpdateEvent;
import net.neoforged.neoforge.event.entity.living.LivingDamageEvent;
import net.neoforged.neoforge.event.entity.player.PlayerEvent;
import net.neoforged.neoforge.event.server.ServerStartedEvent;
import net.neoforged.neoforge.event.server.ServerStoppingEvent;
import net.neoforged.neoforge.event.tick.ServerTickEvent;
import net.neoforged.neoforge.event.level.ChunkEvent;
import net.minecraft.world.level.chunk.LevelChunk;
import net.neoforged.neoforge.server.permission.events.PermissionGatherEvent;

import java.util.HashSet;
import java.util.Locale;
import java.util.Set;
import java.util.OptionalLong;

public final class TrChatServerEvents {

    private static final String REPOSITORY_URL = "https://github.com/Aruvelut-123/TrChat-Neoforge";
    private static final String BILIBILI_PROFILE_URL = "https://space.bilibili.com/475655508";

    private final ChannelManager channels = new ChannelManager();
    private ChatService service;
    private UpdateChecker updateChecker;
    private CommandDispatcher<CommandSourceStack> commandDispatcher;

    @SubscribeEvent
    public void onPermissionNodes(PermissionGatherEvent.Nodes event) {
        TrChatPermissions.register(event);
    }

    @SubscribeEvent
    public void onServerStarted(ServerStartedEvent event) {
        service = new ChatService(event.getServer(), channels);
        if (TrChatConfig.UPDATE_CHECK_ENABLED.get()) {
            updateChecker = new UpdateChecker(event.getServer(), service.languages(), modVersion());
            updateChecker.start();
        }
        TrChatNeoForge.LOGGER.info("{} started with {} channels (NeoForge 21.1.233+, Redis-only transport)",
            TrChatNeoForge.MOD_NAME, service.channelCount());
    }

    @SubscribeEvent
    public void onServerStopping(ServerStoppingEvent event) {
        if (updateChecker != null) {
            updateChecker.close();
            updateChecker = null;
        }
        if (service != null) {
            service.close();
            service = null;
        }
    }

    @SubscribeEvent
    public void onChat(ServerChatEvent event) {
        if (service == null) {
            return;
        }
        event.setCanceled(true);
        service.handleChat(event.getPlayer(), event.getRawText());
    }

    @SubscribeEvent
    public void onPlayerLogin(PlayerEvent.PlayerLoggedInEvent event) {
        if (service != null && event.getEntity() instanceof ServerPlayer player) {
            service.playerJoined(player);
            if (updateChecker != null) {
                updateChecker.notifyPlayer(player);
            }
        }
    }

    @SubscribeEvent
    public void onPlayerLogout(PlayerEvent.PlayerLoggedOutEvent event) {
        if (service != null && event.getEntity() instanceof ServerPlayer player) {
            service.playerLeft(player);
        }
    }

    @SubscribeEvent
    public void onDamage(LivingDamageEvent.Post event) {
        if (service != null && event.getEntity() instanceof ServerPlayer player) {
            service.recordDamage(player, event.getNewDamage());
        }
    }

    @SubscribeEvent
    public void onCommand(CommandEvent event) {
        if (service == null) {
            return;
        }
        CommandSourceStack source = event.getParseResults().getContext().getSource();
        if (source.getEntity() instanceof ServerPlayer player
            && !service.checkCommand(player, event.getParseResults().getReader().getString())) {
            event.setCanceled(true);
            return;
        }
        if (routePrivateAlias(source, event.getParseResults().getReader().getString())) {
            event.setCanceled(true);
        }
    }

    @SubscribeEvent
    public void onAnvil(AnvilUpdateEvent event) {
        if (service != null
            && event.getPlayer() instanceof ServerPlayer player
            && !service.checkAnvil(player, event.getName())) {
            event.setCanceled(true);
        }
    }

    @SubscribeEvent
    public void onChunkLoad(ChunkEvent.Load event) {
        if (service != null && event.getChunk() instanceof LevelChunk chunk) {
            service.chunkLoaded(chunk);
        }
    }

    @SubscribeEvent
    public void onChunkUnload(ChunkEvent.Unload event) {
        if (service != null && event.getChunk() instanceof LevelChunk chunk) {
            service.chunkUnloaded(chunk);
        }
    }

    @SubscribeEvent
    public void onServerTick(ServerTickEvent.Post event) {
        if (service != null) {
            service.tick();
        }
    }

    @SubscribeEvent
    public void onRegisterCommands(RegisterCommandsEvent event) {
        channels.reload();
        commandDispatcher = event.getDispatcher();
        registerCommands(commandDispatcher);
    }

    private void registerCommands(CommandDispatcher<CommandSourceStack> dispatcher) {
        dispatcher.register(Commands.literal("trchat")
            .then(Commands.literal("status")
                .executes(context -> status(context.getSource()))
                .then(Commands.argument("player", StringArgumentType.word())
                    .requires(source -> canUsePermission(source, "trchat.admin"))
                    .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                        context.getSource().getOnlinePlayerNames(), builder
                    ))
                    .executes(context -> playerStatus(
                        context.getSource(),
                        StringArgumentType.getString(context, "player")
                    ))))
            .then(Commands.literal("reload")
                .requires(source -> source.hasPermission(2))
                .executes(context -> reload(context.getSource())))
            .then(Commands.literal("redis")
                .requires(source -> source.hasPermission(2))
                .then(Commands.literal("reconnect")
                    .executes(context -> reconnect(context.getSource()))))
            .then(Commands.literal("mute")
                .requires(source -> canUsePermission(source, "trchat.mute"))
                .then(Commands.literal("on")
                    .executes(context -> mute(context.getSource(), true)))
                .then(Commands.literal("off")
                    .executes(context -> mute(context.getSource(), false)))
                .then(Commands.literal("player")
                    .then(Commands.argument("player", StringArgumentType.word())
                        .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                            context.getSource().getOnlinePlayerNames(), builder
                        ))
                        .then(Commands.argument("duration", StringArgumentType.word())
                            .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                                new String[]{"30s", "5m", "1h", "1d", "7d", "permanent"}, builder
                            ))
                            .executes(context -> mutePlayer(
                                context.getSource(),
                                StringArgumentType.getString(context, "player"),
                                StringArgumentType.getString(context, "duration"),
                                ""
                            ))
                            .then(Commands.argument("reason", StringArgumentType.greedyString())
                                .executes(context -> mutePlayer(
                                    context.getSource(),
                                    StringArgumentType.getString(context, "player"),
                                    StringArgumentType.getString(context, "duration"),
                                    StringArgumentType.getString(context, "reason")
                                ))))))
                .executes(context -> mute(context.getSource(), service != null && !service.isGlobalMute())))
            .then(Commands.literal("unmute")
                .requires(source -> canUsePermission(source, "trchat.mute"))
                .then(Commands.argument("player", StringArgumentType.word())
                    .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                        context.getSource().getOnlinePlayerNames(), builder
                    ))
                    .executes(context -> unmutePlayer(
                        context.getSource(), StringArgumentType.getString(context, "player")
                    ))))
            .then(Commands.literal("shadowmute")
                .requires(source -> canUsePermission(source, "trchat.shadowmute"))
                .then(Commands.argument("player", StringArgumentType.word())
                    .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                        context.getSource().getOnlinePlayerNames(), builder
                    ))
                    .executes(context -> shadowMute(
                        context.getSource(), StringArgumentType.getString(context, "player"), null
                    ))
                    .then(Commands.literal("on")
                        .executes(context -> shadowMute(
                            context.getSource(), StringArgumentType.getString(context, "player"), true
                        )))
                    .then(Commands.literal("off")
                        .executes(context -> shadowMute(
                            context.getSource(), StringArgumentType.getString(context, "player"), false
                        )))))
            .then(Commands.literal("spy")
                .executes(context -> privateSpy(context.getSource(), null))
                .then(Commands.literal("on")
                    .executes(context -> privateSpy(context.getSource(), true)))
                .then(Commands.literal("off")
                    .executes(context -> privateSpy(context.getSource(), false))))
            .then(Commands.literal("msg")
                .then(Commands.argument("player", StringArgumentType.word())
                    .then(Commands.argument("message", StringArgumentType.greedyString())
                        .executes(context -> privateMessage(
                            context.getSource(),
                            StringArgumentType.getString(context, "player"),
                            StringArgumentType.getString(context, "message")
                        )))))
            .then(Commands.literal("channel")
                .then(Commands.literal("join")
                    .then(Commands.argument("channel", StringArgumentType.word())
                        .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                            channels.all().stream()
                                .filter(ChannelDefinition::isJoinable)
                                .map(ChannelDefinition::id),
                            builder
                        ))
                        .executes(context -> selectChannel(
                            context.getSource(),
                            StringArgumentType.getString(context, "channel")
                        ))
                        .then(Commands.argument("player", StringArgumentType.word())
                            .requires(source -> canUsePermission(source, "trchat.command.channel.other"))
                            .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                                context.getSource().getOnlinePlayerNames(), builder
                            ))
                            .executes(context -> setPlayerChannel(
                                context.getSource(),
                                StringArgumentType.getString(context, "channel"),
                                StringArgumentType.getString(context, "player")
                            )))))
                .then(Commands.literal("quit")
                    .executes(context -> quitChannel(context.getSource()))
                    .then(Commands.argument("player", StringArgumentType.word())
                        .requires(source -> canUsePermission(source, "trchat.command.channel.other"))
                        .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                            context.getSource().getOnlinePlayerNames(), builder
                        ))
                        .executes(context -> quitPlayerChannel(
                            context.getSource(), StringArgumentType.getString(context, "player")
                        )))))
            .then(Commands.literal("color")
                .requires(source -> canUsePermission(source, "trchat.command.color"))
                .then(Commands.argument("color", StringArgumentType.word())
                    .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                        chatColorSuggestions(context.getSource()),
                        builder
                    ))
                    .executes(context -> selectChatColor(
                        context.getSource(), StringArgumentType.getString(context, "color")
                    ))))
            .then(Commands.literal("clear")
                .requires(source -> canUsePermission(source, "trchat.command.clear"))
                .then(Commands.argument("player", StringArgumentType.word())
                    .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                        java.util.stream.Stream.concat(
                            context.getSource().getOnlinePlayerNames().stream(),
                            java.util.stream.Stream.of("*")
                        ),
                        builder
                    ))
                    .executes(context -> clearChat(
                        context.getSource(), StringArgumentType.getString(context, "player")
                    ))))
            .then(Commands.literal("view")
                .then(Commands.argument("snapshot", StringArgumentType.word())
                    .executes(context -> openSnapshot(
                        context.getSource(),
                        StringArgumentType.getString(context, "snapshot")
                    )))));

        dispatcher.register(Commands.literal("trmsg")
            .then(Commands.argument("player", StringArgumentType.word())
                .then(Commands.argument("message", StringArgumentType.greedyString())
                    .executes(context -> privateMessage(
                        context.getSource(),
                        StringArgumentType.getString(context, "player"),
                        StringArgumentType.getString(context, "message")
                    )))));

        dispatcher.register(Commands.literal("trreply")
            .then(Commands.argument("message", StringArgumentType.greedyString())
                .executes(context -> reply(context.getSource(), StringArgumentType.getString(context, "message")))));
        registerReplyAlias(dispatcher, "r");
        registerReplyAlias(dispatcher, "reply");
        registerModerationAliases(dispatcher);
        registerIgnoreCommands(dispatcher);
        registerControllerCommands(dispatcher);

        registerDynamicCommands(dispatcher);
    }

    private void registerDynamicCommands(CommandDispatcher<CommandSourceStack> dispatcher) {
        if (dispatcher == null) {
            return;
        }
        Set<String> registered = new HashSet<>();
        for (ChannelDefinition channel : channels.all()) {
            for (String alias : channel.bindings().commands()) {
                String key = alias.toLowerCase(Locale.ROOT);
                if (!registered.add(key)) {
                    continue;
                }
                registerDynamicAlias(dispatcher, alias);
            }
        }

        dispatcher.register(Commands.literal("say")
            .requires(source -> source.hasPermission(2))
            .then(Commands.argument("message", StringArgumentType.greedyString())
                .executes(context -> consoleSay(
                    context.getSource(),
                    StringArgumentType.getString(context, "message")
                ))));
    }

    private int status(CommandSourceStack source) {
        if (service == null) {
            source.sendFailure(unavailable());
            return 0;
        }
        ServerPlayer viewer = source.getPlayer();
        String redis = service.languages().text(
            viewer,
            !service.isRedisEnabled()
                ? "Status-State-Disabled"
                : service.isRedisConnected() ? "Status-State-Connected" : "Status-State-Reconnecting"
        );
        String globalMute = service.languages().text(
            viewer, service.isGlobalMute() ? "Status-State-Enabled" : "Status-State-Disabled"
        );
        String controller = service.languages().text(
            viewer,
            service.isCommandControllerEnabled() ? "Status-State-Enabled" : "Status-State-Disabled"
        );
        MutableComponent overview = service.languages().component(
            viewer,
            "Status-Overview",
            modVersion(),
            service.channelCount(),
            service.autoJoinChannel(),
            redis,
            globalMute,
            controller,
            service.commandRuleCount(),
            source.getServer().getPlayerCount(),
            source.getServer().getMaxPlayers()
        ).copy();
        overview
            .append("\n")
            .append(service.languages().component(viewer, "Status-Creator-Prefix"))
            .append(statusLink(viewer, "Status-Creator-Link", BILIBILI_PROFILE_URL, "Status-Creator-Link-Hover"))
            .append("\n")
            .append(service.languages().component(viewer, "Status-Original-Author"))
            .append("\n")
            .append(service.languages().component(viewer, "Status-Repository-Prefix"))
            .append(statusLink(viewer, "Status-Repository-Link", REPOSITORY_URL, "Status-Repository-Link-Hover"))
            .append("\n")
            .append(service.languages().component(viewer, "Status-Footer"));
        source.sendSuccess(() -> overview, false);
        return 1;
    }

    private int playerStatus(CommandSourceStack source, String playerName) {
        if (service == null) {
            source.sendFailure(unavailable());
            return 0;
        }
        ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(playerName);
        ServerPlayer viewer = source.getPlayer();
        if (target == null) {
            source.sendFailure(service.languages().component(viewer, "General-Player-Not-Found", playerName));
            return 0;
        }
        boolean muted = service.isMuted(target);
        MutableComponent output = service.languages().component(
            viewer,
            "Player-Status-Overview",
            target.getGameProfile().getName(),
            service.activeChannel(target),
            service.joinedChannelCount(target),
            target.connection.latency(),
            state(viewer, muted),
            state(viewer, service.isShadowMuted(target)),
            state(viewer, service.isPrivateSpy(target)),
            state(viewer, target.hasPermissions(2)),
            target.gameMode.getGameModeForPlayer().getName().toUpperCase(Locale.ROOT)
        ).copy();
        if (muted) {
            String expiry = service.muteExpiry(target);
            if (expiry.equals("permanent")) {
                expiry = service.languages().text(viewer, "Player-Status-Permanent");
            }
            output
                .append("\n")
                .append(service.languages().component(
                    viewer, "Player-Status-Mute-Detail", expiry, service.muteReason(target)
                ));
        }
        output
            .append("\n")
            .append(service.languages().component(viewer, "Status-Footer"));
        source.sendSuccess(() -> output, false);
        return 1;
    }

    private String state(ServerPlayer viewer, boolean enabled) {
        return service.languages().text(
            viewer, enabled ? "Status-State-Enabled" : "Status-State-Disabled"
        );
    }

    private MutableComponent statusLink(ServerPlayer viewer, String key, String url, String hoverKey) {
        return service.languages().component(viewer, key).copy()
            .withStyle(style -> style
                .withClickEvent(new ClickEvent(ClickEvent.Action.OPEN_URL, url))
                .withHoverEvent(new HoverEvent(
                    HoverEvent.Action.SHOW_TEXT,
                    service.languages().component(viewer, hoverKey)
                )));
    }

    private int controlledCommand(
        CommandSourceStack source,
        String label,
        String arguments,
        ControlledCommand command
    ) {
        if (service == null) {
            return 0;
        }
        String commandLine = arguments.isBlank() ? label : label + ' ' + arguments;
        if (!service.isCommandManaged(commandLine)) {
            source.sendFailure(service.languages().component(
                source.getPlayer(), "Command-Controller-Disabled", label
            ));
            return 0;
        }
        return switch (command) {
            case ABOUT -> {
                source.sendSuccess(() -> service.languages().component(
                    source.getPlayer(), "Command-About", modVersion()
                ), false);
                yield 1;
            }
            case STATUS -> status(source);
            case HELP -> {
                source.sendSuccess(() -> service.languages().component(
                    source.getPlayer(), "Command-Help"
                ), false);
                yield 1;
            }
        };
    }

    private int reload(CommandSourceStack source) {
        if (service == null) {
            return 0;
        }
        ChatService.ReloadResult result = service.reloadConfiguration();
        if (result.channelCount() < 0) {
            source.sendFailure(service.languages().component(
                source.getPlayer(), "Reload-Failed", String.join(", ", result.failedSections())
            ));
            return 0;
        }
        registerDynamicCommands(commandDispatcher);
        for (ServerPlayer player : source.getServer().getPlayerList().getPlayers()) {
            source.getServer().getCommands().sendCommands(player);
        }
        if (!result.success()) {
            source.sendFailure(service.languages().component(
                source.getPlayer(),
                "Reload-Partial",
                result.channelCount(),
                String.join(", ", result.failedSections())
            ));
            return 0;
        }
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Reload-Success", result.channelCount()
        ), true);
        return result.channelCount();
    }

    private int reconnect(CommandSourceStack source) {
        if (service == null) {
            return 0;
        }
        service.reconnectRedis();
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Redis-Reconnect-Started"
        ), true);
        return 1;
    }

    private int mute(CommandSourceStack source, boolean muted) {
        if (service == null) {
            return 0;
        }
        service.setGlobalMute(muted, true);
        return 1;
    }

    private int mutePlayer(CommandSourceStack source, String playerName, String duration, String reason) {
        if (service == null) return 0;
        ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(playerName);
        if (target == null) {
            source.sendFailure(service.languages().component(source.getPlayer(), "General-Player-Not-Found", playerName));
            return 0;
        }
        OptionalLong parsed = ModerationService.parseDuration(duration);
        if (parsed.isEmpty()) {
            source.sendFailure(service.languages().component(source.getPlayer(), "Mute-Wrong-Format", duration));
            return 0;
        }
        String actualReason = reason == null || reason.isBlank() ? "-" : reason.trim();
        service.mutePlayer(target, parsed.getAsLong(), actualReason);
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Mute-Muted-Player", target.getGameProfile().getName(),
            ModerationService.describeDuration(parsed.getAsLong()), actualReason
        ), true);
        return 1;
    }

    private int unmutePlayer(CommandSourceStack source, String playerName) {
        if (service == null) return 0;
        ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(playerName);
        if (target == null) {
            source.sendFailure(service.languages().component(source.getPlayer(), "General-Player-Not-Found", playerName));
            return 0;
        }
        service.unmutePlayer(target);
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Mute-Cancel-Muted-Player", target.getGameProfile().getName()
        ), true);
        return 1;
    }

    private int shadowMute(CommandSourceStack source, String playerName, Boolean requested) {
        if (service == null) return 0;
        ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(playerName);
        if (target == null) {
            source.sendFailure(service.languages().component(source.getPlayer(), "General-Player-Not-Found", playerName));
            return 0;
        }
        boolean enabled = requested == null ? !service.isShadowMuted(target) : requested;
        service.setShadowMuted(target, enabled);
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), enabled ? "Mute-Shadow-On" : "Mute-Shadow-Off",
            target.getGameProfile().getName()
        ), true);
        return 1;
    }

    private int privateSpy(CommandSourceStack source, Boolean requested) {
        if (service == null) return 0;
        try {
            ServerPlayer player = source.getPlayerOrException();
            if (!player.hasPermissions(2) && !TrChatPermissions.check(player, "trchat.spy")) {
                source.sendFailure(service.languages().component(player, "General-No-Permission"));
                return 0;
            }
            boolean enabled = service.setPrivateSpy(player, requested);
            player.displayClientMessage(service.languages().component(
                player, enabled ? "Private-Message-Spy-On" : "Private-Message-Spy-Off"
            ), true);
            player.playNotifySound(
                SoundEvents.ANVIL_LAND,
                SoundSource.PLAYERS,
                1.0F,
                enabled ? 2.0F : 0.0F
            );
            return 1;
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private int privateMessage(CommandSourceStack source, String target, String message) {
        if (service == null) {
            return 0;
        }
        try {
            return service.sendPrivate(source.getPlayerOrException(), target, message);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private int reply(CommandSourceStack source, String message) {
        if (service == null) {
            return 0;
        }
        try {
            return service.reply(source.getPlayerOrException(), message);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private int selectChannel(CommandSourceStack source, String channelId) {
        if (service == null) {
            return 0;
        }
        try {
            ChannelDefinition channel = channels.byId(channelId).orElse(null);
            if (channel == null) {
                source.sendFailure(service.languages().component(
                    source.getPlayer(), "Channel-Unknown", channelId
                ));
                return 0;
            }
            return service.executeChannel(source.getPlayerOrException(), channel, "");
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private int setPlayerChannel(CommandSourceStack source, String channelId, String playerName) {
        if (service == null) {
            return 0;
        }
        ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(playerName);
        if (target == null) {
            source.sendFailure(service.languages().component(
                source.getPlayer(), "General-Player-Not-Found", playerName
            ));
            return 0;
        }
        ChannelDefinition channel = channels.byId(channelId).filter(ChannelDefinition::isJoinable).orElse(null);
        if (channel == null) {
            source.sendFailure(service.languages().component(
                source.getPlayer(), "Channel-Not-Found", channelId
            ));
            return 0;
        }
        if (service.setChannel(target, channel) == 0) {
            source.sendFailure(service.languages().component(source.getPlayer(), "General-No-Permission"));
            return 0;
        }
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Channel-Join-Other", target.getGameProfile().getName(), channel.id()
        ), true);
        return 1;
    }

    private int quitChannel(CommandSourceStack source) {
        if (service == null) {
            return 0;
        }
        try {
            return service.quitChannel(source.getPlayerOrException());
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private int quitPlayerChannel(CommandSourceStack source, String playerName) {
        if (service == null) {
            return 0;
        }
        ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(playerName);
        if (target == null) {
            source.sendFailure(service.languages().component(
                source.getPlayer(), "General-Player-Not-Found", playerName
            ));
            return 0;
        }
        service.quitChannel(target);
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Channel-Quit-Other", target.getGameProfile().getName()
        ), true);
        return 1;
    }

    private int selectChatColor(CommandSourceStack source, String color) {
        if (service == null) {
            return 0;
        }
        try {
            return service.setChatColor(source.getPlayerOrException(), color);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private java.util.List<String> chatColorSuggestions(CommandSourceStack source) {
        if (service == null || source.getPlayer() == null) {
            return java.util.List.of("reset");
        }
        java.util.List<String> colors = new java.util.ArrayList<>(service.availableChatColors(source.getPlayer()));
        colors.add("reset");
        return colors;
    }

    private int clearChat(CommandSourceStack source, String targetName) {
        if (service == null) {
            return 0;
        }
        java.util.List<ServerPlayer> targets;
        if ("*".equals(targetName)) {
            targets = source.getServer().getPlayerList().getPlayers();
        } else {
            ServerPlayer target = source.getServer().getPlayerList().getPlayerByName(targetName);
            if (target == null) {
                source.sendFailure(service.languages().component(
                    source.getPlayer(), "General-Player-Not-Found", targetName
                ));
                return 0;
            }
            targets = java.util.List.of(target);
        }
        for (ServerPlayer target : targets) {
            for (int line = 0; line < 80; line++) {
                target.sendSystemMessage(Component.empty());
            }
        }
        source.sendSuccess(() -> service.languages().component(
            source.getPlayer(), "Clear-Success", "*".equals(targetName) ? "*" : targets.getFirst().getGameProfile().getName()
        ), true);
        return targets.size();
    }

    private int consoleSay(CommandSourceStack source, String message) {
        if (service == null) {
            return 0;
        }
        int result = service.sendConsole(message);
        if (result == 0 && channels.autoJoin().isEmpty()) {
            source.sendFailure(service.languages().component(source.getPlayer(), "Channel-Auto-Join-Missing"));
        }
        return result;
    }

    private int openSnapshot(CommandSourceStack source, String snapshot) {
        if (service == null) {
            return 0;
        }
        try {
            return service.openFunctionSnapshot(source.getPlayerOrException(), snapshot) ? 1 : 0;
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private void registerDynamicAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .executes(context -> executeBoundAlias(context.getSource(), alias, ""))
            .then(Commands.argument("arguments", StringArgumentType.greedyString())
                .executes(context -> executeBoundAlias(
                    context.getSource(),
                    alias,
                    StringArgumentType.getString(context, "arguments")
                ))));
    }

    private void registerReplyAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .then(Commands.argument("message", StringArgumentType.greedyString())
                .executes(context -> reply(context.getSource(), StringArgumentType.getString(context, "message")))));
    }

    private void registerControllerCommands(CommandDispatcher<CommandSourceStack> dispatcher) {
        registerControllerCommand(dispatcher, "arasple", ControlledCommand.ABOUT);
        for (String alias : Set.of("ver", "vers", "version", "versions")) {
            registerControllerCommand(dispatcher, alias, ControlledCommand.STATUS);
        }
        for (String alias : Set.of("help", "helps")) {
            registerControllerCommand(dispatcher, alias, ControlledCommand.HELP);
        }
    }

    private void registerControllerCommand(
        CommandDispatcher<CommandSourceStack> dispatcher,
        String label,
        ControlledCommand command
    ) {
        dispatcher.register(Commands.literal(label)
            .executes(context -> controlledCommand(context.getSource(), label, "", command))
            .then(Commands.argument("arguments", StringArgumentType.greedyString())
                .executes(context -> controlledCommand(
                    context.getSource(),
                    label,
                    StringArgumentType.getString(context, "arguments"),
                    command
                ))));
    }

    private void registerModerationAliases(CommandDispatcher<CommandSourceStack> dispatcher) {
        registerMuteAlias(dispatcher, "trmute");
        registerMuteAlias(dispatcher, "mute");
        dispatcher.register(Commands.literal("trunmute")
            .requires(source -> canUsePermission(source, "trchat.mute"))
            .then(Commands.argument("player", StringArgumentType.word())
                .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                    context.getSource().getOnlinePlayerNames(), builder
                ))
                .executes(context -> unmutePlayer(
                    context.getSource(), StringArgumentType.getString(context, "player")
                ))));
        registerShadowMuteAlias(dispatcher, "trshadowmute");
        registerShadowMuteAlias(dispatcher, "shadowmute");
        dispatcher.register(Commands.literal("trspy")
            .executes(context -> privateSpy(context.getSource(), null)));
    }

    private void registerIgnoreCommands(CommandDispatcher<CommandSourceStack> dispatcher) {
        registerIgnoreCommand(dispatcher, "ignore");
        registerIgnoreCommand(dispatcher, "trignore");
        dispatcher.register(Commands.literal("ignorelist")
            .requires(source -> canUsePermission(source, "trchat.command.ignore"))
            .executes(context -> ignoredPlayers(context.getSource())));
    }

    private void registerIgnoreCommand(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .requires(source -> canUsePermission(source, "trchat.command.ignore"))
            .then(Commands.argument("player", StringArgumentType.word())
                .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                    service == null ? context.getSource().getOnlinePlayerNames() : service.knownPlayerNames(),
                    builder
                ))
                .executes(context -> ignorePlayer(
                    context.getSource(), StringArgumentType.getString(context, "player"), null
                ))
                .then(Commands.literal("on")
                    .executes(context -> ignorePlayer(
                        context.getSource(), StringArgumentType.getString(context, "player"), true
                    )))
                .then(Commands.literal("off")
                    .executes(context -> ignorePlayer(
                        context.getSource(), StringArgumentType.getString(context, "player"), false
                    )))));
    }

    private int ignorePlayer(CommandSourceStack source, String playerName, Boolean ignored) {
        if (service == null) {
            return 0;
        }
        try {
            return service.setIgnored(source.getPlayerOrException(), playerName, ignored);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private int ignoredPlayers(CommandSourceStack source) {
        if (service == null) {
            return 0;
        }
        try {
            ServerPlayer player = source.getPlayerOrException();
            String names = String.join(", ", service.ignoredPlayerNames(player));
            player.sendSystemMessage(service.languages().component(
                player, "Ignore-List", names.isBlank() ? "-" : names
            ));
            return 1;
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private void registerMuteAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .requires(source -> canUsePermission(source, "trchat.mute"))
            .then(Commands.argument("player", StringArgumentType.word())
                .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                    context.getSource().getOnlinePlayerNames(), builder
                ))
                .then(Commands.argument("duration", StringArgumentType.word())
                    .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                        new String[]{"30s", "5m", "1h", "1d", "7d", "permanent"}, builder
                    ))
                    .executes(context -> mutePlayer(
                        context.getSource(),
                        StringArgumentType.getString(context, "player"),
                        StringArgumentType.getString(context, "duration"),
                        ""
                    ))
                    .then(Commands.argument("reason", StringArgumentType.greedyString())
                        .executes(context -> mutePlayer(
                            context.getSource(),
                            StringArgumentType.getString(context, "player"),
                            StringArgumentType.getString(context, "duration"),
                            StringArgumentType.getString(context, "reason")
                        ))))));
    }

    private void registerShadowMuteAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .requires(source -> canUsePermission(source, "trchat.shadowmute"))
            .then(Commands.argument("player", StringArgumentType.word())
                .suggests((context, builder) -> SharedSuggestionProvider.suggest(
                    context.getSource().getOnlinePlayerNames(), builder
                ))
                .executes(context -> shadowMute(
                    context.getSource(), StringArgumentType.getString(context, "player"), null
                ))
                .then(Commands.literal("on")
                    .executes(context -> shadowMute(
                        context.getSource(), StringArgumentType.getString(context, "player"), true
                    )))
                .then(Commands.literal("off")
                    .executes(context -> shadowMute(
                        context.getSource(), StringArgumentType.getString(context, "player"), false
                    )))));
    }

    private int executeBoundAlias(CommandSourceStack source, String alias, String arguments) {
        ChannelDefinition channel = channels.byCommand(alias).orElse(null);
        if (channel == null) {
            if (service != null) {
                source.sendFailure(service.languages().component(
                    source.getPlayer(), "Channel-Command-Unbound", alias
                ));
            }
            return 0;
        }
        if (!channel.options().privateChannel()) {
            return executeBoundChannel(source, channel.id(), arguments);
        }
        String[] privateArguments = arguments.trim().split("\\s+", 2);
        if (privateArguments.length < 2 || privateArguments[1].isBlank()) {
            return executeBoundChannel(source, channel.id(), "");
        }
        return privateMessage(source, privateArguments[0], privateArguments[1]);
    }

    private boolean routePrivateAlias(CommandSourceStack source, String commandLine) {
        CommandInvocation invocation = CommandInvocation.parse(commandLine);
        ChannelDefinition channel = channels.byCommand(invocation.alias()).orElse(null);
        if (channel == null || !channel.options().privateChannel()) {
            return false;
        }
        executeBoundAlias(source, invocation.alias(), invocation.arguments());
        return true;
    }

    private int executeBoundChannel(CommandSourceStack source, String channelId, String message) {
        if (service == null) {
            return 0;
        }
        try {
            ChannelDefinition channel = channels.byId(channelId).orElse(null);
            return channel == null ? 0 : service.executeChannel(source.getPlayerOrException(), channel, message);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(service.languages().component(null, "General-Player-Only"));
            return 0;
        }
    }

    private static boolean canUsePermission(CommandSourceStack source, String permission) {
        ServerPlayer player = source.getPlayer();
        return source.hasPermission(2) || (player != null && TrChatPermissions.check(player, permission));
    }

    private static Component unavailable() {
        return LegacyText.parse("&8[&3Tr&bChat&8] &cTrChat NeoForge is not running.");
    }

    private static String modVersion() {
        return ModList.get()
            .getModContainerById(TrChatNeoForge.MOD_ID)
            .map(container -> container.getModInfo().getVersion().toString())
            .orElse("development");
    }

    private enum ControlledCommand {
        ABOUT,
        STATUS,
        HELP
    }
}
