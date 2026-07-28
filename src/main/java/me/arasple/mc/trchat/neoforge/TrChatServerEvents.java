package me.arasple.mc.trchat.neoforge;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import me.arasple.mc.trchat.neoforge.channel.ChannelDefinition;
import me.arasple.mc.trchat.neoforge.channel.ChannelManager;
import me.arasple.mc.trchat.neoforge.chat.ChatService;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.commands.SharedSuggestionProvider;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerPlayer;
import net.neoforged.bus.api.SubscribeEvent;
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

public final class TrChatServerEvents {

    private final ChannelManager channels = new ChannelManager();
    private ChatService service;

    @SubscribeEvent
    public void onPermissionNodes(PermissionGatherEvent.Nodes event) {
        TrChatPermissions.register(event);
    }

    @SubscribeEvent
    public void onServerStarted(ServerStartedEvent event) {
        service = new ChatService(event.getServer(), channels);
        TrChatNeoForge.LOGGER.info("{} started with {} channels (NeoForge 21.1.233+, Redis-only transport)",
            TrChatNeoForge.MOD_NAME, service.channelCount());
    }

    @SubscribeEvent
    public void onServerStopping(ServerStoppingEvent event) {
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
        registerCommands(event.getDispatcher());
    }

    private void registerCommands(CommandDispatcher<CommandSourceStack> dispatcher) {
        dispatcher.register(Commands.literal("trchat")
            .then(Commands.literal("status")
                .executes(context -> status(context.getSource())))
            .then(Commands.literal("reload")
                .requires(source -> source.hasPermission(2))
                .executes(context -> reload(context.getSource())))
            .then(Commands.literal("redis")
                .requires(source -> source.hasPermission(2))
                .then(Commands.literal("reconnect")
                    .executes(context -> reconnect(context.getSource()))))
            .then(Commands.literal("mute")
                .requires(source -> source.hasPermission(2))
                .then(Commands.literal("on")
                    .executes(context -> mute(context.getSource(), true)))
                .then(Commands.literal("off")
                    .executes(context -> mute(context.getSource(), false)))
                .executes(context -> mute(context.getSource(), service != null && !service.isGlobalMute())))
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
                                .filter(channel -> !channel.options().privateChannel() && !channel.id().equalsIgnoreCase("Server"))
                                .map(ChannelDefinition::id),
                            builder
                        ))
                        .executes(context -> selectChannel(
                            context.getSource(),
                            StringArgumentType.getString(context, "channel")
                        )))))
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

        Set<String> registered = new HashSet<>();
        for (ChannelDefinition channel : channels.all()) {
            if (channel.id().equalsIgnoreCase("Server")) {
                continue;
            }
            for (String alias : channel.bindings().commands()) {
                String key = alias.toLowerCase(Locale.ROOT);
                if (!registered.add(key)) {
                    continue;
                }
                if (channel.options().privateChannel()) {
                    registerPrivateAlias(dispatcher, alias);
                } else {
                    registerChannelAlias(dispatcher, alias, channel.id());
                }
            }
        }

        if (channels.byId("Server").isPresent()) {
            dispatcher.register(Commands.literal("say")
                .requires(source -> source.hasPermission(2))
                .then(Commands.argument("message", StringArgumentType.greedyString())
                    .executes(context -> serverSay(
                        context.getSource(),
                        StringArgumentType.getString(context, "message")
                    ))));
        }
    }

    private int status(CommandSourceStack source) {
        if (service == null) {
            source.sendFailure(Component.literal("TrChat NeoForge is not running."));
            return 0;
        }
        String redis = !service.isRedisEnabled()
            ? "disabled"
            : service.isRedisConnected() ? "connected" : "reconnecting";
        source.sendSuccess(() -> Component.literal(
            "TrChat NeoForge: " + service.channelCount() + " channels, Redis " + redis
                + ", global mute " + (service.isGlobalMute() ? "on" : "off")
        ), false);
        return 1;
    }

    private int reload(CommandSourceStack source) {
        if (service == null) {
            return 0;
        }
        int loaded = service.reloadChannels();
        if (loaded < 0) {
            source.sendFailure(Component.literal("Channel reload failed; see the server log."));
            return 0;
        }
        source.sendSuccess(() -> Component.literal("Reloaded " + loaded + " channels. Run /reload after changing command bindings."), true);
        return loaded;
    }

    private int reconnect(CommandSourceStack source) {
        if (service == null) {
            return 0;
        }
        service.reconnectRedis();
        source.sendSuccess(() -> Component.literal("TrChat Redis reconnect started."), true);
        return 1;
    }

    private int mute(CommandSourceStack source, boolean muted) {
        if (service == null) {
            return 0;
        }
        service.setGlobalMute(muted, true);
        source.sendSuccess(() -> Component.literal("Global chat mute is now " + (muted ? "on." : "off.")), true);
        return 1;
    }

    private int privateMessage(CommandSourceStack source, String target, String message) {
        if (service == null) {
            return 0;
        }
        try {
            return service.sendPrivate(source.getPlayerOrException(), target, message);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(Component.literal("This command can only be used by a player."));
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
            source.sendFailure(Component.literal("This command can only be used by a player."));
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
                source.sendFailure(Component.literal("Unknown channel: " + channelId));
                return 0;
            }
            return service.executeChannel(source.getPlayerOrException(), channel, "");
        } catch (CommandSyntaxException exception) {
            source.sendFailure(Component.literal("This command can only be used by a player."));
            return 0;
        }
    }

    private int serverSay(CommandSourceStack source, String message) {
        if (service == null) {
            return 0;
        }
        return service.sendServer(message, source.getPlayer());
    }

    private int openSnapshot(CommandSourceStack source, String snapshot) {
        if (service == null) {
            return 0;
        }
        try {
            return service.openFunctionSnapshot(source.getPlayerOrException(), snapshot) ? 1 : 0;
        } catch (CommandSyntaxException exception) {
            source.sendFailure(Component.literal("This command can only be used by a player."));
            return 0;
        }
    }

    private void registerPrivateAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .then(Commands.argument("player", StringArgumentType.word())
                .then(Commands.argument("message", StringArgumentType.greedyString())
                    .executes(context -> privateMessage(
                        context.getSource(),
                        StringArgumentType.getString(context, "player"),
                        StringArgumentType.getString(context, "message")
                    )))));
    }

    private void registerReplyAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .then(Commands.argument("message", StringArgumentType.greedyString())
                .executes(context -> reply(context.getSource(), StringArgumentType.getString(context, "message")))));
    }

    private void registerChannelAlias(
        CommandDispatcher<CommandSourceStack> dispatcher,
        String alias,
        String channelId
    ) {
        dispatcher.register(Commands.literal(alias)
            .executes(context -> executeBoundChannel(context.getSource(), channelId, ""))
            .then(Commands.argument("message", StringArgumentType.greedyString())
                .executes(context -> executeBoundChannel(
                    context.getSource(),
                    channelId,
                    StringArgumentType.getString(context, "message")
                ))));
    }

    private int executeBoundChannel(CommandSourceStack source, String channelId, String message) {
        if (service == null) {
            return 0;
        }
        try {
            ChannelDefinition channel = channels.byId(channelId).orElse(null);
            return channel == null ? 0 : service.executeChannel(source.getPlayerOrException(), channel, message);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(Component.literal("This channel command can only be used by a player."));
            return 0;
        }
    }
}
