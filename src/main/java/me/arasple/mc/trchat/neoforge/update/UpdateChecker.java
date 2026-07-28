package me.arasple.mc.trchat.neoforge.update;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import me.arasple.mc.trchat.neoforge.TrChatNeoForge;
import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import me.arasple.mc.trchat.neoforge.lang.LanguageService;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.HoverEvent;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

public final class UpdateChecker implements AutoCloseable {

    private static final URI API = URI.create(
        "https://api.github.com/repos/Aruvelut-123/TrChat-Neoforge/releases/latest"
    );
    private static final String RELEASES_URL =
        "https://github.com/Aruvelut-123/TrChat-Neoforge/releases";

    private final MinecraftServer server;
    private final LanguageService languages;
    private final String currentText;
    private final SemanticVersion current;
    private final HttpClient client = HttpClient.newBuilder()
        .connectTimeout(Duration.ofSeconds(30))
        .followRedirects(HttpClient.Redirect.NORMAL)
        .build();
    private final ScheduledExecutorService executor = Executors.newSingleThreadScheduledExecutor(task -> {
        Thread thread = new Thread(task, "TrChat Update Checker");
        thread.setDaemon(true);
        return thread;
    });
    private final AtomicBoolean checking = new AtomicBoolean();
    private final Set<UUID> notified = ConcurrentHashMap.newKeySet();

    private volatile ReleaseInfo available;
    private volatile boolean reportedCurrent;

    public UpdateChecker(MinecraftServer server, LanguageService languages, String currentVersion) {
        this.server = server;
        this.languages = languages;
        currentText = currentVersion;
        current = SemanticVersion.parse(currentVersion);
    }

    public void start() {
        executor.scheduleWithFixedDelay(
            this::check,
            1,
            TrChatConfig.UPDATE_CHECK_INTERVAL_MINUTES.get(),
            TimeUnit.MINUTES
        );
    }

    public void notifyPlayer(ServerPlayer player) {
        ReleaseInfo release = available;
        if (release == null
            || notified.contains(player.getUUID())
            || !TrChatPermissions.check(player, "trchat.admin")) {
            return;
        }
        player.sendSystemMessage(message(player, release));
        notified.add(player.getUUID());
    }

    private void check() {
        if (!checking.compareAndSet(false, true)) {
            return;
        }
        try {
            HttpRequest request = HttpRequest.newBuilder(API)
                .timeout(Duration.ofSeconds(30))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "TrChat-NeoForge/" + currentText)
                .GET()
                .build();
            HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());
            if (response.statusCode() < 200 || response.statusCode() >= 300) {
                throw new IllegalStateException("GitHub API returned HTTP " + response.statusCode());
            }
            JsonObject json = JsonParser.parseString(response.body()).getAsJsonObject();
            String tag = json.get("tag_name").getAsString();
            String page = json.has("html_url") ? json.get("html_url").getAsString() : RELEASES_URL;
            SemanticVersion latest = SemanticVersion.parse(tag);
            if (latest.compareTo(current) > 0) {
                ReleaseInfo release = new ReleaseInfo(tag.replaceFirst("^[vV]", ""), page);
                boolean changed = !release.equals(available);
                available = release;
                if (changed) {
                    notified.clear();
                    server.execute(() -> notifyAvailable(release));
                }
            } else {
                available = null;
                if (!reportedCurrent) {
                    reportedCurrent = true;
                    if (current.compareTo(latest) > 0) {
                        TrChatNeoForge.LOGGER.info(
                            "TrChat NeoForge {} is newer than the latest GitHub release {}.",
                            currentText, tag
                        );
                    } else {
                        TrChatNeoForge.LOGGER.info("TrChat NeoForge {} is up to date.", currentText);
                    }
                }
            }
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
        } catch (Exception exception) {
            TrChatNeoForge.LOGGER.warn("Unable to check TrChat NeoForge updates: {}", exception.getMessage());
        } finally {
            checking.set(false);
        }
    }

    private void notifyAvailable(ReleaseInfo release) {
        TrChatNeoForge.LOGGER.warn(
            "TrChat NeoForge update available: {} -> {} ({})",
            currentText, release.version(), release.url()
        );
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            notifyPlayer(player);
        }
    }

    private MutableComponent message(ServerPlayer player, ReleaseInfo release) {
        MutableComponent output = languages.component(
            player, "Updater-Available", currentText, release.version()
        ).copy();
        MutableComponent link = languages.component(player, "Updater-Link").copy()
            .withStyle(style -> style
                .withClickEvent(new ClickEvent(ClickEvent.Action.OPEN_URL, release.url()))
                .withHoverEvent(new HoverEvent(
                    HoverEvent.Action.SHOW_TEXT,
                    languages.component(player, "Updater-Link-Hover")
                )));
        return output
            .append("\n")
            .append(languages.component(player, "Updater-Link-Prefix"))
            .append(link)
            .append("\n")
            .append(languages.component(player, "Status-Footer"));
    }

    @Override
    public void close() {
        executor.shutdownNow();
        notified.clear();
    }

    private record ReleaseInfo(String version, String url) {
    }
}
