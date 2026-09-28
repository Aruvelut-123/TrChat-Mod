package me.arasple.mc.trchat.redis;

import me.arasple.mc.trchat.TrChatMod;
import me.arasple.mc.trchat.protocol.RedisEnvelopeCodec;
import me.arasple.mc.trchat.protocol.TrChatMessage;

import java.io.IOException;
import java.util.List;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.Consumer;

public final class RedisBridge implements AutoCloseable {

    private final RedisSettings settings;
    private final Consumer<TrChatMessage> receiver;
    private final AtomicBoolean running = new AtomicBoolean();
    private volatile RespConnection publisher;
    private volatile RespConnection subscriber;
    private volatile boolean subscribed;
    private Thread subscriberThread;

    public RedisBridge(RedisSettings settings, Consumer<TrChatMessage> receiver) {
        this.settings = settings;
        this.receiver = receiver;
    }

    public void start() {
        if (!running.compareAndSet(false, true)) {
            return;
        }
        //? if >=1.20.5 {
        subscriberThread = Thread.ofPlatform()
            .daemon(true)
            .name("TrChat Redis subscriber")
            .start(this::subscriptionLoop);
        //? } else {
        subscriberThread = new Thread(this::subscriptionLoop, "TrChat Redis subscriber");
        subscriberThread.setDaemon(true);
        subscriberThread.start();
        //? }
    }

    public boolean publish(TrChatMessage message) {
        if (!running.get()) {
            return false;
        }
        String payload = RedisEnvelopeCodec.encode(message);
        synchronized (this) {
            try {
                if (publisher == null) {
                    publisher = new RespConnection(settings);
                }
                Object response = publisher.command("PUBLISH", settings.channel(), payload);
                return response instanceof Long subscriberCount && subscriberCount > 0;
            } catch (IOException exception) {
                closePublisher();
                TrChatMod.LOGGER.warn("Redis publish failed: {}", exception.getMessage());
                return false;
            }
        }
    }

    public boolean isConnected() {
        return subscribed;
    }

    @Override
    public void close() {
        if (!running.compareAndSet(true, false)) {
            return;
        }
        subscribed = false;
        closeSubscriber();
        closePublisher();
        if (subscriberThread != null) {
            subscriberThread.interrupt();
        }
    }

    private void subscriptionLoop() {
        while (running.get()) {
            try {
                subscriber = new RespConnection(settings);
                subscriber.write("SUBSCRIBE", settings.channel());
                Object acknowledgement = subscriber.read();
                if (!(acknowledgement instanceof List<?>)) {
                    throw new IOException("Unexpected SUBSCRIBE response: " + acknowledgement);
                }
                subscribed = true;
                TrChatMod.LOGGER.info("Connected to Redis at {}:{} on channel '{}'",
                    settings.host(), settings.port(), settings.channel());

                while (running.get()) {
                    Object response = subscriber.read();
                    if (response instanceof List<?> values
                        && values.size() >= 3
                        && "message".equals(values.get(0))
                        && settings.channel().equals(values.get(1))
                        && values.get(2) instanceof String payload) {
                        receive(payload);
                    }
                }
            } catch (Exception exception) {
                if (running.get()) {
                    TrChatMod.LOGGER.warn("Redis subscription lost: {}", exception.getMessage());
                }
            } finally {
                subscribed = false;
                closeSubscriber();
            }

            if (running.get()) {
                try {
                    Thread.sleep(settings.reconnectDelayMillis());
                } catch (InterruptedException exception) {
                    Thread.currentThread().interrupt();
                }
            }
        }
    }

    private void receive(String payload) {
        try {
            receiver.accept(RedisEnvelopeCodec.decode(payload));
        } catch (RuntimeException exception) {
            TrChatMod.LOGGER.warn("Ignoring malformed TrChat Redis message: {}", exception.getMessage());
        }
    }

    private synchronized void closePublisher() {
        closeQuietly(publisher);
        publisher = null;
    }

    private void closeSubscriber() {
        closeQuietly(subscriber);
        subscriber = null;
    }

    private static void closeQuietly(AutoCloseable closeable) {
        if (closeable != null) {
            try {
                closeable.close();
            } catch (Exception ignored) {
            }
        }
    }
}
