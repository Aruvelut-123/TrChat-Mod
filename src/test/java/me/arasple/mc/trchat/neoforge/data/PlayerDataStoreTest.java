package me.arasple.mc.trchat.neoforge.data;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.nio.file.Path;
import java.util.Set;
import java.util.UUID;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class PlayerDataStoreTest {

    @TempDir
    Path directory;

    @Test
    void persistsActiveAndJoinedChannels() {
        UUID uuid = UUID.randomUUID();
        try (PlayerDataStore store = new PlayerDataStore(directory)) {
            store.initialize();
            store.save(PlayerDataStore.PlayerState.empty(uuid, "TestPlayer")
                .withChannels("Global", Set.of("Normal", "Global")));
        }

        try (PlayerDataStore store = new PlayerDataStore(directory)) {
            store.initialize();
            PlayerDataStore.PlayerState loaded = store.load(uuid, "RenamedPlayer");

            assertEquals("global", loaded.activeChannel());
            assertEquals(Set.of("normal", "global"), loaded.joinedChannels());
            assertEquals("RenamedPlayer", loaded.playerName());
        }
    }

    @Test
    void persistsMuteStatusReasonAndModerationFlags() {
        UUID uuid = UUID.randomUUID();
        long muteUntil = System.currentTimeMillis() + 3_600_000L;
        try (PlayerDataStore store = new PlayerDataStore(directory)) {
            store.initialize();
            store.save(PlayerDataStore.PlayerState.empty(uuid, "MutedPlayer")
                .withMute(muteUntil, "Repeated spam")
                .withShadowMuted(true)
                .withPrivateSpy(true));
        }

        try (PlayerDataStore store = new PlayerDataStore(directory)) {
            store.initialize();
            PlayerDataStore.PlayerState loaded = store.load(uuid, "MutedPlayer");

            assertEquals(muteUntil, loaded.muteUntil());
            assertEquals("Repeated spam", loaded.muteReason());
            assertTrue(loaded.shadowMuted());
            assertTrue(loaded.privateSpy());
        }
    }
}
