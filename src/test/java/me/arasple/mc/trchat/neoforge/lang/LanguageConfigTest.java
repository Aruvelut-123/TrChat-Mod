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

    private Map<?, ?> load(String language) throws Exception {
        try (InputStream input = getClass().getResourceAsStream("/defaults/lang/" + language + ".yml")) {
            assertNotNull(input);
            return new Yaml(new SafeConstructor(new LoaderOptions())).load(input);
        }
    }
}
