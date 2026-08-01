package me.arasple.mc.trchat.neoforge.update;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;

class UpdateCheckerTest {

    @Test
    void normalizesReleaseNotesForChatDisplay() {
        assertEquals(
            List.of("## Changes", "", "- Fixed private chat"),
            ReleaseNotes.normalize(
                "\r\n## Changes  \r\n\r\n- Fixed private chat\r\n"
            )
        );
    }

    @Test
    void treatsMissingReleaseNotesAsEmpty() {
        assertEquals(List.of(), ReleaseNotes.normalize(null));
        assertEquals(List.of(), ReleaseNotes.normalize("  \r\n"));
    }
}
