package me.arasple.mc.trchat.util.proxy;

import java.util.Locale;

/** Proxy protocol selected for raw plugin-message transport. */
public enum ProxyMode {
    BUNGEE,
    VELOCITY;

    public static ProxyMode parse(String value) {
        if (value == null) {
            return VELOCITY;
        }
        try {
            return valueOf(value.trim().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException ignored) {
            return VELOCITY;
        }
    }
}
