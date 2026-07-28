package me.arasple.mc.trchat.neoforge.moderation;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class ModerationServiceTest {

    @Test
    void parsesSingleAndCompoundDurations() {
        assertEquals(30_000L, ModerationService.parseDuration("30s").orElseThrow());
        assertEquals(5_400_000L, ModerationService.parseDuration("1h30m").orElseThrow());
        assertEquals(691_200_000L, ModerationService.parseDuration("1w1d").orElseThrow());
    }

    @Test
    void parsesPermanentAndRejectsInvalidDurations() {
        assertEquals(-1L, ModerationService.parseDuration("永久").orElseThrow());
        assertEquals(-1L, ModerationService.parseDuration("permanent").orElseThrow());
        assertTrue(ModerationService.parseDuration("10").isEmpty());
        assertTrue(ModerationService.parseDuration("1h-nope").isEmpty());
        assertTrue(ModerationService.parseDuration("0s").isEmpty());
    }
}
