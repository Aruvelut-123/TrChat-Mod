package me.arasple.mc.trchat.neoforge.channel;

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
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.stream.Stream;

public final class ChannelManager {

    private static final System.Logger LOGGER = System.getLogger(ChannelManager.class.getName());
    public static final String[] DEFAULT_CHANNELS = {"Normal", "Global", "Staff", "Private"};

    private final Path directory;
    private volatile Map<String, ChannelDefinition> channels = Map.of();

    public ChannelManager() {
        this(FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge").resolve("channels"));
    }

    ChannelManager(Path directory) {
        this.directory = directory;
    }

    public synchronized int reload() {
        try {
            Files.createDirectories(directory);
            for (String channel : DEFAULT_CHANNELS) {
                copyDefault(channel);
            }

            LinkedHashMap<String, ChannelDefinition> loaded = new LinkedHashMap<>();
            try (Stream<Path> files = Files.walk(directory)) {
                for (Path file : files
                    .filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().toLowerCase(Locale.ROOT).endsWith(".yml"))
                    .sorted()
                    .toList()) {
                    ChannelDefinition definition = load(file);
                    loaded.put(definition.id().toLowerCase(Locale.ROOT), definition);
                }
            }
            if (!loaded.containsKey("normal")) {
                throw new IOException("Normal.yml is required");
            }
            channels = Map.copyOf(loaded);
            LOGGER.log(System.Logger.Level.INFO, "Loaded {0} chat channels from {1}", loaded.size(), directory);
            return loaded.size();
        } catch (IOException | RuntimeException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to load channel configuration from " + directory, exception);
            return -1;
        }
    }

    public List<ChannelDefinition> all() {
        return channels.values().stream()
            .sorted(Comparator.comparing(ChannelDefinition::id))
            .toList();
    }

    public ChannelDefinition normal() {
        return channels.get("normal");
    }

    public Optional<ChannelDefinition> byId(String id) {
        return Optional.ofNullable(channels.get(id.toLowerCase(Locale.ROOT)));
    }

    public Optional<ChannelDefinition> byCommand(String command) {
        return all().stream()
            .filter(channel -> channel.bindings().commands().stream().anyMatch(command::equalsIgnoreCase))
            .findFirst();
    }

    public PrefixMatch byPrefix(String message) {
        List<PrefixMatch> matches = new ArrayList<>();
        for (ChannelDefinition channel : all()) {
            for (String prefix : channel.bindings().prefixes()) {
                if (message.startsWith(prefix)) {
                    matches.add(new PrefixMatch(channel, prefix));
                }
            }
        }
        return matches.stream()
            .max(Comparator.comparingInt(match -> match.prefix().length()))
            .orElse(null);
    }

    public Path directory() {
        return directory;
    }

    @SuppressWarnings("unchecked")
    private ChannelDefinition load(Path file) throws IOException {
        LoaderOptions options = new LoaderOptions();
        options.setAllowDuplicateKeys(false);
        options.setCodePointLimit(4 * 1024 * 1024);
        Yaml yaml = new Yaml(new SafeConstructor(options));
        try (Reader reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
            Object root = yaml.load(reader);
            if (!(root instanceof Map<?, ?> map)) {
                throw new IOException("Channel root must be a YAML mapping: " + file);
            }
            String name = file.getFileName().toString();
            return ChannelDefinition.from(name.substring(0, name.length() - 4), map);
        }
    }

    private void copyDefault(String channel) throws IOException {
        Path target = directory.resolve(channel + ".yml");
        if (Files.exists(target)) {
            return;
        }
        String resource = "/defaults/channels/" + channel + ".yml";
        try (InputStream input = ChannelManager.class.getResourceAsStream(resource)) {
            if (input == null) {
                throw new IOException("Missing bundled channel " + resource);
            }
            Files.copy(input, target);
        }
    }

    public record PrefixMatch(ChannelDefinition channel, String prefix) {
    }
}
