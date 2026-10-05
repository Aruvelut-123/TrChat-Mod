package me.arasple.mc.trchat.util.proxy.transport;

//? if neoforge {
import me.arasple.mc.trchat.util.proxy.ProxyMode;
import me.arasple.mc.trchat.util.proxy.ProxyTransport;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.network.protocol.common.ClientboundCustomPayloadPacket;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.neoforged.neoforge.network.event.RegisterPayloadHandlersEvent;
import net.neoforged.neoforge.network.registration.NetworkRegistry;

import java.util.Set;

/** NeoForge raw custom-payload transport for Bukkit-compatible proxy channels. */
public final class NeoForgeProxyTransport {

    private NeoForgeProxyTransport() {
    }

    public static void register(RegisterPayloadHandlersEvent event) {
        var registrar = event.registrar("trchat").optional();
        registrar.playBidirectional(
            ProxyPayload.BUNGEE_TYPE,
            ProxyPayload.codec(ProxyPayload.BUNGEE_TYPE),
            (payload, context) -> {
                if (context.flow() == PacketFlow.SERVERBOUND) {
                    context.enqueueWork(() -> ProxyTransport.accept(ProxyMode.BUNGEE, payload.bytes()));
                }
            }
        );
        registrar.playToServer(
            ProxyPayload.VELOCITY_INCOMING_TYPE,
            ProxyPayload.codec(ProxyPayload.VELOCITY_INCOMING_TYPE),
            (payload, context) -> context.enqueueWork(
                () -> ProxyTransport.accept(ProxyMode.VELOCITY, payload.bytes())
            )
        );
        registrar.playToClient(
            ProxyPayload.VELOCITY_OUTGOING_TYPE,
            ProxyPayload.codec(ProxyPayload.VELOCITY_OUTGOING_TYPE),
            (payload, context) -> {
                // trchat:proxy is the server-to-proxy leg and has no inbound handler.
            }
        );
    }

    public static final class NeoForgeSender implements ProxyTransport.Sender {

        private final MinecraftServer server;

        public NeoForgeSender(MinecraftServer server) {
            this.server = server;
        }

        @Override
        public boolean isReady() {
            return !server.getPlayerList().getPlayers().isEmpty();
        }

        @Override
        public boolean send(ProxyMode mode, byte[] packet) {
            ServerPlayer player = server.getPlayerList().getPlayers().stream().findFirst().orElse(null);
            if (player == null) {
                return false;
            }
            ProxyPayload payload = ProxyPayload.forMode(mode, packet);
            // The proxy consumes this vanilla plugin channel without a NeoForge handshake.
            NetworkRegistry.onMinecraftRegister(player.connection.getConnection(), Set.of(payload.type().id()));
            player.connection.send(new ClientboundCustomPayloadPacket(payload));
            return true;
        }
    }

}
//? }
