package me.arasple.mc.trchat.neoforge.channel;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.nio.file.Path;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

class ChannelManagerTest {

    @TempDir
    Path directory;

    @Test
    void createsAndLoadsBukkitStyleDefaults() {
        ChannelManager manager = new ChannelManager(directory.resolve("channels"));

        assertEquals(5, manager.reload());
        assertEquals(5, manager.all().size());
        assertTrue(manager.byCommand("ALL").isPresent());
        assertEquals("Global", manager.byPrefix("!all hello").channel().id());
        assertTrue(manager.byId("private").orElseThrow().options().privateChannel());
        assertTrue(manager.byId("server").isPresent());
        assertFalse(manager.byId("server").orElseThrow().options().redis());
        assertFalse(manager.normal().formats().isEmpty());
        assertNotNull(manager.normal().formats().getFirst().message());
        assertTrue(java.nio.file.Files.exists(directory.resolve("channels").resolve("Example.yml")));
        assertFalse(manager.byId("Example").isPresent());
        assertTrue(manager.byCommand("examplechat").isEmpty());
    }
}
