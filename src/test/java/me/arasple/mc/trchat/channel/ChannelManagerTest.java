package me.arasple.mc.trchat.channel;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
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

        assertEquals(4, manager.reload());
        assertEquals(4, manager.all().size());
        assertTrue(manager.byCommand("ALL").isPresent());
        assertEquals("Global", manager.byPrefix("!all hello").channel().id());
        assertTrue(manager.byId("private").orElseThrow().options().privateChannel());
        assertEquals("Private", manager.byCommand("tell").orElseThrow().id());
        assertFalse(manager.byId("private").orElseThrow().isJoinable());
        assertTrue(manager.byId("server").isEmpty());
        assertEquals("Normal", manager.autoJoin().orElseThrow().id());
        assertFalse(manager.normal().formats().isEmpty());
        assertNotNull(manager.normal().formats().getFirst().message());
        assertTrue(java.nio.file.Files.exists(directory.resolve("channels").resolve("Example.yml")));
        assertFalse(manager.byId("Example").isPresent());
        assertTrue(manager.byCommand("examplechat").isEmpty());
        assertTrue(manager.normal().isJoinable());
    }

    @Test
    void rejectsMultipleAutoJoinChannelsAtomically() throws Exception {
        Path channels = directory.resolve("channels");
        ChannelManager manager = new ChannelManager(channels);
        assertEquals(4, manager.reload());

        Files.writeString(
            channels.resolve("Other.yml"),
            "Options:\n  Auto-Join: true\nFormats: []\n",
            StandardCharsets.UTF_8
        );

        assertEquals(-1, manager.reload());
        assertEquals(4, manager.all().size());
        assertTrue(manager.byId("Other").isEmpty());
        assertEquals("Normal", manager.autoJoin().orElseThrow().id());
    }

    @Test
    void movesAutoJoinFromNormalToGlobalWithoutDeletingTheOption() throws Exception {
        Path channels = directory.resolve("channels");
        ChannelManager manager = new ChannelManager(channels);
        assertEquals(4, manager.reload());

        Path normal = channels.resolve("Normal.yml");
        Files.writeString(
            normal,
            Files.readString(normal, StandardCharsets.UTF_8)
                .replace("Auto-Join: true", "Auto-Join: false"),
            StandardCharsets.UTF_8
        );
        Path global = channels.resolve("Global.yml");
        Files.writeString(
            global,
            Files.readString(global, StandardCharsets.UTF_8)
                .replace("Always-Listen: true", "Always-Listen: true\n  Auto-Join: true"),
            StandardCharsets.UTF_8
        );

        assertEquals(4, manager.reload());
        assertEquals("Global", manager.autoJoin().orElseThrow().id());
        assertTrue(Files.readString(global, StandardCharsets.UTF_8).contains("Auto-Join: true"));
    }

    @Test
    void ignoresLegacyServerChannelWithoutDeletingIt() throws Exception {
        Path channels = directory.resolve("channels");
        Files.createDirectories(channels);
        Path legacy = channels.resolve("Server.yml");
        String contents = "Options:\n  Always-Listen: true\nConsole: []\n";
        Files.writeString(legacy, contents, StandardCharsets.UTF_8);

        ChannelManager manager = new ChannelManager(channels);

        assertEquals(4, manager.reload());
        assertTrue(manager.byId("Server").isEmpty());
        assertEquals(contents, Files.readString(legacy, StandardCharsets.UTF_8));
    }
}
