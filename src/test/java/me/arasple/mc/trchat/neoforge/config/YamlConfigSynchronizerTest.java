package me.arasple.mc.trchat.neoforge.config;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

class YamlConfigSynchronizerTest {

    @TempDir
    Path directory;

    @Test
    void removesUnknownKeysAddsDefaultsAndPreservesKnownValues() throws Exception {
        Path file = directory.resolve("datasource.yml");
        Files.writeString(file, """
            Type: MySQL
            SQLite:
              Unknown: remove-me
            Unknown-Root: remove-me
            """, StandardCharsets.UTF_8);

        Map<String, Object> result = YamlConfigSynchronizer.synchronize(
            file, "/defaults/datasource.yml", Set.of()
        );

        assertEquals("MySQL", result.get("Type"));
        assertFalse(result.containsKey("Unknown-Root"));
        Map<?, ?> sqlite = (Map<?, ?>) result.get("SQLite");
        assertEquals("data.db", sqlite.get("File"));
        assertFalse(sqlite.containsKey("Unknown"));
        assertTrue(result.containsKey("MariaDB"));
    }

    @Test
    void repairsLanguageMappingsAndPreservesRichKnownValues() throws Exception {
        Path file = directory.resolve("language.yml");
        Files.writeString(file, """
            General-Muted:
              text: '&c保留富文本形式'
            Placeholder-Translations:
              'yes': 自定义“是”
              Custom English Value: 应删除
            Unknown-Message: 应删除
            """, StandardCharsets.UTF_8);

        Map<String, Object> result = YamlConfigSynchronizer.synchronize(
            file, "/defaults/lang/zh_CN.yml", Set.of()
        );

        Map<?, ?> translations = (Map<?, ?>) result.get("Placeholder-Translations");
        assertEquals("自定义“是”", translations.get("yes"));
        assertFalse(translations.containsKey("Custom English Value"));
        assertFalse(result.containsKey("Unknown-Message"));
        assertTrue(result.containsKey("General-Muted"));
        assertTrue(result.get("General-Muted") instanceof Map<?, ?>);
    }

    @Test
    void retainsEntriesOnlyInExplicitlyOpenConfigurationSections() throws Exception {
        Path file = directory.resolve("function.yml");
        Files.writeString(file, """
            Custom:
              serverSpecificRule:
                arbitrary-option: keep-me
            """, StandardCharsets.UTF_8);

        Map<String, Object> result = YamlConfigSynchronizer.synchronize(
            file, "/defaults/function.yml", Set.of("Custom")
        );

        Map<?, ?> custom = (Map<?, ?>) result.get("Custom");
        assertTrue(custom.containsKey("serverSpecificRule"));
    }
}
