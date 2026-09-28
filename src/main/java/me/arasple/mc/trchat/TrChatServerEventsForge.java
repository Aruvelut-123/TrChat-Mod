//? if forge {
package me.arasple.mc.trchat;

import me.arasple.mc.trchat.channel.ChannelManager;
import me.arasple.mc.trchat.chat.ChatService;
import me.arasple.mc.trchat.config.TrChatConfig;
import me.arasple.mc.trchat.permission.TrChatPermissions;
import me.arasple.mc.trchat.update.UpdateChecker;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraftforge.event.AnvilUpdateEvent;
import net.minecraftforge.event.CommandEvent;
import net.minecraftforge.event.RegisterCommandsEvent;
import net.minecraftforge.event.ServerChatEvent;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.event.entity.living.LivingDamageEvent;
import net.minecraftforge.event.entity.player.PlayerEvent;
import net.minecraftforge.event.level.ChunkEvent;
import net.minecraftforge.event.server.ServerStartedEvent;
import net.minecraftforge.event.server.ServerStoppingEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.server.permission.events.PermissionGatherEvent;

/**
 * Forge 1.20.1 (LTS) event binding. The NeoForge variant lives in
 * {@link TrChatServerEvents}; the Fabric variant in {@link TrChatServerEventsFabric}.
 */
public final class TrChatServerEventsForge extends TrChatCommands {

    public TrChatServerEventsForge() {
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
        TrChatMod.LOGGER.info("{} started with {} channels (Forge 1.20.1 LTS, Redis-only transport)",
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
    public void onDamage(LivingDamageEvent event) {
        if (service != null && event.getEntity() instanceof ServerPlayer player) {
            service.recordDamage(player, event.getAmount());
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
    public void onServerTick(TickEvent.ServerTickEvent event) {
        if (event.phase == TickEvent.Phase.END && service != null) {
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
