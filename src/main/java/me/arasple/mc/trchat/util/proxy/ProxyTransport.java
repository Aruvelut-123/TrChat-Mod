package me.arasple.mc.trchat.util.proxy;

import java.util.function.Consumer;

/**
 * Loader-independent bridge to the installed proxy transport.
 * <p>
 * Each loader registers its concrete {@link Sender} once at startup; the
 * proxy bridge publishes through it and feeds inbound chunks to the receiver.
 */
public final class ProxyTransport {

    /** A sender implementation provided by the current loader's transport. */
    public interface Sender {
        /** Whether at least one backend connection can carry packets right now. */
        boolean isReady();

        /** Send one raw packet towards the proxy. */
        boolean send(ProxyMode mode, byte[] packet);
    }

    private static volatile Sender sender;
    private static volatile ProxyMode mode = ProxyMode.VELOCITY;

    private static volatile Consumer<byte[]> receiver = ignored -> {
    };

    private ProxyTransport() {
    }

    public static void setSender(Sender value) {
        sender = value;
    }

    public static void setMode(ProxyMode value) {
        mode = value == null ? ProxyMode.VELOCITY : value;
    }

    public static void setReceiver(Consumer<byte[]> value) {
        receiver = value == null ? ignored -> {
        } : value;
    }

    public static boolean send(byte[] packet) {
        Sender current = sender;
        return current != null && current.isReady() && current.send(mode, packet);
    }

    public static boolean isReady() {
        Sender current = sender;
        return current != null && current.isReady();
    }

    /** Called by the loader's payload handler, possibly on a network thread. */
    public static void accept(byte[] packet) {
        receiver.accept(packet);
    }

    public static void accept(ProxyMode sourceMode, byte[] packet) {
        if (mode == sourceMode) {
            accept(packet);
        }
    }
}
