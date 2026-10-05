//? if neoforge {
package me.arasple.mc.trchat.util.proxy.transport;

import me.arasple.mc.trchat.util.proxy.ProxyMode;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;

/** Raw plugin-message chunk carried on a vanilla custom-payload channel. */
public record ProxyPayload(CustomPacketPayload.Type<ProxyPayload> type, byte[] bytes)
    implements CustomPacketPayload {

    public static final CustomPacketPayload.Type<ProxyPayload> BUNGEE_TYPE =
        CustomPacketPayload.createType("trchat:main");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_INCOMING_TYPE =
        CustomPacketPayload.createType("trchat:server");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_OUTGOING_TYPE =
        CustomPacketPayload.createType("trchat:proxy");

    public static StreamCodec<FriendlyByteBuf, ProxyPayload> codec(CustomPacketPayload.Type<ProxyPayload> type) {
        return new StreamCodec<>() {
            @Override
            public ProxyPayload decode(FriendlyByteBuf buffer) {
                byte[] bytes = new byte[buffer.readableBytes()];
                buffer.readBytes(bytes);
                return new ProxyPayload(type, bytes);
            }

            @Override
            public void encode(FriendlyByteBuf buffer, ProxyPayload value) {
                buffer.writeBytes(value.bytes());
            }
        };
    }

    public static ProxyPayload forMode(ProxyMode mode, byte[] bytes) {
        return new ProxyPayload(
            mode == ProxyMode.BUNGEE ? BUNGEE_TYPE : VELOCITY_OUTGOING_TYPE,
            bytes
        );
    }
}
//? } else if fabric {
package me.arasple.mc.trchat.util.proxy.transport;

import me.arasple.mc.trchat.util.proxy.ProxyMode;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;

/** Raw plugin-message chunk carried on a vanilla custom-payload channel. */
public record ProxyPayload(CustomPacketPayload.Type<ProxyPayload> type, byte[] bytes)
    implements CustomPacketPayload {

    public static final CustomPacketPayload.Type<ProxyPayload> BUNGEE_TYPE =
        CustomPacketPayload.createType("trchat:main");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_INCOMING_TYPE =
        CustomPacketPayload.createType("trchat:server");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_OUTGOING_TYPE =
        CustomPacketPayload.createType("trchat:proxy");

    public static StreamCodec<FriendlyByteBuf, ProxyPayload> codec(CustomPacketPayload.Type<ProxyPayload> type) {
        return new StreamCodec<>() {
            @Override
            public ProxyPayload decode(FriendlyByteBuf buffer) {
                byte[] bytes = new byte[buffer.readableBytes()];
                buffer.readBytes(bytes);
                return new ProxyPayload(type, bytes);
            }

            @Override
            public void encode(FriendlyByteBuf buffer, ProxyPayload value) {
                buffer.writeBytes(value.bytes());
            }
        };
    }

    public static ProxyPayload forMode(ProxyMode mode, byte[] bytes) {
        return new ProxyPayload(
            mode == ProxyMode.BUNGEE ? BUNGEE_TYPE : VELOCITY_OUTGOING_TYPE,
            bytes
        );
    }
}
//? } else {
//? }
