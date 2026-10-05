package me.arasple.mc.trchat.util.proxy.common;

import java.util.UUID;

/**
 * One chunk of a chunked proxy message.
 * <p>
 * The wire format matches the TrChat Bukkit proxy protocol exactly:
 * <pre>
 * {
 *   "uid": "0000-0000-0000-0000",
 *   "data": "abc",
 *   "index": 1,
 *   "total": 100
 * }
 * </pre>
 * {@code data} holds a Base64 encoded JSON array of the original string
 * arguments, split so no single packet exceeds the client payload limit.
 *
 * @author 坏黑
 * @since 2019-02-13 9:28
 */
public final class MessagePacket {

    private final UUID uid;
    private final String data;
    private final int index;
    private final int total;

    public MessagePacket(UUID uid, String data, int index, int total) {
        this.uid = uid;
        this.data = data;
        this.index = index;
        this.total = total;
    }

    public UUID uid() {
        return uid;
    }

    public String data() {
        return data;
    }

    public int index() {
        return index;
    }

    public int total() {
        return total;
    }
}