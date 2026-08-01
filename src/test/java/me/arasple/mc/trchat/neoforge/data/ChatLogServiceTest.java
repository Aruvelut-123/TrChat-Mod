package me.arasple.mc.trchat.neoforge.data;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class ChatLogServiceTest {

    @Test
    void formatsNormalAndPrivateTemplatesByIndex() {
        assertEquals(
            "[12:30:00] Alice: hello",
            ChatLogService.format("[{0}] {1}: {2}", "12:30:00", "Alice", "hello")
        );
        assertEquals(
            "[12:30:00] Alice -> Bob: hello",
            ChatLogService.format(
                "[{0}] {1} -> {2}: {3}", "12:30:00", "Alice", "Bob", "hello"
            )
        );
    }
}
