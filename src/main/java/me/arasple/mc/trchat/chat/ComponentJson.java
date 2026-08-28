package me.arasple.mc.trchat.chat;

import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;

public final class ComponentJson {

    private ComponentJson() {
    }

    public static String serialize(Component component, MinecraftServer server) {
        return Component.Serializer.toJson(component, server.registryAccess());
    }

    public static Component deserialize(String json, String fallback, MinecraftServer server) {
        if (json != null && !json.isBlank()) {
            try {
                Component component = Component.Serializer.fromJson(json, server.registryAccess());
                if (component != null) {
                    return component;
                }
            } catch (RuntimeException ignored) {
            }
        }
        return LegacyText.parse(fallback);
    }
}
