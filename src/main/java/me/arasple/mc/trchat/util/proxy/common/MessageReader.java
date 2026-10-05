package me.arasple.mc.trchat.util.proxy.common;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.nio.charset.StandardCharsets;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.function.Consumer;
import java.util.function.LongSupplier;

/** Reads bounded, expiring Bukkit proxy fragments and delivers each UID once. */
public final class MessageReader {
    public static final int MAX_MESSAGE_CHUNKS = 128;
    public static final int MAX_PENDING_MESSAGES = 128;
    public static final int MAX_BUFFERED_CHARACTERS = 8 * 1024 * 1024;
    private static final int MAX_PACKET_BYTES = 32767;
    private static final int MAX_COMPLETED_MESSAGES = 1024;
    private static final long MESSAGE_TTL_NANOS = 10_000_000_000L;

    private final Map<UUID, PendingMessage> messages = new LinkedHashMap<>();
    private final Map<UUID, Long> completed = new LinkedHashMap<>();
    private final Consumer<String[]> callback;
    private final LongSupplier nanoTime;
    private int bufferedCharacters;

    public MessageReader(Consumer<String[]> callback) {
        this(callback, System::nanoTime);
    }

    /** The clock is injectable so expiration can be verified without sleeping. */
    public MessageReader(Consumer<String[]> callback, LongSupplier nanoTime) {
        this.callback = Objects.requireNonNull(callback);
        this.nanoTime = Objects.requireNonNull(nanoTime);
    }

    public void read(byte[] packet) {
        if (packet == null || packet.length == 0 || packet.length > MAX_PACKET_BYTES) {
            return;
        }
        String[] result;
        try {
            MessagePacket parsed = parse(new String(packet, StandardCharsets.UTF_8));
            if (parsed == null) {
                return;
            }
            result = accept(parsed);
        } catch (RuntimeException exception) {
            return;
        }
        if (result != null) {
            callback.accept(result);
        }
    }

    private synchronized String[] accept(MessagePacket packet) {
        long now = nanoTime.getAsLong();
        expire(now);
        if (completed.containsKey(packet.uid())) {
            return null;
        }
        PendingMessage pending = messages.get(packet.uid());
        if (pending == null) {
            while (messages.size() >= MAX_PENDING_MESSAGES) {
                evictOldest();
            }
            pending = new PendingMessage(now);
            messages.put(packet.uid(), pending);
        }
        if (!pending.message.add(packet)) {
            return null;
        }
        pending.characters += packet.data().length();
        bufferedCharacters += packet.data().length();
        while (bufferedCharacters > MAX_BUFFERED_CHARACTERS) {
            evictOldest();
        }
        if (messages.get(packet.uid()) != pending || !pending.message.isCompleted()) {
            return null;
        }
        messages.remove(packet.uid());
        bufferedCharacters -= pending.characters;
        completed.put(packet.uid(), now);
        while (completed.size() > MAX_COMPLETED_MESSAGES) {
            Iterator<UUID> iterator = completed.keySet().iterator();
            iterator.next();
            iterator.remove();
        }
        return pending.message.build();
    }

    private void expire(long now) {
        Iterator<Map.Entry<UUID, PendingMessage>> iterator = messages.entrySet().iterator();
        while (iterator.hasNext()) {
            PendingMessage pending = iterator.next().getValue();
            if (now - pending.createdAtNanos >= MESSAGE_TTL_NANOS) {
                bufferedCharacters -= pending.characters;
                iterator.remove();
            }
        }
        completed.values().removeIf(createdAt -> now - createdAt >= MESSAGE_TTL_NANOS);
    }

    private void evictOldest() {
        Iterator<Map.Entry<UUID, PendingMessage>> iterator = messages.entrySet().iterator();
        if (!iterator.hasNext()) {
            return;
        }
        PendingMessage oldest = iterator.next().getValue();
        bufferedCharacters -= oldest.characters;
        iterator.remove();
    }

    private static MessagePacket parse(String json) {
        JsonObject object = JsonParser.parseString(json).getAsJsonObject();
        if (!isString(object, "uid") || !isString(object, "data")
            || !object.has("index") || !object.has("total")) {
            return null;
        }
        String uid = object.get("uid").getAsString();
        String data = object.get("data").getAsString();
        int index = object.get("index").getAsBigDecimal().intValueExact();
        int total = object.get("total").getAsBigDecimal().intValueExact();
        if (uid.length() != 36 || data.length() > MessageBuilder.MESSAGE_LENGTH
            || index < 1 || total < 1 || total > MAX_MESSAGE_CHUNKS || index > total) {
            return null;
        }
        return new MessagePacket(UUID.fromString(uid), data, index, total);
    }

    private static boolean isString(JsonObject object, String key) {
        return object.has(key) && object.get(key).isJsonPrimitive()
            && object.get(key).getAsJsonPrimitive().isString();
    }

    private static final class PendingMessage {
        private final Message message = new Message();
        private final long createdAtNanos;
        private int characters;

        private PendingMessage(long createdAtNanos) {
            this.createdAtNanos = createdAtNanos;
        }
    }
}
