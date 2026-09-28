package me.arasple.mc.trchat.update;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class SemanticVersionTest {

    @Test
    void comparesFourPartReleaseVersions() {
        assertTrue(SemanticVersion.parse("2.4.9.4").compareTo(SemanticVersion.parse("2.4.9.3")) > 0);
        assertEquals(0, SemanticVersion.parse("v2.4.9.4").compareTo(SemanticVersion.parse("2.4.9.4")));
    }

    @Test
    void stableReleaseIsNewerThanPreRelease() {
        assertTrue(SemanticVersion.parse("2.4.9.4").compareTo(SemanticVersion.parse("2.4.9.4-rc.1")) > 0);
        assertTrue(SemanticVersion.parse("2.4.9.4-rc.2").compareTo(SemanticVersion.parse("2.4.9.4-rc.1")) > 0);
    }
}
