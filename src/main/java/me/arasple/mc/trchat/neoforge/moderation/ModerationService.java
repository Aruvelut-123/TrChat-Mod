package me.arasple.mc.trchat.neoforge.moderation;

import me.arasple.mc.trchat.neoforge.data.PlayerDataStore;
import me.arasple.mc.trchat.neoforge.lang.LanguageService;
import net.minecraft.server.level.ServerPlayer;

import java.time.Duration;
import java.time.Instant;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.util.Locale;
import java.util.Map;
import java.util.OptionalLong;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class ModerationService implements AutoCloseable {

    private static final Pattern DURATION_PART = Pattern.compile("(\\d+)([smhdw])", Pattern.CASE_INSENSITIVE);
    private static final DateTimeFormatter MUTE_TIME = DateTimeFormatter
        .ofPattern("yyyy-MM-dd HH:mm:ss")
        .withZone(ZoneId.systemDefault());

    private final PlayerDataStore store = new PlayerDataStore();
    private final LanguageService languages = new LanguageService();
    private final Map<UUID, PlayerDataStore.PlayerState> states = new ConcurrentHashMap<>();

    public ModerationService() {
        store.initialize();
        languages.reload();
    }

    public LanguageService languages() {
        return languages;
    }

    public void reloadLanguages() {
        languages.reload();
    }

    public void playerJoined(ServerPlayer player) {
        states.put(player.getUUID(), store.load(player.getUUID(), player.getGameProfile().getName()));
    }

    public void playerLeft(ServerPlayer player) {
        PlayerDataStore.PlayerState state = states.remove(player.getUUID());
        if (state != null) store.saveAsync(state);
    }

    public boolean isMuted(ServerPlayer player) {
        PlayerDataStore.PlayerState state = state(player);
        if (state.muteUntil() == 0L) return false;
        if (state.muteUntil() > 0L && state.muteUntil() <= System.currentTimeMillis()) {
            update(state.withMute(0L, ""));
            return false;
        }
        return true;
    }

    public String muteExpiry(ServerPlayer player) {
        long until = state(player).muteUntil();
        return until < 0L ? "permanent" : MUTE_TIME.format(Instant.ofEpochMilli(until));
    }

    public String muteReason(ServerPlayer player) {
        String reason = state(player).muteReason();
        return reason.isBlank() ? "-" : reason;
    }

    public void mute(ServerPlayer player, long durationMillis, String reason) {
        long until = durationMillis < 0L ? -1L : System.currentTimeMillis() + durationMillis;
        update(state(player).withMute(until, reason == null ? "" : reason));
    }

    public void unmute(ServerPlayer player) {
        update(state(player).withMute(0L, ""));
    }

    public boolean shadowMuted(ServerPlayer player) {
        return state(player).shadowMuted();
    }

    public void setShadowMuted(ServerPlayer player, boolean value) {
        update(state(player).withShadowMuted(value));
    }

    public boolean privateSpy(ServerPlayer player) {
        return state(player).privateSpy();
    }

    public boolean setPrivateSpy(ServerPlayer player, boolean value) {
        update(state(player).withPrivateSpy(value));
        return value;
    }

    public boolean togglePrivateSpy(ServerPlayer player) {
        return setPrivateSpy(player, !privateSpy(player));
    }

    public static OptionalLong parseDuration(String input) {
        if (input == null) return OptionalLong.empty();
        String normalized = input.trim().toLowerCase(Locale.ROOT);
        if (normalized.equals("permanent") || normalized.equals("forever")
            || normalized.equals("perm") || normalized.equals("永久")) {
            return OptionalLong.of(-1L);
        }
        Matcher matcher = DURATION_PART.matcher(normalized);
        long total = 0L;
        int consumed = 0;
        while (matcher.find()) {
            if (matcher.start() != consumed) return OptionalLong.empty();
            long number;
            try {
                number = Long.parseLong(matcher.group(1));
                long multiplier = switch (matcher.group(2).toLowerCase(Locale.ROOT)) {
                    case "s" -> 1_000L;
                    case "m" -> 60_000L;
                    case "h" -> 3_600_000L;
                    case "d" -> 86_400_000L;
                    case "w" -> 604_800_000L;
                    default -> 0L;
                };
                total = Math.addExact(total, Math.multiplyExact(number, multiplier));
            } catch (ArithmeticException exception) {
                return OptionalLong.empty();
            }
            consumed = matcher.end();
        }
        return consumed == normalized.length() && total > 0L ? OptionalLong.of(total) : OptionalLong.empty();
    }

    public static String describeDuration(long durationMillis) {
        if (durationMillis < 0L) return "permanent";
        Duration duration = Duration.ofMillis(durationMillis);
        long days = duration.toDays();
        long hours = duration.minusDays(days).toHours();
        long minutes = duration.minusDays(days).minusHours(hours).toMinutes();
        long seconds = duration.minusDays(days).minusHours(hours).minusMinutes(minutes).toSeconds();
        StringBuilder result = new StringBuilder();
        if (days > 0) result.append(days).append('d');
        if (hours > 0) result.append(hours).append('h');
        if (minutes > 0) result.append(minutes).append('m');
        if (seconds > 0 || result.isEmpty()) result.append(seconds).append('s');
        return result.toString();
    }

    private PlayerDataStore.PlayerState state(ServerPlayer player) {
        return states.computeIfAbsent(
            player.getUUID(),
            ignored -> store.load(player.getUUID(), player.getGameProfile().getName())
        );
    }

    private void update(PlayerDataStore.PlayerState state) {
        states.put(state.uuid(), state);
        store.saveAsync(state);
    }

    @Override
    public void close() {
        states.values().forEach(store::save);
        states.clear();
        store.close();
    }
}
