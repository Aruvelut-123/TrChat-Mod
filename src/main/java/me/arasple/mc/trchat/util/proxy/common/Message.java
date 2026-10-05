package me.arasple.mc.trchat.util.proxy.common;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonParser;

import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.UUID;

/** Accumulates the ordered Base64 fragments used by the Bukkit proxy protocol. */
public final class Message {
    private final Map<Integer, MessagePacket> packets = new TreeMap<>();
    private UUID uid;
    private int total;

    public synchronized boolean add(MessagePacket packet) {
        if (packet == null || packet.uid() == null || packet.data() == null
            || packet.total() < 1 || packet.total() > MessageReader.MAX_MESSAGE_CHUNKS
            || packet.index() < 1 || packet.index() > packet.total()
            || packet.data().length() > MessageBuilder.MESSAGE_LENGTH) {
            return false;
        }
        if (uid != null && (!uid.equals(packet.uid()) || total != packet.total())) {
            return false;
        }
        if (packets.containsKey(packet.index())) {
            return false;
        }
        uid = packet.uid();
        total = packet.total();
        packets.put(packet.index(), packet);
        return true;
    }

    public synchronized boolean isCompleted() {
        return total != 0 && packets.size() == total;
    }

    public synchronized String[] build() {
        if (!isCompleted()) {
            throw new IllegalStateException("Proxy message is incomplete");
        }
        StringBuilder builder = new StringBuilder();
        for (MessagePacket packet : packets.values()) {
            builder.append(packet.data());
        }
        JsonArray json = JsonParser.parseString(ByteUtils.decode(builder.toString())).getAsJsonArray();
        String[] args = new String[json.size()];
        for (int index = 0; index < json.size(); index++) {
            JsonElement element = json.get(index);
            if (!element.isJsonPrimitive() || !element.getAsJsonPrimitive().isString()) {
                throw new IllegalArgumentException("Proxy message arguments must be strings");
            }
            args[index] = element.getAsString();
        }
        return args;
    }

    public synchronized List<MessagePacket> packets() {
        return List.copyOf(packets.values());
    }
}
