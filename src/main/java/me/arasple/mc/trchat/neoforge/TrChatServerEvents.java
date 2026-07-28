package me.arasple.mc.trchat.neoforge;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import me.arasple.mc.trchat.neoforge.chat.ChatService;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerPlayer;
import net.neoforged.bus.api.SubscribeEvent;
import net.neoforged.neoforge.event.RegisterCommandsEvent;
import net.neoforged.neoforge.event.ServerChatEvent;
import net.neoforged.neoforge.event.entity.player.PlayerEvent;
import net.neoforged.neoforge.event.server.ServerStartedEvent;
import net.neoforged.neoforge.event.server.ServerStoppingEvent;
import net.neoforged.neoforge.event.tick.ServerTickEvent;

public final class TrChatServerEvents {

    private ChatService service;

    @SubscribeEvent
    public void onServerStarted(ServerStartedEvent event) {
        service = new ChatService(event.getServer());
        TrChatNeoForge.LOGGER.info("{} started (NeoForge 21.1.233+, Redis-only transport)", TrChatNeoForge.MOD_NAME);
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
        if (service != null && event.getEntity() instanceof ServerPlayer) {
            service.playerListChanged();
        }
    }

    @SubscribeEvent
    public void onPlayerLogout(PlayerEvent.PlayerLoggedOutEvent event) {
        if (service != null && event.getEntity() instanceof ServerPlayer) {
            service.playerListChanged();
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
        registerCommands(event.getDispatcher());
    }

    private void registerCommands(CommandDispatcher<CommandSourceStack> dispatcher) {
        dispatcher.register(Commands.literal("trchat")
            .then(Commands.literal("status")
                .executes(context -> status(context.getSource())))
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
                        ))))));

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
                .executes(context -> reply(
                    context.getSource(),
                    StringArgumentType.getString(context, "message")
                ))));

        registerPrivateAlias(dispatcher, "msg");
        registerPrivateAlias(dispatcher, "message");
        registerPrivateAlias(dispatcher, "tell");
        registerPrivateAlias(dispatcher, "whisper");
        registerPrivateAlias(dispatcher, "w");
        registerReplyAlias(dispatcher, "r");
        registerReplyAlias(dispatcher, "reply");
        registerGlobalAlias(dispatcher, "global");
        registerGlobalAlias(dispatcher, "all");
        registerGlobalAlias(dispatcher, "shout");
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
            "TrChat NeoForge: Redis " + redis + ", global mute " + (service.isGlobalMute() ? "on" : "off")
        ), false);
        return 1;
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
                .executes(context -> reply(
                    context.getSource(),
                    StringArgumentType.getString(context, "message")
                ))));
    }

    private void registerGlobalAlias(CommandDispatcher<CommandSourceStack> dispatcher, String alias) {
        dispatcher.register(Commands.literal(alias)
            .then(Commands.argument("message", StringArgumentType.greedyString())
                .executes(context -> global(
                    context.getSource(),
                    StringArgumentType.getString(context, "message")
                ))));
    }

    private int global(CommandSourceStack source, String message) {
        if (service == null) {
            return 0;
        }
        try {
            return service.sendGlobal(source.getPlayerOrException(), message);
        } catch (CommandSyntaxException exception) {
            source.sendFailure(Component.literal("This command can only be used by a player."));
            return 0;
        }
    }
}
