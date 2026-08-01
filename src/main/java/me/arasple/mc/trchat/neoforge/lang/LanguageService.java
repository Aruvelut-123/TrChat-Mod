package me.arasple.mc.trchat.neoforge.lang;

import me.arasple.mc.trchat.neoforge.chat.LegacyText;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import me.arasple.mc.trchat.neoforge.config.YamlConfigSynchronizer;
import me.arasple.mc.trchat.neoforge.placeholder.PlaceholderCatalog;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerPlayer;
import net.neoforged.fml.loading.FMLPaths;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;
import java.util.stream.Stream;

public final class LanguageService {

    private static final String[] DEFAULTS = {"zh_CN", "en_US", "es_ES"};
    private static final System.Logger LOGGER = System.getLogger(LanguageService.class.getName());
    private static final Pattern ENGLISH_RESULT = Pattern.compile(
        "[A-Za-z]+(?:[ _-][A-Za-z]+)*"
    );

    private final Path directory;
    private volatile Map<String, Map<String, String>> languages = Map.of();

    public LanguageService() {
        directory = FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge").resolve("lang");
    }

    public synchronized boolean reload() {
        try {
            Files.createDirectories(directory);
            Map<String, Map<String, String>> loaded = new HashMap<>();
            for (String language : DEFAULTS) {
                synchronizeLanguage(directory.resolve(language + ".yml"), language);
            }
            try (Stream<Path> files = Files.list(directory)) {
                for (Path file : files.filter(path -> path.getFileName().toString().endsWith(".yml")).toList()) {
                    String name = stem(file);
                    Map<String, Object> repaired = synchronizeLanguage(file, name);
                    loaded.put(name.toLowerCase(Locale.ROOT), flatten(repaired));
                }
            }
            languages = Map.copyOf(loaded);
            return true;
        } catch (IOException | RuntimeException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to load languages from " + directory, exception);
            return false;
        }
    }

    public Component component(ServerPlayer player, String key, Object... arguments) {
        return LegacyText.parse(text(player, key, arguments));
    }

    public String text(ServerPlayer player, String key, Object... arguments) {
        Map<String, String> selected = selected(player);
        Map<String, String> fallback = fallback();
        String value = selected == null ? null : selected.get(key);
        if (value == null) value = fallback.getOrDefault(key, key);
        for (int index = 0; index < arguments.length; index++) {
            value = value.replace("{" + index + "}", String.valueOf(arguments[index]));
        }
        return value;
    }

    public String translatePlaceholder(ServerPlayer player, String token, String value) {
        return translatePlaceholderValue(token, value, selected(player), fallback());
    }

    static String translatePlaceholderValue(
        String token,
        String value,
        Map<String, String> selected,
        Map<String, String> fallback
    ) {
        if (!PlaceholderCatalog.isLocalizable(token)
            || value == null
            || !ENGLISH_RESULT.matcher(value).matches()) {
            return value;
        }
        String key = "Placeholder-Translations." + value.toLowerCase(Locale.ROOT);
        String translated = selected == null ? null : selected.get(key);
        return translated == null ? fallback.getOrDefault(key, value) : translated;
    }

    private Map<String, String> selected(ServerPlayer player) {
        String requested = player == null
            ? TrChatConfig.DEFAULT_LANGUAGE.get()
            : player.clientInformation().language();
        return languages.get(requested.toLowerCase(Locale.ROOT));
    }

    private Map<String, String> fallback() {
        return languages.getOrDefault(
            TrChatConfig.DEFAULT_LANGUAGE.get().toLowerCase(Locale.ROOT),
            languages.getOrDefault("en_us", Map.of())
        );
    }

    private static Map<String, String> flatten(Map<String, Object> root) {
        Map<String, String> output = new HashMap<>();
        root.forEach((key, entry) -> {
            if ("Placeholder-Translations".equals(key) && entry instanceof Map<?, ?> translations) {
                translations.forEach((english, translated) -> output.put(
                    key + '.' + String.valueOf(english).toLowerCase(Locale.ROOT), flattenValue(translated)
                ));
            } else {
                output.put(key, flattenValue(entry));
            }
        });
        return Map.copyOf(output);
    }

    private static String flattenValue(Object value) {
        if (value instanceof String string) return string;
        if (value instanceof java.util.List<?> list) {
            for (Object entry : list) {
                if (entry instanceof String string && !string.isBlank()) return string;
                if (entry instanceof Map<?, ?> map && map.get("text") != null) return String.valueOf(map.get("text"));
            }
        }
        if (value instanceof Map<?, ?> map && map.get("text") != null) return String.valueOf(map.get("text"));
        return value == null ? "" : String.valueOf(value);
    }

    private Map<String, Object> synchronizeLanguage(Path file, String requested) throws IOException {
        String bundled = java.util.Arrays.stream(DEFAULTS)
            .filter(value -> value.equalsIgnoreCase(requested))
            .findFirst()
            .orElse("en_US");
        return YamlConfigSynchronizer.synchronize(
            file, "/defaults/lang/" + bundled + ".yml", Set.of()
        );
    }

    private static String stem(Path file) {
        String name = file.getFileName().toString();
        return name.substring(0, name.length() - 4);
    }
}
