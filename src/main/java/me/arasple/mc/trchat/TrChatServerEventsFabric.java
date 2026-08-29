//? if fabric {
package me.arasple.mc.trchat;

import me.arasple.mc.trchat.channel.ChannelManager;
import me.arasple.mc.trchat.chat.ChatService;
import me.arasple.mc.trchat.config.TrChatConfig;
import me.arasple.mc.trchat.update.UpdateChecker;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.entity.event.v1.ServerLivingEntityEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerChunkEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.fabric.api.message.v1.ServerMessageEvents;
import net.fabricmc.fabric.api.networking.v1.ServerPlayConnectionEvents;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.network.chat.ChatType;
import net.minecraft.network.chat.PlayerChatMessage;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.level.chunk.LevelChunk;

/**
 * Fabric event binding. The NeoForge variant lives in {@link TrChatServerEvents}.
 */
public final class TrChatServerEventsFabric extends TrChatCommands {

    public TrChatServerEventsFabric() {
        ServerLifecycleEvents.SERVER_STARTED.register(server -> {
            service = new ChatService(server, channels);
            if (TrChatConfig.UPDATE_CHECK_ENABLED.get()) {
                updateChecker = new UpdateChecker(server, service.languages(), modVersion());
                updateChecker.start();
            }
            TrChatMod.LOGGER.info("{} started with {} channels (Fabric, Redis-only transport)",
                TrChatMod.MOD_NAME, service.channelCount());
        });

        ServerLifecycleEvents.SERVER_STOPPING.register(server -> {
            if (updateChecker != null) {
                updateChecker.close();
                updateChecker = null;
            }
            if (service != null) {
                service.close();
                service = null;
            }
        });

        ServerMessageEvents.ALLOW_CHAT_MESSAGE.register((message, player, bound) -> {
            if (service == null) {
                return true;
            }
            if (isDisabledWorld(player)) {
                return true;
            }
            service.handleChat(player, message.signedContent());
            return false;
        });

        ServerPlayConnectionEvents.JOIN.register((handler, sender, server) -> {
            if (service != null) {
                ServerPlayer player = handler.player;
                if (player != null) {
                    service.playerJoined(player);
                    if (updateChecker != null) {
                        updateChecker.notifyPlayer(player);
                    }
                }
            }
        });

        ServerPlayConnectionEvents.DISCONNECT.register((handler, server) -> {
            if (service != null) {
                ServerPlayer player = handler.player;
                if (player != null) {
                    service.playerLeft(player);
                }
            }
        });

        ServerLivingEntityEvents.AFTER_DAMAGE.register((entity, source, baseDamage, damageTaken, blocked) -> {
            if (service != null && entity instanceof ServerPlayer player) {
                service.recordDamage(player, damageTaken);
            }
        });

        //? if >=26.1 {
        ServerChunkEvents.CHUNK_LOAD.register((world, chunk, fromGeneration) -> {
            if (service != null) {
                service.chunkLoaded(chunk);
            }
        });
        //? } else {
        ServerChunkEvents.CHUNK_LOAD.register((world, chunk) -> {
            if (service != null) {
                service.chunkLoaded(chunk);
            }
        });
        //? }
        ServerChunkEvents.CHUNK_UNLOAD.register((world, chunk) -> {
            if (service != null) {
                service.chunkUnloaded(chunk);
            }
        });

        ServerTickEvents.END_SERVER_TICK.register(server -> {
            if (service != null) {
                service.tick();
            }
        });

        CommandRegistrationCallback.EVENT.register((dispatcher, registryAccess, environment) -> {
            channels.reload();
            commandDispatcher = dispatcher;
            registerCommands(dispatcher);
        });
    }
}
//? }