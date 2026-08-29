package me.arasple.mc.trchat.config;

import me.arasple.mc.trchat.platform.Platform;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.IOException;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Set;
import java.util.stream.Collectors;

/**
 * Loads the bundled/configured special characters and wraps them with a
 * dedicated color so resource-pack emoji do not lose their colors. Mirrors
 * the Bukkit TrChat special-chars.yml feature.
 */
public final class SpecialChars {

    private static final System.Logger LOGGER = System.getLogger(SpecialChars.class.getName());
    private static final char COLOR_CHAR = '&';

    private static volatile Set<String> chars = Set.of();

    private SpecialChars() {
    }

    public static void reload() {
        reload(Platform.configDir().resolve("trchat"));
    }

    public static void reload(Path folder) {
        try {
            Files.createDirectories(folder);
            Path file = folder.resolve("special-chars.yml");
            if (!Files.exists(file)) {
                try (var input = SpecialChars.class.getResourceAsStream("/defaults/special-chars.yml")) {
                    if (input == null) {
                        throw new IOException("Missing bundled special-chars.yml");
                    }
                    Files.copy(input, file);
                }
            }
            LoaderOptions options = new LoaderOptions();
            options.setAllowDuplicateKeys(false);
            options.setCodePointLimit(4 * 1024 * 1024);
            Set<String> loaded = Set.of();
            try (Reader reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
                Object root = new Yaml(new SafeConstructor(options)).load(reader);
                if (root instanceof java.util.Map<?, ?> map && map.get("SpecialChars") instanceof java.util.List<?> list) {
                    loaded = list.stream()
                        .map(String::valueOf)
                        .filter(value -> !value.isBlank())
                        .collect(Collectors.toUnmodifiableSet());
                }
            }
            chars = loaded;
            LOGGER.log(System.Logger.Level.INFO, "Loaded {0} special characters from {1}", loaded.size(), file);
        } catch (IOException | RuntimeException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to load special-chars.yml", exception);
            chars = Set.of();
        }
    }

    public static Set<String> chars() {
        return chars;
    }

    public static boolean hasSpecialChars(String text) {
        Set<String> special = chars;
        if (special.isEmpty() || text == null || text.isEmpty()) {
            return false;
        }
        for (int index = 0; index < text.length();) {
            int codePoint = text.codePointAt(index);
            if (special.contains(new String(Character.toChars(codePoint)))) {
                return true;
            }
            index += Character.charCount(codePoint);
        }
        return false;
    }

    /**
     * Wraps configured special characters with {@code prefix} and restores
     * {@code suffix} after each run, while keeping any manual color codes.
     */
    public static String wrapSpecialChars(String text, String prefix, String suffix) {
        Set<String> special = chars;
        if (text == null || prefix == null || prefix.isBlank() || special.isEmpty()) {
            return text;
        }
        StringBuilder output = new StringBuilder(text.length());
        boolean inSpecial = false;
        boolean manualColor = false;
        boolean hasManualColor = false;
        int index = 0;
        while (index < text.length()) {
            int codePoint = text.codePointAt(index);
            String character = new String(Character.toChars(codePoint));
            boolean isSpecial = special.contains(character);
            boolean isExtension = codePoint == 0x200D
                || codePoint >= 0x1F3FB && codePoint <= 0x1F3FF
                || codePoint == 0xFE0F;

            if (!isSpecial && !isExtension && codePoint == COLOR_CHAR) {
                char next = index + 1 < text.length() ? text.charAt(index + 1) : ' ';
                boolean isDefaultPrefix = index == 0 && text.startsWith(suffix);
                if (!isDefaultPrefix) {
                    hasManualColor = !(next == 'r' || next == 'R');
                }
                output.append(character);
                if (index + 1 < text.length()) {
                    output.append(text.charAt(index + 1));
                }
                index += index + 1 < text.length() ? 2 : 1;
                continue;
            }

            if (isSpecial) {
                if (!inSpecial) {
                    manualColor = hasManualColor;
                    inSpecial = true;
                    if (!manualColor) {
                        output.append(prefix);
                    }
                }
                output.append(character);
            } else if (isExtension && inSpecial) {
                output.append(character);
            } else if (inSpecial) {
                if (!manualColor) {
                    output.append(suffix);
                }
                inSpecial = false;
                output.append(character);
            } else {
                output.append(character);
            }
            index += Character.charCount(codePoint);
        }
        if (inSpecial && !manualColor) {
            output.append(suffix);
        }
        return output.toString();
    }
}
