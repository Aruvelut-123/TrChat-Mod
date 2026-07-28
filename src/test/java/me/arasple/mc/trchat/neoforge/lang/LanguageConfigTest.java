package me.arasple.mc.trchat.neoforge.lang;

import org.junit.jupiter.api.Test;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.InputStream;
import java.util.Map;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

class LanguageConfigTest {

    @Test
    void bundledLanguagesHaveTheSameRequiredKeys() throws Exception {
        Map<?, ?> chinese = load("zh_CN");
        Set<?> expected = chinese.keySet();
        assertEquals(expected, load("en_US").keySet());
        assertEquals(expected, load("es_ES").keySet());
        assertTrue(expected.containsAll(Set.of(
            "General-Muted",
            "Private-Message-Spy-Format",
            "Mute-Shadow-On",
            "Function-Inventory-Format",
            "Filter-Anvil-Blocked",
            "Redis-Private-Unavailable"
        )));
    }

    @Test
    void statusAndAboutPagesFitTheDefaultChatWidth() throws Exception {
        for (String language : Set.of("zh_CN", "en_US", "es_ES")) {
            Map<?, ?> values = load(language);
            assertLinesFit(language, "Status-Overview", String.valueOf(values.get("Status-Overview")));
            assertLinesFit(language, "Command-About", String.valueOf(values.get("Command-About")));
            assertLinesFit(language, "Command-Help", String.valueOf(values.get("Command-Help")));
            assertLinesFit(language, "Player-Status-Overview", String.valueOf(values.get("Player-Status-Overview")));
            assertLinesFit(language, "Updater-Available", String.valueOf(values.get("Updater-Available")));
            assertLinesFit(language, "Status-Footer", String.valueOf(values.get("Status-Footer")));
            assertLinesFit(
                language,
                "creator link",
                String.valueOf(values.get("Status-Creator-Prefix"))
                    + values.get("Status-Creator-Link")
            );
            assertLinesFit(
                language,
                "repository link",
                String.valueOf(values.get("Status-Repository-Prefix"))
                    + values.get("Status-Repository-Link")
            );
        }
    }

    private static void assertLinesFit(String language, String key, String value) {
        for (String line : value.split("\\R")) {
            String visible = line
                .replaceAll("(?i)[&§][0-9A-FK-OR]", "")
                .replaceAll("\\{\\d+}", "Global");
            int width = visible.codePoints().map(character -> character > 0x7F ? 2 : 1).sum();
            assertTrue(
                width <= 42,
                () -> language + ' ' + key + " is too wide (" + width + "): " + visible
            );
        }
    }

    private Map<?, ?> load(String language) throws Exception {
        try (InputStream input = getClass().getResourceAsStream("/defaults/lang/" + language + ".yml")) {
            assertNotNull(input);
            return new Yaml(new SafeConstructor(new LoaderOptions())).load(input);
        }
    }
}
