package me.arasple.mc.trchat.neoforge.lang;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

class LanguageServiceTest {

    @Test
    void translatesOnlyCompleteEnglishPlaceholderResults() {
        Map<String, String> chinese = Map.of(
            "Placeholder-Translations.yes", "是",
            "Placeholder-Translations.survival", "生存模式"
        );

        assertEquals("是", LanguageService.translatePlaceholderValue("player_online", "yes", chinese, Map.of()));
        assertEquals("生存模式", LanguageService.translatePlaceholderValue("player_gamemode", "SURVIVAL", chinese, Map.of()));
        assertEquals("yes 1", LanguageService.translatePlaceholderValue("player_online", "yes 1", chinese, Map.of()));
        assertEquals("玩家", LanguageService.translatePlaceholderValue("player_online", "玩家", chinese, Map.of()));
        assertEquals("PlayerName", LanguageService.translatePlaceholderValue("player_name", "PlayerName", chinese, Map.of()));
        assertEquals("yes", LanguageService.translatePlaceholderValue("player_name", "yes", chinese, Map.of()));
        assertNull(LanguageService.translatePlaceholderValue("player_online", null, chinese, Map.of()));
    }

    @Test
    void fallsBackWhenSelectedLanguageHasNoTranslation() {
        assertEquals("sí", LanguageService.translatePlaceholderValue(
            "player_online", "yes", Map.of(), Map.of("Placeholder-Translations.yes", "sí")
        ));
    }
}
