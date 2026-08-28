package me.arasple.mc.trchat.protocol;

import java.util.UUID;

/**
 * Wire-format helpers shared with TrChat Bukkit's FastUUID based protocol.
 */
public final class TrChatProtocol {

    public static final UUID NIL_UUID = new UUID(0L, 0L);

    private TrChatProtocol() {
    }

    public static String formatUuid(UUID uuid) {
        return uuid.toString().replace("-", "");
    }

    public static boolean isCrossServerSafeItemNamespace(String namespace) {
        return "minecraft".equals(namespace);
    }

    public static UUID parseUuid(String value) {
        if (value == null) {
            return null;
        }
        String normalized = value.trim();
        if (normalized.length() == 32) {
            normalized = normalized.substring(0, 8) + '-'
                + normalized.substring(8, 12) + '-'
                + normalized.substring(12, 16) + '-'
                + normalized.substring(16, 20) + '-'
                + normalized.substring(20);
        }
        try {
            return UUID.fromString(normalized);
        } catch (IllegalArgumentException ignored) {
            return null;
        }
    }

    public static TrChatMessage forwardPrivate(
        String target,
        String sender,
        String receiverComponent,
        String fallback,
        String messageComponent
    ) {
        return TrChatMessage.of(
            "ForwardMessage",
            "SendPrivateRaw",
            target,
            sender,
            receiverComponent,
            fallback,
            messageComponent
        );
    }

    public static TrChatMessage emptyPlayerNames(String serverId) {
        return TrChatMessage.of(
            "UpdateNames",
            serverId,
            "",
            "#",
            formatUuid(NIL_UUID)
        );
    }
}
