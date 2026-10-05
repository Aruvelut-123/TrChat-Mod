package me.arasple.mc.trchat.util.proxy.common;

import java.nio.charset.StandardCharsets;
import java.util.Base64;

/**
 * Trivial Base64 helpers shared with the TrChat Bukkit proxy protocol.
 *
 * @author 坏黑
 * @since 2018-04-16
 */
public final class ByteUtils {

    private ByteUtils() {
    }

    public static String encode(String value) {
        return Base64.getEncoder().encodeToString(value.getBytes(StandardCharsets.UTF_8));
    }

    public static String decode(String value) {
        return new String(Base64.getDecoder().decode(value), StandardCharsets.UTF_8);
    }
}
