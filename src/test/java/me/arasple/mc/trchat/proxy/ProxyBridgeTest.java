package me.arasple.mc.trchat.proxy;

import me.arasple.mc.trchat.protocol.TrChatMessage;
import me.arasple.mc.trchat.util.proxy.ProxyBridge;
import me.arasple.mc.trchat.util.proxy.ProxyMode;
import me.arasple.mc.trchat.util.proxy.ProxyTransport;
import me.arasple.mc.trchat.util.proxy.common.MessageBuilder;
import me.arasple.mc.trchat.util.proxy.common.MessageReader;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

class ProxyBridgeTest {
    @AfterEach
    void resetTransport() {
        ProxyTransport.setSender(null);
        ProxyTransport.setReceiver(null);
        ProxyTransport.setMode(ProxyMode.VELOCITY);
    }

    @Test
    void preservesActionAndFiltersTheOtherProxyProtocol() {
        List<TrChatMessage> received = new ArrayList<>();
        TrChatMessage expected = TrChatMessage.of("ForwardMessage", "GlobalMute", "on");
        byte[] packet = MessageBuilder.create(UUID.randomUUID(), expected.data().toArray(String[]::new)).get(0);
        try (ProxyBridge bridge = new ProxyBridge(received::add)) {
            bridge.start();
            ProxyTransport.accept(ProxyMode.BUNGEE, packet);
            assertTrue(received.isEmpty());
            ProxyTransport.accept(ProxyMode.VELOCITY, packet);
            assertEquals(List.of(expected), received);
        }
        ProxyTransport.accept(ProxyMode.VELOCITY, packet);
        assertEquals(List.of(expected), received);
    }

    @Test
    void publishesEachFragmentOnceOnTheSelectedProtocolAndReportsNoCarrier() {
        List<byte[]> packets = new ArrayList<>();
        List<ProxyMode> modes = new ArrayList<>();
        ProxyTransport.setMode(ProxyMode.BUNGEE);
        ProxyTransport.setSender(new ProxyTransport.Sender() {
            public boolean isReady() { return true; }
            public boolean send(ProxyMode mode, byte[] packet) {
                modes.add(mode);
                packets.add(packet);
                return true;
            }
        });
        TrChatMessage expected = TrChatMessage.of("BroadcastRaw", "hello".repeat(10_000));
        try (ProxyBridge bridge = new ProxyBridge(ignored -> {})) {
            bridge.start();
            assertTrue(bridge.publish(expected));
            assertTrue(packets.size() > 1);
            assertTrue(modes.stream().allMatch(mode -> mode == ProxyMode.BUNGEE));
            List<TrChatMessage> decoded = new ArrayList<>();
            MessageReader reader = new MessageReader(args -> decoded.add(new TrChatMessage(List.of(args))));
            packets.forEach(reader::read);
            assertEquals(List.of(expected), decoded);
            ProxyTransport.setSender(null);
            assertFalse(bridge.publish(expected));
        }
    }
}
