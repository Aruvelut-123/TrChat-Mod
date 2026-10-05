package me.arasple.mc.trchat.util.proxy;

import me.arasple.mc.trchat.protocol.TrChatMessage;
import me.arasple.mc.trchat.util.proxy.common.MessageBuilder;
import me.arasple.mc.trchat.util.proxy.common.MessageReader;

import java.util.List;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.Consumer;

/**
 * Cross-server transport that piggybacks on the TrChat proxy protocol
 * (chunked, Base64 encoded JSON arrays). The UID belongs to the chunk envelope;
 * the decoded array starts with the action ("BroadcastRaw", "SendPrivateRaw", ...).
 */
public final class ProxyBridge implements AutoCloseable {

    private final AtomicBoolean running = new AtomicBoolean();
    private final MessageReader reader;

    public ProxyBridge(Consumer<TrChatMessage> receiver) {
        this.reader = new MessageReader(args -> {
            if (args.length == 0 || !running.get()) {
                return;
            }
            receiver.accept(new TrChatMessage(List.of(args)));
        });
    }

    public void start() {
        if (running.compareAndSet(false, true)) {
            ProxyTransport.setReceiver(reader::read);
        }
    }

    public boolean publish(TrChatMessage message) {
        if (!running.get() || message.data().isEmpty()) {
            return false;
        }
        List<byte[]> packets = MessageBuilder.create(UUID.randomUUID(), message.data().toArray(String[]::new));
        for (byte[] packet : packets) {
            if (!ProxyTransport.send(packet)) {
                return false;
            }
        }
        return true;
    }

    public boolean isConnected() {
        return running.get() && ProxyTransport.isReady();
    }

    @Override
    public void close() {
        if (running.compareAndSet(true, false)) {
            ProxyTransport.setReceiver(null);
            ProxyTransport.setSender(null);
        }
    }
}
