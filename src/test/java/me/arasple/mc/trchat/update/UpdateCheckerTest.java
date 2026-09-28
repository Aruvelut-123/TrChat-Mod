package me.arasple.mc.trchat.update;

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

    @Test
    void parsesMarkdownHeadingsAndListsForMinecraftRendering() {
        assertEquals(
            new ReleaseNotes.FormattedLine(ReleaseNotes.LineType.LEVEL_TWO_HEADING, "Changes"),
            ReleaseNotes.parseLine("## Changes")
        );
        assertEquals(
            new ReleaseNotes.FormattedLine(ReleaseNotes.LineType.LEVEL_THREE_HEADING, "Fixes"),
            ReleaseNotes.parseLine("### Fixes ###")
        );
        assertEquals(
            new ReleaseNotes.FormattedLine(ReleaseNotes.LineType.LIST_ITEM, "Fixed private chat"),
            ReleaseNotes.parseLine("- Fixed private chat")
        );
    }

    @Test
    void rendersPlainAndBlankLinesWithoutMarkdownMarkers() {
        assertEquals(
            new ReleaseNotes.FormattedLine(ReleaseNotes.LineType.TEXT, "Regular note"),
            ReleaseNotes.parseLine("Regular note")
        );
        assertEquals(
            new ReleaseNotes.FormattedLine(ReleaseNotes.LineType.BLANK, ""),
            ReleaseNotes.parseLine("")
        );
    }
}
