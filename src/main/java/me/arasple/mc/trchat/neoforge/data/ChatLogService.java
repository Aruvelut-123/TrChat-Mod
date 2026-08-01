package me.arasple.mc.trchat.neoforge.data;

import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import net.neoforged.fml.loading.FMLPaths;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalTime;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.util.concurrent.ConcurrentLinkedQueue;

public final class ChatLogService implements AutoCloseable {

    private static final System.Logger LOGGER = System.getLogger(ChatLogService.class.getName());
    private static final DateTimeFormatter FILE_DATE = DateTimeFormatter.ISO_LOCAL_DATE;
    private static final DateTimeFormatter LINE_TIME = DateTimeFormatter.ofPattern("HH:mm:ss");
    private static final long FLUSH_INTERVAL_TICKS = 20L * 60L * 5L;
    private static final long CLEANUP_INTERVAL_TICKS = 20L * 60L * 60L;

    private final Path directory;
    private final ConcurrentLinkedQueue<String> pending = new ConcurrentLinkedQueue<>();
    private long ticks;

    public ChatLogService() {
        this(FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge").resolve("logs"));
    }

    ChatLogService(Path directory) {
        this.directory = directory;
        deleteExpired();
    }

    public void logNormal(String sender, String message) {
        pending.add(format(
            TrChatConfig.LOG_NORMAL_FORMAT.get(), now(), safe(sender), safe(message)
        ));
    }

    public void logPrivate(String sender, String target, String message) {
        pending.add(format(
            TrChatConfig.LOG_PRIVATE_FORMAT.get(), now(), safe(sender), safe(target), safe(message)
        ));
    }

    public void tick() {
        ticks++;
        if (ticks % FLUSH_INTERVAL_TICKS == 0L) {
            flush();
        }
        if (ticks % CLEANUP_INTERVAL_TICKS == 0L) {
            deleteExpired();
        }
    }

    synchronized void flush() {
        if (pending.isEmpty()) {
            return;
        }
        try {
            Files.createDirectories(directory);
            StringBuilder batch = new StringBuilder();
            String line;
            while ((line = pending.poll()) != null) {
                batch.append(line).append(System.lineSeparator());
            }
            Files.writeString(
                directory.resolve(FILE_DATE.format(LocalDate.now()) + ".txt"),
                batch,
                StandardCharsets.UTF_8,
                StandardOpenOption.CREATE,
                StandardOpenOption.APPEND
            );
        } catch (IOException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to write TrChat log", exception);
        }
    }

    private void deleteExpired() {
        int days = TrChatConfig.LOG_RETENTION_DAYS.getAsInt();
        if (days <= 0 || !Files.isDirectory(directory)) {
            return;
        }
        Instant cutoff = Instant.now().minusSeconds(days * 86_400L);
        try (var files = Files.list(directory)) {
            for (Path file : files.filter(Files::isRegularFile).toList()) {
                if (Files.getLastModifiedTime(file).toInstant().isBefore(cutoff)) {
                    Files.deleteIfExists(file);
                }
            }
        } catch (IOException exception) {
            LOGGER.log(System.Logger.Level.WARNING, "Unable to clean expired TrChat logs", exception);
        }
    }

    static String format(String template, String... arguments) {
        String formatted = template == null ? "" : template;
        for (int index = 0; index < arguments.length; index++) {
            formatted = formatted.replace("{" + index + "}", arguments[index]);
        }
        return formatted;
    }

    private static String now() {
        return LINE_TIME.format(LocalTime.now(ZoneId.systemDefault()));
    }

    private static String safe(String value) {
        return value == null ? "" : value.replace('\r', ' ').replace('\n', ' ');
    }

    @Override
    public void close() {
        flush();
    }
}
