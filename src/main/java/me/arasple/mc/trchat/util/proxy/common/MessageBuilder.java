package me.arasple.mc.trchat.util.proxy.common;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

/**
 * Splits a TrChat proxy message into payload-sized chunks.
 * The first argument becomes the message UID; every other argument becomes a
 * string element of the JSON array that is Base64 encoded and chunked, exactly
 * like the TrChat Bukkit proxy protocol.
 *
 * @author 坏黑
 * @since 2020-10-15
 */
public final class MessageBuilder {

    /** Maximum bytes carried by one packet, safely below the client payload limit */
    public static final int MESSAGE_LENGTH = 30000;

    private MessageBuilder() {
    }

    public static List<byte[]> create(String... message) {
        if (message.length < 1) {
            return List.of();
        }
        String uid = message[0];
        JsonArray json = new JsonArray();
        for (int i = 1; i < message.length; i++) {
            json.add(new JsonPrimitive(message[i]));
        }
        String source = ByteUtils.encode(json.toString());

        List<byte[]> packets = new ArrayList<>();
        int times = (int) Math.ceil(source.length() / (double) MESSAGE_LENGTH);
        for (int i = 0; i < times; i++) {
            int from = i * MESSAGE_LENGTH;
            int to = Math.min(from + MESSAGE_LENGTH, source.length());
            String data = source.substring(from, to);
            JsonObject packet = new JsonObject();
            packet.addProperty("uid", uid);
            packet.addProperty("index", i + 1);
            packet.addProperty("total", times);
            packet.addProperty("data", data);
            packets.add(packet.toString().getBytes(StandardCharsets.UTF_8));
        }
        return packets;
    }

    /** Convenience: build packets tagged with a fresh UID */
    public static List<byte[]> create(UUID uid, String... messages) {
        List<String> all = new ArrayList<>(messages.length + 1);
        all.add(uid.toString());
        for (String message : messages) {
            all.add(message);
        }
        return create(all.toArray(new String[0]));
    }
}
