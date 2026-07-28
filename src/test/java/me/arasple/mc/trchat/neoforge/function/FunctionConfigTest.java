package me.arasple.mc.trchat.neoforge.function;

import org.junit.jupiter.api.Test;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.InputStream;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

class FunctionConfigTest {

    @Test
    void bundledConfigContainsAllBukkitFunctionsAndExamples() {
        try (InputStream input = getClass().getResourceAsStream("/defaults/function.yml")) {
            assertNotNull(input);
            Map<?, ?> root = new Yaml(new SafeConstructor(new LoaderOptions())).load(input);
            Map<?, ?> general = (Map<?, ?>) root.get("General");
            assertTrue(general.keySet().containsAll(java.util.Set.of(
                "Command-Controller", "Mention", "Mention-All", "Item-Show",
                "Inventory-Show", "EnderChest-Show"
            )));
            Map<?, ?> custom = (Map<?, ?>) root.get("Custom");
            assertTrue(custom.keySet().containsAll(java.util.Set.of(
                "shareUrl", "shareQQ", "shareBilibili", "hidePhoneNumber",
                "hideIDCardNumber", "glowIP", "glowEmail"
            )));
        } catch (java.io.IOException exception) {
            throw new AssertionError(exception);
        }
    }
}
