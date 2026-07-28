package me.arasple.mc.trchat.neoforge.protocol;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

class RedisEnvelopeCodecTest {

    @Test
    void encodesBukkitCompatibleEnvelope() {
        TrChatMessage message = TrChatMessage.of("UpdateNames", "25565", "Alice,Bob");
        assertEquals(
            "{\"data\":[\"UpdateNames\",\"25565\",\"Alice,Bob\"]}",
            RedisEnvelopeCodec.encode(message)
        );
    }

    @Test
    void preservesJsonAndUnicodeInRawComponents() {
        TrChatMessage message = TrChatMessage.of(
            "BroadcastRaw",
            "{\"text\":\"你好\\nworld\"}",
            "§b你好"
        );
        assertEquals(message, RedisEnvelopeCodec.decode(RedisEnvelopeCodec.encode(message)));
    }

    @Test
    void acceptsScalarProducedByBukkitArrayConverter() {
        assertEquals(
            List.of("GlobalMute"),
            RedisEnvelopeCodec.decode("{\"data\":\"GlobalMute\"}").data()
        );
    }

    @Test
    void rejectsMissingData() {
        assertThrows(IllegalArgumentException.class, () -> RedisEnvelopeCodec.decode("{\"other\":[]}"));
    }
}
