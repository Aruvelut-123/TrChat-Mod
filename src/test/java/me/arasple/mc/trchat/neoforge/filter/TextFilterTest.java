package me.arasple.mc.trchat.neoforge.filter;

import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;

class TextFilterTest {

    @Test
    void ignoresConfiguredPunctuationAndCase() {
        TextFilter.Result result = TextFilter.filter(
            "This F.u_C-k is hidden",
            List.of("fuck"),
            Set.of('.', '_', '-'),
            List.of(),
            '*'
        );
        assertEquals("This *.*_*-* is hidden", result.text());
        assertEquals(1, result.matches());
    }

    @Test
    void preservesWhiteListedPhrases() {
        TextFilter.Result result = TextFilter.filter(
            "has been and has",
            List.of("has"),
            Set.of(' '),
            List.of("has been"),
            '*'
        );
        assertEquals("has been and ***", result.text());
    }
}
