package me.arasple.mc.trchat.neoforge.channel;

import me.arasple.mc.trchat.neoforge.config.YamlConfigSynchronizer;
import net.neoforged.fml.loading.FMLPaths;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.IOException;
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
    public static final String[] DEFAULT_CHANNELS = {"Normal", "Global", "Staff", "Private", "Server"};
    private static final String EXAMPLE_CHANNEL = "Example";

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
                synchronizeChannel(directory.resolve(channel + ".yml"), channel);
            }
            synchronizeChannel(directory.resolve(EXAMPLE_CHANNEL + ".yml"), EXAMPLE_CHANNEL);

            LinkedHashMap<String, ChannelDefinition> loaded = new LinkedHashMap<>();
            try (Stream<Path> files = Files.walk(directory)) {
                for (Path file : files
                    .filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().toLowerCase(Locale.ROOT).endsWith(".yml"))
                    .filter(path -> !path.getFileName().toString().equalsIgnoreCase(EXAMPLE_CHANNEL + ".yml"))
                    .sorted()
                    .toList()) {
                    synchronizeChannel(file, stem(file));
                    ChannelDefinition definition = load(file);
                    loaded.put(definition.id().toLowerCase(Locale.ROOT), definition);
                }
            }
            if (!loaded.containsKey("normal")) {
                throw new IOException("Normal.yml is required");
            }
            List<String> autoJoinChannels = loaded.values().stream()
                .filter(channel -> channel.options().autoJoin())
                .map(ChannelDefinition::id)
                .sorted(String.CASE_INSENSITIVE_ORDER)
                .toList();
            if (autoJoinChannels.size() > 1) {
                throw new IOException(
                    "Only one channel may set Options.Auto-Join to true; found "
                        + String.join(", ", autoJoinChannels)
                );
            }
            if (autoJoinChannels.size() == 1) {
                ChannelDefinition autoJoin = loaded.get(autoJoinChannels.getFirst().toLowerCase(Locale.ROOT));
                if (autoJoin.options().privateChannel() || autoJoin.id().equalsIgnoreCase("Server")) {
                    throw new IOException(
                        "Options.Auto-Join cannot be enabled for private or Server channel "
                            + autoJoin.id()
                    );
                }
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

    public Optional<ChannelDefinition> autoJoin() {
        return channels.values().stream()
            .filter(channel -> channel.options().autoJoin())
            .findFirst();
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

    private void synchronizeChannel(Path file, String channel) throws IOException {
        String bundled = java.util.Arrays.stream(DEFAULT_CHANNELS)
            .filter(value -> value.equalsIgnoreCase(channel))
            .findFirst()
            .orElseGet(() -> EXAMPLE_CHANNEL.equalsIgnoreCase(channel) ? EXAMPLE_CHANNEL : "Schema");
        YamlConfigSynchronizer.synchronize(
            file, "/defaults/channels/" + bundled + ".yml", java.util.Set.of()
        );
    }

    private static String stem(Path file) {
        String name = file.getFileName().toString();
        return name.substring(0, name.length() - 4);
    }

    public record PrefixMatch(ChannelDefinition channel, String prefix) {
    }
}
