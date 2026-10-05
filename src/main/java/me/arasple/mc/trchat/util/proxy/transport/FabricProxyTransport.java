package me.arasple.mc.trchat.util.proxy.transport;

//? if fabric {
import me.arasple.mc.trchat.util.proxy.ProxyMode;
import me.arasple.mc.trchat.util.proxy.ProxyTransport;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.fabricmc.fabric.api.networking.v1.ServerPlayNetworking;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;

/** Fabric raw custom-payload transport for Bukkit-compatible proxy channels. */
public final class FabricProxyTransport {

    private FabricProxyTransport() {
    }

    public static void register() {
        registerType(ProxyPayload.BUNGEE_TYPE);
        registerType(ProxyPayload.VELOCITY_INCOMING_TYPE);
        registerType(ProxyPayload.VELOCITY_OUTGOING_TYPE);
        ServerPlayNetworking.registerGlobalReceiver(
            ProxyPayload.BUNGEE_TYPE,
            (payload, context) -> ProxyTransport.accept(ProxyMode.BUNGEE, payload.bytes())
        );
        ServerPlayNetworking.registerGlobalReceiver(
            ProxyPayload.VELOCITY_INCOMING_TYPE,
            (payload, context) -> ProxyTransport.accept(ProxyMode.VELOCITY, payload.bytes())
        );
    }

    private static void registerType(net.minecraft.network.protocol.common.custom.CustomPacketPayload.Type<ProxyPayload> type) {
        //? if >=26.1 {
        PayloadTypeRegistry.serverboundPlay().register(type, ProxyPayload.codec(type));
        PayloadTypeRegistry.clientboundPlay().register(type, ProxyPayload.codec(type));
        //? } else {
        PayloadTypeRegistry.playC2S().register(type, ProxyPayload.codec(type));
        PayloadTypeRegistry.playS2C().register(type, ProxyPayload.codec(type));
        //? }
    }

    public static final class FabricSender implements ProxyTransport.Sender {

        private final MinecraftServer server;

        public FabricSender(MinecraftServer server) {
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
            ServerPlayNetworking.send(player, payload);
            return true;
        }
    }
}
//? }
