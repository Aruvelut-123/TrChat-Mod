package me.arasple.mc.trchat.neoforge.lang;

import me.arasple.mc.trchat.neoforge.chat.LegacyText;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerPlayer;
import net.neoforged.fml.loading.FMLPaths;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.IOException;
import java.io.InputStream;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.stream.Stream;

public final class LanguageService {

    private static final String[] DEFAULTS = {"zh_CN", "en_US", "es_ES"};
    private static final System.Logger LOGGER = System.getLogger(LanguageService.class.getName());

    private final Path directory;
    private volatile Map<String, Map<String, String>> languages = Map.of();

    public LanguageService() {
        directory = FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge").resolve("lang");
    }

    public synchronized boolean reload() {
        try {
            Files.createDirectories(directory);
            for (String language : DEFAULTS) copyDefault(language);
            Map<String, Map<String, String>> loaded = new HashMap<>();
            try (Stream<Path> files = Files.list(directory)) {
                for (Path file : files.filter(path -> path.getFileName().toString().endsWith(".yml")).toList()) {
                    loaded.put(stem(file).toLowerCase(Locale.ROOT), load(file));
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
        String requested = player == null
            ? TrChatConfig.DEFAULT_LANGUAGE.get()
            : player.clientInformation().language();
        Map<String, String> selected = languages.get(requested.toLowerCase(Locale.ROOT));
        Map<String, String> fallback = languages.getOrDefault(
            TrChatConfig.DEFAULT_LANGUAGE.get().toLowerCase(Locale.ROOT),
            languages.getOrDefault("en_us", Map.of())
        );
        String value = selected == null ? null : selected.get(key);
        if (value == null) value = fallback.getOrDefault(key, key);
        for (int index = 0; index < arguments.length; index++) {
            value = value.replace("{" + index + "}", String.valueOf(arguments[index]));
        }
        return value;
    }

    private Map<String, String> load(Path file) throws IOException {
        Yaml yaml = new Yaml(new SafeConstructor(new LoaderOptions()));
        try (Reader reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
            Object value = yaml.load(reader);
            if (!(value instanceof Map<?, ?> root)) throw new IOException("Language root must be a mapping: " + file);
            Map<String, String> output = new HashMap<>();
            root.forEach((key, entry) -> output.put(String.valueOf(key), flatten(entry)));
            return Map.copyOf(output);
        }
    }

    private static String flatten(Object value) {
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

    private void copyDefault(String language) throws IOException {
        Path target = directory.resolve(language + ".yml");
        if (Files.exists(target)) return;
        try (InputStream input = LanguageService.class.getResourceAsStream("/defaults/lang/" + language + ".yml")) {
            if (input == null) throw new IOException("Missing bundled language " + language);
            Files.copy(input, target);
        }
    }

    private static String stem(Path file) {
        String name = file.getFileName().toString();
        return name.substring(0, name.length() - 4);
    }
}
