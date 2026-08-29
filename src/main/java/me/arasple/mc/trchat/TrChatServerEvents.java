//? if neoforge {
package me.arasple.mc.trchat;

import me.arasple.mc.trchat.channel.ChannelManager;
import me.arasple.mc.trchat.chat.ChatService;
import me.arasple.mc.trchat.config.TrChatConfig;
import me.arasple.mc.trchat.permission.TrChatPermissions;
import me.arasple.mc.trchat.update.UpdateChecker;
import net.minecraft.commands.CommandSourceStack;
//? if >=1.21.11 {
import net.minecraft.server.permissions.Permission;
import net.minecraft.server.permissions.PermissionLevel;
//? }
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.level.chunk.LevelChunk;
import net.neoforged.bus.api.EventPriority;
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
import net.neoforged.neoforge.server.permission.events.PermissionGatherEvent;

public final class TrChatServerEvents extends TrChatCommands {

    public TrChatServerEvents() {
    }

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
        TrChatMod.LOGGER.info("{} started with {} channels (NeoForge, Redis-only transport)",
            TrChatMod.MOD_NAME, service.channelCount());
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

    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void onChat(ServerChatEvent event) {
        if (service == null) {
            return;
        }
        handleChat(event);
    }

    private void handleChat(ServerChatEvent event) {
        if (isDisabledWorld(event.getPlayer())) {
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
            //? if >=26.1 {
            service.recordDamage(player, event.getInflictedDamage());
            //? } else {
            service.recordDamage(player, event.getNewDamage());
            //? }
        }
    }

    @SubscribeEvent(priority = EventPriority.LOWEST)
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
}
//? }