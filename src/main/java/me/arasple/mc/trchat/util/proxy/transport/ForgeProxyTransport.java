package me.arasple.mc.trchat.util.proxy.transport;

//? if forge {
import io.netty.buffer.Unpooled;
import me.arasple.mc.trchat.util.proxy.ProxyMode;
import me.arasple.mc.trchat.util.proxy.ProxyTransport;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundCustomPayloadPacket;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraftforge.network.NetworkEvent;
import net.minecraftforge.network.NetworkDirection;
import net.minecraftforge.network.NetworkRegistry;
import net.minecraftforge.network.event.EventNetworkChannel;

/** Forge 1.20.1 raw custom-payload transport for Bukkit proxy channels. */
public final class ForgeProxyTransport {

    private static final ResourceLocation BUNGEE_CHANNEL = new ResourceLocation("trchat", "main");
    private static final ResourceLocation VELOCITY_INCOMING_CHANNEL = new ResourceLocation("trchat", "server");
    private static final ResourceLocation VELOCITY_OUTGOING_CHANNEL = new ResourceLocation("trchat", "proxy");

    private static boolean registered;

    private ForgeProxyTransport() {
    }

    public static synchronized void register() {
        if (registered) {
            return;
        }
        registered = true;
        createChannel(BUNGEE_CHANNEL, ProxyMode.BUNGEE);
        createChannel(VELOCITY_INCOMING_CHANNEL, ProxyMode.VELOCITY);
    }

    private static void createChannel(ResourceLocation name, ProxyMode mode) {
        EventNetworkChannel channel = NetworkRegistry.newEventChannel(
            name,
            () -> "1",
            NetworkRegistry.acceptMissingOr("1"),
            NetworkRegistry.acceptMissingOr("1")
        );
        channel.addListener(event -> handleInbound(mode, event));
    }

    private static void handleInbound(ProxyMode mode, NetworkEvent event) {
        NetworkEvent.Context context = event.getSource().get();
        if (context.getDirection() != NetworkDirection.PLAY_TO_SERVER) {
            return;
        }
        FriendlyByteBuf payload = event.getPayload();
        if (payload == null || !payload.isReadable()) {
            return;
        }
        byte[] bytes = new byte[payload.readableBytes()];
        payload.getBytes(payload.readerIndex(), bytes);
        context.setPacketHandled(true);
        ProxyTransport.accept(mode, bytes);
    }

    public static final class ForgeSender implements ProxyTransport.Sender {

        private final MinecraftServer server;

        public ForgeSender(MinecraftServer server) {
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
            ResourceLocation channel = mode == ProxyMode.BUNGEE
                ? BUNGEE_CHANNEL
                : VELOCITY_OUTGOING_CHANNEL;
            FriendlyByteBuf payload = new FriendlyByteBuf(Unpooled.wrappedBuffer(packet));
            player.connection.send(new ClientboundCustomPayloadPacket(channel, payload));
            return true;
        }
    }
}
//? }
