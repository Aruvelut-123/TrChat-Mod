package me.arasple.mc.trchat.chat;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class MessageGuardTest {

    @Test
    void filtersCaseInsensitively() {
        assertEquals("hello *** world", MessageGuard.filter("hello BAD world", List.of("bad"), "*"));
    }

    @Test
    void measuresNormalizedSimilarity() {
        assertTrue(MessageGuard.similarity("Hello world", "hello  world!") > 0.9D);
    }
}
