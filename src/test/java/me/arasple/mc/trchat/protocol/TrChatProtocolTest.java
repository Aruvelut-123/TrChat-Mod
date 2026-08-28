package me.arasple.mc.trchat.protocol;

import org.junit.jupiter.api.Test;

import java.util.UUID;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

class TrChatProtocolTest {

    private static final UUID UUID_VALUE = UUID.fromString("12345678-1234-5678-90ab-cdef12345678");

    @Test
    void usesBukkitFastUuidWireFormat() {
        assertEquals("123456781234567890abcdef12345678", TrChatProtocol.formatUuid(UUID_VALUE));
        assertEquals("00000000000000000000000000000000", TrChatProtocol.formatUuid(TrChatProtocol.NIL_UUID));
    }

    @Test
    void permitsOnlyVanillaDisplayedItemsAcrossServers() {
        org.junit.jupiter.api.Assertions.assertTrue(
            TrChatProtocol.isCrossServerSafeItemNamespace("minecraft")
        );
        org.junit.jupiter.api.Assertions.assertFalse(
            TrChatProtocol.isCrossServerSafeItemNamespace("irons_spellbooks")
        );
        org.junit.jupiter.api.Assertions.assertFalse(
            TrChatProtocol.isCrossServerSafeItemNamespace("neoforge")
        );
        org.junit.jupiter.api.Assertions.assertFalse(
            TrChatProtocol.isCrossServerSafeItemNamespace(null)
        );
    }

    @Test
    void acceptsBukkitAndJavaUuidFormatsButRejectsEmptyValues() {
        assertEquals(UUID_VALUE, TrChatProtocol.parseUuid("123456781234567890abcdef12345678"));
        assertEquals(UUID_VALUE, TrChatProtocol.parseUuid(UUID_VALUE.toString()));
        assertNull(TrChatProtocol.parseUuid(""));
        assertNull(TrChatProtocol.parseUuid(null));
    }

    @Test
    void buildsBukkit243CompatiblePrivateAndEmptyPlayerPackets() {
        assertEquals(List.of(
            "ForwardMessage", "SendPrivateRaw", "Alex", "Steve",
            "receiver-json", "fallback", "message-json"
        ), TrChatProtocol.forwardPrivate(
            "Alex", "Steve", "receiver-json", "fallback", "message-json"
        ).data());
        assertEquals(List.of(
            "UpdateNames", "25566", "", "#", "00000000000000000000000000000000"
        ), TrChatProtocol.emptyPlayerNames("25566").data());
    }
}
