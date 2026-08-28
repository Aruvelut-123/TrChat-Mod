package me.arasple.mc.trchat.config;

import org.yaml.snakeyaml.DumperOptions;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Reconciles YAML mappings against a bundled default while retaining all known
 * user values. Lists are treated as configurable values rather than schemas.
 */
public final class YamlConfigSynchronizer {

    private static final System.Logger LOGGER = System.getLogger(YamlConfigSynchronizer.class.getName());

    private YamlConfigSynchronizer() {
    }

    public static Map<String, Object> synchronize(
        Path file,
        String defaultResource,
        Set<String> openMapPaths
    ) throws IOException {
        return synchronize(file, defaultResource, defaultResource, openMapPaths);
    }

    /**
     * Reconciles against channel-specific defaults while accepting optional
     * keys declared by a broader schema. Schema-only keys are retained when
     * configured, but are not added when absent.
     */
    public static Map<String, Object> synchronize(
        Path file,
        String defaultResource,
        String schemaResource,
        Set<String> openMapPaths
    ) throws IOException {
        Map<String, Object> defaults = loadResource(defaultResource);
        Map<String, Object> schema = defaultResource.equals(schemaResource)
            ? defaults
            : loadResource(schemaResource);
        Files.createDirectories(file.toAbsolutePath().getParent());
        if (!Files.exists(file)) {
            try (InputStream input = YamlConfigSynchronizer.class.getResourceAsStream(defaultResource)) {
                if (input == null) {
                    throw new IOException("Missing bundled YAML " + defaultResource);
                }
                Files.copy(input, file);
            }
        }
        Map<String, Object> current = loadFile(file);
        Map<String, Object> reconciled = reconcileMap(defaults, schema, current, "", openMapPaths);
        if (!reconciled.equals(current)) {
            write(file, reconciled);
            LOGGER.log(System.Logger.Level.INFO, "Repaired YAML configuration {0}", file);
        }
        return reconciled;
    }

    private static Map<String, Object> loadResource(String resource) throws IOException {
        try (InputStream input = YamlConfigSynchronizer.class.getResourceAsStream(resource)) {
            if (input == null) {
                throw new IOException("Missing bundled YAML " + resource);
            }
            return load(new InputStreamReader(input, StandardCharsets.UTF_8), resource);
        }
    }

    private static Map<String, Object> loadFile(Path file) throws IOException {
        try (Reader reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
            return load(reader, file.toString());
        }
    }

    private static Map<String, Object> load(Reader reader, String source) throws IOException {
        LoaderOptions options = new LoaderOptions();
        options.setAllowDuplicateKeys(false);
        options.setCodePointLimit(4 * 1024 * 1024);
        Object value = new Yaml(new SafeConstructor(options)).load(reader);
        if (!(value instanceof Map<?, ?> map)) {
            throw new IOException("YAML root must be a mapping: " + source);
        }
        return stringMap(map);
    }

    private static Map<String, Object> reconcileMap(
        Map<String, Object> defaults,
        Map<String, Object> schema,
        Map<String, Object> current,
        String path,
        Set<String> openMapPaths
    ) {
        LinkedHashMap<String, Object> output = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : defaults.entrySet()) {
            String childPath = path.isEmpty() ? entry.getKey() : path + '.' + entry.getKey();
            Object value = current.containsKey(entry.getKey())
                ? reconcile(
                    entry.getValue(),
                    schema.getOrDefault(entry.getKey(), entry.getValue()),
                    current.get(entry.getKey()),
                    childPath,
                    openMapPaths
                )
                : copy(entry.getValue());
            output.put(entry.getKey(), value);
        }

        for (Map.Entry<String, Object> entry : current.entrySet()) {
            if (output.containsKey(entry.getKey())) {
                continue;
            }
            String childPath = path.isEmpty() ? entry.getKey() : path + '.' + entry.getKey();
            if (schema.containsKey(entry.getKey())) {
                Object schemaValue = schema.get(entry.getKey());
                output.put(entry.getKey(), reconcile(
                    schemaValue, schemaValue, entry.getValue(), childPath, openMapPaths
                ));
            } else if (openMapPaths.contains(path)) {
                output.put(entry.getKey(), copy(entry.getValue()));
            }
        }
        return output;
    }

    private static Object reconcile(
        Object defaultValue,
        Object schemaValue,
        Object currentValue,
        String path,
        Set<String> openMapPaths
    ) {
        if (defaultValue instanceof Map<?, ?> defaultMap) {
            if (!(currentValue instanceof Map<?, ?> currentMap)) {
                return copy(defaultValue);
            }
            Map<String, Object> allowed = schemaValue instanceof Map<?, ?> schemaMap
                ? stringMap(schemaMap)
                : stringMap(defaultMap);
            return reconcileMap(
                stringMap(defaultMap), allowed, stringMap(currentMap), path, openMapPaths
            );
        }
        if (defaultValue instanceof List<?>) {
            return currentValue instanceof List<?> ? copy(currentValue) : copy(defaultValue);
        }
        // Some language entries intentionally replace a scalar default with a
        // rich map/list component. The key is known, so retain that user value.
        return currentValue;
    }

    private static Map<String, Object> stringMap(Map<?, ?> source) {
        LinkedHashMap<String, Object> output = new LinkedHashMap<>();
        source.forEach((key, value) -> output.put(String.valueOf(key), copy(value)));
        return output;
    }

    private static Object copy(Object value) {
        if (value instanceof Map<?, ?> map) {
            return stringMap(map);
        }
        if (value instanceof List<?> list) {
            List<Object> output = new ArrayList<>(list.size());
            list.forEach(entry -> output.add(copy(entry)));
            return output;
        }
        return value;
    }

    private static void write(Path file, Map<String, Object> value) throws IOException {
        DumperOptions options = new DumperOptions();
        options.setDefaultFlowStyle(DumperOptions.FlowStyle.BLOCK);
        options.setPrettyFlow(true);
        options.setIndent(2);
        options.setIndicatorIndent(2);
        options.setIndentWithIndicator(true);
        options.setSplitLines(false);
        String yaml = new Yaml(options).dump(value);
        Path temporary = Files.createTempFile(file.toAbsolutePath().getParent(), file.getFileName().toString(), ".tmp");
        try {
            Files.writeString(temporary, yaml, StandardCharsets.UTF_8);
            try {
                Files.move(temporary, file, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException ignored) {
                Files.move(temporary, file, StandardCopyOption.REPLACE_EXISTING);
            }
        } finally {
            Files.deleteIfExists(temporary);
        }
    }
}
