package me.arasple.mc.trchat.neoforge.lang;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

class LanguageServiceTest {

    @TempDir
    Path directory;

    @Test
    void translatesOnlyCompleteEnglishPlaceholderResults() {
        Map<String, String> chinese = Map.of(
            "Placeholder-Translations.yes", "是",
            "Placeholder-Translations.survival", "生存模式"
        );

        assertEquals("是", LanguageService.translatePlaceholderValue("player_online", "yes", chinese, Map.of()));
        assertEquals("生存模式", LanguageService.translatePlaceholderValue("player_gamemode", "SURVIVAL", chinese, Map.of()));
        assertEquals("是", LanguageService.translatePlaceholderValue(" PLAYER_ONLINE ", " yes ", chinese, Map.of()));
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

    @Test
    void loadsPlaceholderTranslationsFromEveryBundledLanguage() throws Exception {
        LanguageService service = new LanguageService(directory, "en_US");
        assertTrue(service.reload());

        assertEquals("是", service.translatePlaceholderForLanguage("zh_cn", "player_online", "yes"));
        assertEquals("生存模式", service.translatePlaceholderForLanguage("zh-CN", "player_gamemode", "SURVIVAL"));
        assertEquals("sí", service.translatePlaceholderForLanguage("es_es", "player_online", "yes"));
        assertEquals("SUPERVIVENCIA", service.translatePlaceholderForLanguage("es-ES", "player_gamemode", "SURVIVAL"));
        assertEquals("yes", service.translatePlaceholderForLanguage("en_us", "player_online", "yes"));
    }

    @Test
    void reloadsCustomizedPlaceholderTranslationsFromLanguageFile() throws Exception {
        LanguageService service = new LanguageService(directory, "zh_CN");
        assertTrue(service.reload());
        Path chinese = directory.resolve("zh_CN.yml");
        String configured = Files.readString(chinese, StandardCharsets.UTF_8)
            .replace("'yes': '是'", "'yes': '自定义是'");
        Files.writeString(chinese, configured, StandardCharsets.UTF_8);

        assertTrue(service.reload());
        assertEquals("自定义是", service.translatePlaceholderForLanguage("zh_cn", "player_online", "yes"));
    }
}
