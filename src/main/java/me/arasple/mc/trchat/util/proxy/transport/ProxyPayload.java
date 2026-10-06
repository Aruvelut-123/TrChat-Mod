//? if neoforge {
package me.arasple.mc.trchat.util.proxy.transport;

import me.arasple.mc.trchat.util.proxy.ProxyMode;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
//? if >=1.21.11 {
import net.minecraft.resources.Identifier;
//? } else {
import net.minecraft.resources.ResourceLocation;
//? }

/** Raw plugin-message chunk carried on a vanilla custom-payload channel. */
public record ProxyPayload(CustomPacketPayload.Type<ProxyPayload> type, byte[] bytes)
    implements CustomPacketPayload {

    /**
     * {@code CustomPacketPayload.createType(String)} treats its argument as a
     * path and adds {@code minecraft:}. Plugin-message channels already carry
     * their own namespace, so construct the payload type with the full ID.
     */
    private static CustomPacketPayload.Type<ProxyPayload> proxyType(String path) {
        //? if >=1.21.11 {
        return new CustomPacketPayload.Type<>(
            Identifier.fromNamespaceAndPath("trchat", path)
        );
        //? } else {
        return new CustomPacketPayload.Type<>(
            ResourceLocation.fromNamespaceAndPath("trchat", path)
        );
        //? }
    }

    public static final CustomPacketPayload.Type<ProxyPayload> BUNGEE_TYPE =
        proxyType("main");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_INCOMING_TYPE =
        proxyType("server");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_OUTGOING_TYPE =
        proxyType("proxy");

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
//? if >=1.21.11 {
import net.minecraft.resources.Identifier;
//? } else {
import net.minecraft.resources.ResourceLocation;
//? }

/** Raw plugin-message chunk carried on a vanilla custom-payload channel. */
public record ProxyPayload(CustomPacketPayload.Type<ProxyPayload> type, byte[] bytes)
    implements CustomPacketPayload {

    /**
     * {@code CustomPacketPayload.createType(String)} treats its argument as a
     * path and adds {@code minecraft:}. Plugin-message channels already carry
     * their own namespace, so construct the payload type with the full ID.
     */
    private static CustomPacketPayload.Type<ProxyPayload> proxyType(String path) {
        //? if >=1.21.11 {
        return new CustomPacketPayload.Type<>(
            Identifier.fromNamespaceAndPath("trchat", path)
        );
        //? } else {
        return new CustomPacketPayload.Type<>(
            ResourceLocation.fromNamespaceAndPath("trchat", path)
        );
        //? }
    }

    public static final CustomPacketPayload.Type<ProxyPayload> BUNGEE_TYPE =
        proxyType("main");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_INCOMING_TYPE =
        proxyType("server");
    public static final CustomPacketPayload.Type<ProxyPayload> VELOCITY_OUTGOING_TYPE =
        proxyType("proxy");

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
