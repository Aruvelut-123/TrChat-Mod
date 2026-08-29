package me.arasple.mc.trchat.chat;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import com.mojang.serialization.DataResult;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.resources.RegistryOps;

import java.util.Optional;

public final class ComponentJson {

    private ComponentJson() {
    }

    public static String serialize(Component component, MinecraftServer server) {
        //? if >=1.21.11 {
        RegistryOps<JsonElement> ops = RegistryOps.create(com.mojang.serialization.JsonOps.INSTANCE, server.registryAccess());
        DataResult<JsonElement> result = net.minecraft.network.chat.ComponentSerialization.CODEC.encodeStart(ops, component);
        return result.result().map(JsonElement::toString).orElse("{}");
        //? } else {
        return Component.Serializer.toJson(component, server.registryAccess());
        //? }
    }

    public static Component deserialize(String json, String fallback, MinecraftServer server) {
        if (json != null && !json.isBlank()) {
            try {
                //? if >=1.21.11 {
                RegistryOps<JsonElement> ops = RegistryOps.create(com.mojang.serialization.JsonOps.INSTANCE, server.registryAccess());
                DataResult<Component> result = net.minecraft.network.chat.ComponentSerialization.CODEC.parse(ops, JsonParser.parseString(json));
                Optional<Component> component = result.result();
                if (component.isPresent()) {
                    return component.get();
                }
                //? } else {
                Component component = Component.Serializer.fromJson(json, server.registryAccess());
                if (component != null) {
                    return component;
                }
                //? }
            } catch (RuntimeException ignored) {
            }
        }
        return LegacyText.parse(fallback);
    }
}
