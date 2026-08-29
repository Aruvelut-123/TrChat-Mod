package me.arasple.mc.trchat.filter;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import me.arasple.mc.trchat.config.YamlConfigSynchronizer;
import me.arasple.mc.trchat.lang.LanguageService;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.SignBlockEntity;
import net.minecraft.world.level.block.entity.SignText;
import net.minecraft.world.level.chunk.LevelChunk;
import me.arasple.mc.trchat.platform.Platform;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;

public final class FilterService {

    private static final System.Logger LOGGER = System.getLogger(FilterService.class.getName());

    private final MinecraftServer server;
    private final Path file;
    private final Path cacheDirectory;
    private final LanguageService languages;
    private final Set<LevelChunk> loadedChunks = ConcurrentHashMap.newKeySet();
    private volatile Settings settings = Settings.empty();
    private volatile List<String> words = List.of();
    private int ticks;

    public FilterService(MinecraftServer server, LanguageService languages) {
        this.server = server;
        this.languages = languages;
        Path folder = Platform.configDir().resolve("trchat");
        this.file = folder.resolve("filter.yml");
        this.cacheDirectory = folder.resolve("filters");
    }

    public synchronized boolean reload() {
        try {
            Files.createDirectories(file.getParent());
            Map<?, ?> root = YamlConfigSynchronizer.synchronize(
                file, "/defaults/filter.yml", Set.of()
            );
            settings = Settings.from(root);
            words = settings.localWords();
            refreshCloudAsync();
            return true;
        } catch (IOException | RuntimeException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to load " + file, exception);
            return false;
        }
    }

    public String filterChat(ServerPlayer player, String input) {
        //? if >=1.21.11 {
        if (!settings.chat() || player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        )) return input;
        //? } else {
        if (!settings.chat() || player.hasPermissions(2)) return input;
        //? }
        return filter(input).text();
    }

    public TextFilter.Result filter(String input) {
        Settings current = settings;
        return TextFilter.filter(input, words, current.punctuation(), current.whiteList(), current.replacement());
    }

    public boolean checkAnvil(ServerPlayer player, String name) {
        //? if >=1.21.11 {
        if (!settings.anvil() || name == null || player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        )) return true;
        //? } else {
        if (!settings.anvil() || name == null || player.hasPermissions(2)) return true;
        //? }
        TextFilter.Result result = filter(name);
        if (result.matches() == 0) return true;
        player.sendSystemMessage(languages.component(player, "Filter-Anvil-Blocked"));
        return false;
    }

    public void chunkLoaded(LevelChunk chunk) {
        loadedChunks.add(chunk);
    }

    public void chunkUnloaded(LevelChunk chunk) {
        loadedChunks.remove(chunk);
    }

    public void tick() {
        ticks++;
        if (settings.sign() && ticks % 20 == 0) {
            for (LevelChunk chunk : List.copyOf(loadedChunks)) {
                for (BlockEntity blockEntity : chunk.getBlockEntities().values()) {
                    if (blockEntity instanceof SignBlockEntity sign) {
                        filterSign(sign);
                    }
                }
            }
        }
        if (ticks >= 72_000) {
            ticks = 0;
            refreshCloudAsync();
        }
    }

    private void filterSign(SignBlockEntity sign) {
        boolean changed = false;
        for (boolean front : new boolean[]{true, false}) {
            SignText original = sign.getText(front);
            SignText filtered = original;
            for (int line = 0; line < 4; line++) {
                Component message = original.getMessage(line, false);
                TextFilter.Result result = filter(message.getString());
                if (result.matches() > 0) {
                    filtered = filtered.setMessage(line, Component.literal(result.text()).setStyle(message.getStyle()));
                    changed = true;
                }
            }
            if (filtered != original) {
                sign.setText(filtered, front);
            }
        }
        if (changed && sign.getLevel() != null) {
            sign.getLevel().sendBlockUpdated(sign.getBlockPos(), sign.getBlockState(), sign.getBlockState(), 3);
        }
    }

    private void refreshCloudAsync() {
        Settings current = settings;
        if (!current.cloudEnabled() || current.urls().isEmpty()) return;
        Thread.ofVirtual().name("TrChat-Filter-Cloud").start(() -> {
            Set<String> collected = new HashSet<>(current.localWords());
            for (String url : current.urls()) {
                collected.addAll(fetch(url, current.cloudIgnored()));
            }
            words = collected.stream()
                .sorted(java.util.Comparator.comparingInt(String::length).reversed())
                .toList();
            LOGGER.log(System.Logger.Level.INFO, "Loaded {0} filter words", words.size());
        });
    }

    private List<String> fetch(String url, Set<String> ignored) {
        Path cache = cacheDirectory.resolve(Integer.toHexString(url.hashCode()) + ".json");
        String json = null;
        try {
            HttpRequest request = HttpRequest.newBuilder(URI.create(url))
                .timeout(Duration.ofSeconds(30))
                .GET()
                .build();
            json = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(30)).build()
                .send(request, HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8))
                .body();
            Files.createDirectories(cacheDirectory);
            Files.writeString(cache, json, StandardCharsets.UTF_8);
        } catch (Exception exception) {
            try {
                if (Files.exists(cache)) json = Files.readString(cache, StandardCharsets.UTF_8);
            } catch (IOException ignoredException) {
                LOGGER.log(System.Logger.Level.WARNING, "Unable to read filter cache " + cache);
            }
        }
        if (json == null) return List.of();
        try {
            JsonObject root = JsonParser.parseString(json).getAsJsonObject();
            JsonArray array = root.getAsJsonArray("words");
            List<String> result = new ArrayList<>();
            for (JsonElement element : array) {
                String word = element.getAsString();
                if (!ignored.contains(word.toLowerCase(Locale.ROOT))) result.add(word);
            }
            return result;
        } catch (RuntimeException exception) {
            LOGGER.log(System.Logger.Level.WARNING, "Invalid cloud thesaurus from " + url);
            return List.of();
        }
    }

    private record Settings(
        boolean chat,
        boolean sign,
        boolean anvil,
        boolean cloudEnabled,
        List<String> urls,
        Set<String> cloudIgnored,
        List<String> localWords,
        Set<Character> punctuation,
        List<String> whiteList,
        char replacement
    ) {
        private static Settings empty() {
            return new Settings(false, false, false, false, List.of(), Set.of(), List.of(), Set.of(), List.of(), '*');
        }

        private static Settings from(Map<?, ?> root) {
            Map<?, ?> enable = map(root.get("Enable"));
            Map<?, ?> cloud = map(root.get("Cloud-Thesaurus"));
            Set<Character> punctuation = new HashSet<>();
            for (String value : strings(root.get("Ignored-Punctuations"))) {
                for (char character : value.toCharArray()) punctuation.add(Character.toLowerCase(character));
            }
            String replacement = string(root.get("Replacement"), "*");
            return new Settings(
                bool(enable.get("Chat"), true),
                bool(enable.get("Sign"), true),
                bool(enable.get("Anvil"), true),
                bool(cloud.get("Enabled"), true),
                strings(cloud.get("Urls")),
                strings(cloud.get("Ignored")).stream().map(value -> value.toLowerCase(Locale.ROOT)).collect(java.util.stream.Collectors.toSet()),
                strings(root.get("Local")),
                Set.copyOf(punctuation),
                strings(root.get("WhiteList")),
                replacement.isEmpty() ? '*' : replacement.charAt(0)
            );
        }
    }

    private static Map<?, ?> map(Object value) {
        return value instanceof Map<?, ?> map ? map : Map.of();
    }

    private static List<String> strings(Object value) {
        if (value instanceof List<?> list) return list.stream().map(String::valueOf).toList();
        return value == null ? List.of() : List.of(String.valueOf(value));
    }

    private static String string(Object value, String fallback) {
        return value == null ? fallback : String.valueOf(value);
    }

    private static boolean bool(Object value, boolean fallback) {
        return value == null ? fallback : Boolean.parseBoolean(String.valueOf(value));
    }
}
