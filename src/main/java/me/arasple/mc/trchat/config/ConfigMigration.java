package me.arasple.mc.trchat.config;

import net.neoforged.fml.loading.FMLPaths;

import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Comparator;
import java.util.stream.Stream;

/**
 * One-time migration from the legacy {@code trchat-neoforge} configuration
 * directory to the new {@code trchat} directory. Runs before any config file
 * is registered so existing settings, channels, languages and logs are moved
 * over, and the legacy directory is removed only after a successful move.
 */
public final class ConfigMigration {

    public static final String LEGACY_DIR = "trchat-neoforge";
    public static final String CURRENT_DIR = "trchat";

    private static final System.Logger LOGGER = System.getLogger(ConfigMigration.class.getName());

    private ConfigMigration() {
    }

    public static void migrateIfNeeded() {
        Path configRoot = FMLPaths.CONFIGDIR.get();
        Path legacy = configRoot.resolve(LEGACY_DIR);
        Path current = configRoot.resolve(CURRENT_DIR);
        if (!Files.isDirectory(legacy)) {
            return;
        }
        if (Files.exists(current)) {
            LOGGER.log(System.Logger.Level.WARNING,
                "Both {0} and {1} config directories exist; skipping migration and keeping {1}",
                legacy, current);
            return;
        }
        try {
            Files.createDirectories(current);
            try (Stream<Path> entries = Files.walk(legacy)) {
                for (Path source : entries.filter(path -> !path.equals(legacy)).toList()) {
                    Path relative = legacy.relativize(source);
                    Path target = current.resolve(relative);
                    if (Files.isDirectory(source)) {
                        Files.createDirectories(target);
                    } else {
                        Files.createDirectories(target.getParent());
                        moveQuietly(source, target);
                    }
                }
            }
            deleteRecursively(legacy);
            LOGGER.log(System.Logger.Level.INFO,
                "Migrated configuration from {0} to {1}; legacy directory removed",
                legacy, current);
        } catch (IOException | RuntimeException exception) {
            LOGGER.log(System.Logger.Level.ERROR,
                "Failed to migrate configuration from " + legacy + " to " + current, exception);
        }
    }

    private static void moveQuietly(Path source, Path target) throws IOException {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignored) {
            Files.move(source, target, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static void deleteRecursively(Path directory) throws IOException {
        try (Stream<Path> entries = Files.walk(directory)) {
            for (Path path : entries.sorted(Comparator.reverseOrder()).toList()) {
                Files.deleteIfExists(path);
            }
        }
    }
}
