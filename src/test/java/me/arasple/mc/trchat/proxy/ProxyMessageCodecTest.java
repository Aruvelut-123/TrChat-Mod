package me.arasple.mc.trchat.proxy;

import me.arasple.mc.trchat.util.proxy.common.MessageBuilder;
import me.arasple.mc.trchat.util.proxy.common.MessageReader;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicLong;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class ProxyMessageCodecTest {

    @Test
    void roundTripsEscapedArgumentsAndOutOfOrderChunks() {
        UUID uid = UUID.randomUUID();
        List<String> expected = List.of("BroadcastRaw", "line\nquote\"slash\\", "你好");
        List<String> actual = new ArrayList<>();
        MessageReader reader = new MessageReader(values -> actual.addAll(List.of(values)));

        List<byte[]> packets = MessageBuilder.create(uid, expected.toArray(String[]::new));
        for (int index = packets.size() - 1; index >= 0; index--) {
            reader.read(packets.get(index));
        }

        assertEquals(expected, actual);
    }

    @Test
    void doesNotCompleteWhenAChunkIsMissing() {
        List<String> actual = new ArrayList<>();
        MessageReader reader = new MessageReader(values -> actual.addAll(List.of(values)));
        StringBuilder large = new StringBuilder();
        for (int index = 0; index < 24000; index++) {
            large.append("长文本");
        }
        List<byte[]> packets = MessageBuilder.create(UUID.randomUUID(), "BroadcastRaw", large.toString());

        assertTrue(packets.size() >= 2);
        reader.read(packets.get(0));
        assertEquals(List.of(), actual);
    }

    @Test
    void readsTheUpstreamSinglePacketVector() {
        List<String> actual = new ArrayList<>();
        MessageReader reader = new MessageReader(values -> actual.addAll(List.of(values)));
        String packet = "{\"uid\":\"12345678-1234-1234-1234-123456789abc\",\"index\":1,\"total\":1,\"data\":\"WyJCcm9hZGNhc3RSYXciLCJoZWxsbyJd\"}";

        reader.read(packet.getBytes(java.nio.charset.StandardCharsets.UTF_8));

        assertEquals(List.of("BroadcastRaw", "hello"), actual);
    }

    @Test
    void preservesJsonControlCharacters() {
        UUID uid = UUID.randomUUID();
        List<String> expected = List.of("line\n\r\t\b\f\"\\", "unicode: \u0000 \u001f", "你好");
        List<String> actual = new ArrayList<>();
        MessageReader reader = new MessageReader(values -> actual.addAll(List.of(values)));

        for (byte[] packet : MessageBuilder.create(uid, expected.toArray(String[]::new))) {
            reader.read(packet);
        }

        assertEquals(expected, actual);
    }

    @Test
    void ignoresDuplicateChunksAndRejectsOversizedTotals() {
        List<String> actual = new ArrayList<>();
        MessageReader reader = new MessageReader(values -> actual.addAll(List.of(values)));
        UUID uid = UUID.randomUUID();
        List<byte[]> packets = MessageBuilder.create(uid, "BroadcastRaw", "x".repeat(120_000));
        assertTrue(packets.size() > 1);

        reader.read(packets.get(0));
        reader.read(packets.get(0));
        for (int index = packets.size() - 1; index > 0; index--) {
            reader.read(packets.get(index));
        }
        assertEquals(List.of("BroadcastRaw", "x".repeat(120_000)), actual);

        String oversized = "{\"uid\":\"" + UUID.randomUUID()
            + "\",\"index\":1,\"total\":129,\"data\":\"x\"}";
        reader.read(oversized.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        assertEquals(List.of("BroadcastRaw", "x".repeat(120_000)), actual);
    }

    @Test
    void expiresIncompleteMessagesUsingInjectedClock() {
        AtomicLong clock = new AtomicLong(0L);
        List<String> actual = new ArrayList<>();
        MessageReader reader = new MessageReader(values -> actual.addAll(List.of(values)), clock::get);
        List<byte[]> packets = MessageBuilder.create(UUID.randomUUID(), "BroadcastRaw", "x".repeat(90_000));
        assertTrue(packets.size() > 1);

        reader.read(packets.get(0));
        clock.set(10_000_000_001L);
        reader.read(MessageBuilder.create(UUID.randomUUID(), "BroadcastRaw", "fresh").get(0));
        for (int index = 1; index < packets.size(); index++) {
            reader.read(packets.get(index));
        }

        assertEquals(List.of("BroadcastRaw", "fresh"), actual);
    }
}
