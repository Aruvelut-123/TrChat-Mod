package me.arasple.mc.trchat.neoforge.data;

import me.arasple.mc.trchat.neoforge.config.YamlConfigSynchronizer;
import net.neoforged.fml.loading.FMLPaths;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

public final class PlayerDataStore implements AutoCloseable {

    private static final System.Logger LOGGER = System.getLogger(PlayerDataStore.class.getName());

    private final Path folder;
    private String jdbcUrl;
    private String user;
    private String password;
    private String table;
    private String channelTable;
    private String ignoredTable;
    private String preferenceTable;
    private String driver;
    private final ExecutorService saveExecutor;

    public PlayerDataStore() {
        this(FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge"));
    }

    PlayerDataStore(Path folder) {
        this.folder = folder;
        this.saveExecutor = Executors.newSingleThreadExecutor(
            Thread.ofVirtual().name("TrChat-Data-Save-", 0).factory()
        );
    }

    public synchronized void initialize() {
        Path config = folder.resolve("datasource.yml");
        try {
            Files.createDirectories(folder);
            Map<?, ?> root = YamlConfigSynchronizer.synchronize(
                config, "/defaults/datasource.yml", Set.of()
            );
            String type = string(root.get("Type"), "SQLite");
            if (type.equalsIgnoreCase("SQLite") || type.equalsIgnoreCase("Local")) {
                Map<?, ?> sqlite = map(root.get("SQLite"));
                Path configured = Path.of(string(sqlite.get("File"), "data.db"));
                Path database = configured.isAbsolute() ? configured : folder.resolve(configured).normalize();
                Files.createDirectories(database.toAbsolutePath().getParent());
                jdbcUrl = "jdbc:sqlite:" + database.toAbsolutePath();
                user = "";
                password = "";
                table = "trchat_player_state";
                channelTable = "trchat_player_channels";
                ignoredTable = "trchat_player_ignored";
                preferenceTable = "trchat_player_preferences";
                driver = "org.sqlite.JDBC";
            } else if (type.equalsIgnoreCase("MySQL")) {
                configureNetworkDatabase(root, "MySQL", "jdbc:mysql", 3306, "com.mysql.cj.jdbc.Driver");
            } else if (type.equalsIgnoreCase("MariaDB")) {
                configureNetworkDatabase(root, "MariaDB", "jdbc:mariadb", 3306, "org.mariadb.jdbc.Driver");
            } else {
                Map<?, ?> jdbc = map(root.get("JDBC"));
                jdbcUrl = string(jdbc.get("Url"), "");
                user = string(jdbc.get("User"), "");
                password = string(jdbc.get("Password"), "");
                String prefix = string(jdbc.get("Table-Prefix"), "trchat_");
                table = safeIdentifier(prefix + "player_state");
                channelTable = safeIdentifier(prefix + "player_channels");
                driver = string(jdbc.get("Driver"), "");
            }
            if (jdbcUrl.isBlank()) throw new IllegalArgumentException("Data source JDBC URL is empty");
            if (!driver.isBlank()) Class.forName(driver);
            try (Connection connection = connection(); Statement statement = connection.createStatement()) {
                statement.executeUpdate("""
                    CREATE TABLE IF NOT EXISTS %s (
                      uuid VARCHAR(36) PRIMARY KEY,
                      player_name VARCHAR(64) NOT NULL,
                      mute_until BIGINT NOT NULL DEFAULT 0,
                      mute_reason VARCHAR(512) NOT NULL DEFAULT '',
                      shadow_muted INTEGER NOT NULL DEFAULT 0,
                      private_spy INTEGER NOT NULL DEFAULT 0
                    )
                    """.formatted(table));
                statement.executeUpdate("""
                    CREATE TABLE IF NOT EXISTS %s (
                      uuid VARCHAR(36) NOT NULL,
                      channel_id VARCHAR(128) NOT NULL,
                      is_active INTEGER NOT NULL DEFAULT 0,
                      PRIMARY KEY (uuid, channel_id)
                    )
                    """.formatted(channelTable));
                statement.executeUpdate("""
                    CREATE TABLE IF NOT EXISTS %s (
                      uuid VARCHAR(36) NOT NULL,
                      ignored_uuid VARCHAR(36) NOT NULL,
                      ignored_name VARCHAR(64) NOT NULL,
                      PRIMARY KEY (uuid, ignored_uuid)
                    )
                    """.formatted(ignoredTable));
                statement.executeUpdate("""
                    CREATE TABLE IF NOT EXISTS %s (
                      uuid VARCHAR(36) PRIMARY KEY,
                      chat_color VARCHAR(32) NOT NULL DEFAULT ''
                    )
                    """.formatted(preferenceTable));
            }
        } catch (IOException | SQLException | ClassNotFoundException exception) {
            throw new IllegalStateException("Unable to initialize player data storage", exception);
        }
    }

    private void configureNetworkDatabase(
        Map<?, ?> root,
        String section,
        String scheme,
        int defaultPort,
        String driverClass
    ) {
        Map<?, ?> database = map(root.get(section));
        String host = string(database.get("Host"), "127.0.0.1");
        String port = string(database.get("Port"), Integer.toString(defaultPort));
        String name = string(database.get("Database"), "trchat");
        String parameters = string(database.get("Parameters"), "");
        jdbcUrl = scheme + "://" + host + ':' + port + '/' + name
            + (parameters.isBlank() ? "" : '?' + parameters);
        user = string(database.get("User"), "");
        password = string(database.get("Password"), "");
        Map<?, ?> jdbc = map(root.get("JDBC"));
        String prefix = string(jdbc.get("Table-Prefix"), "trchat_");
        table = safeIdentifier(prefix + "player_state");
        channelTable = safeIdentifier(prefix + "player_channels");
        ignoredTable = safeIdentifier(prefix + "player_ignored");
        preferenceTable = safeIdentifier(prefix + "player_preferences");
        driver = driverClass;
    }

    public PlayerState load(UUID uuid, String playerName) {
        String sql = "SELECT mute_until,mute_reason,shadow_muted,private_spy FROM " + table + " WHERE uuid=?";
        try (Connection connection = connection(); PreparedStatement statement = connection.prepareStatement(sql)) {
            statement.setString(1, uuid.toString());
            try (ResultSet result = statement.executeQuery()) {
                if (result.next()) {
                    ChannelMembership membership = loadMembership(connection, uuid);
                    return new PlayerState(
                        uuid,
                        playerName,
                        result.getLong(1),
                        result.getString(2),
                        result.getInt(3) != 0,
                        result.getInt(4) != 0,
                        membership.activeChannel(),
                        membership.joinedChannels(),
                        loadIgnoredPlayers(connection, uuid),
                        loadChatColor(connection, uuid)
                    );
                }
            }
        } catch (SQLException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to load player " + uuid, exception);
        }
        PlayerState state = PlayerState.empty(uuid, playerName);
        save(state);
        return state;
    }

    public void saveAsync(PlayerState state) {
        saveExecutor.execute(() -> save(state));
    }

    public synchronized void save(PlayerState state) {
        String update = "UPDATE " + table
            + " SET player_name=?,mute_until=?,mute_reason=?,shadow_muted=?,private_spy=? WHERE uuid=?";
        String insert = "INSERT INTO " + table
            + " (uuid,player_name,mute_until,mute_reason,shadow_muted,private_spy) VALUES (?,?,?,?,?,?)";
        try (Connection connection = connection()) {
            connection.setAutoCommit(false);
            try {
                try (PreparedStatement updateStatement = connection.prepareStatement(update)) {
                    updateStatement.setString(1, state.playerName());
                    updateStatement.setLong(2, state.muteUntil());
                    updateStatement.setString(3, state.muteReason());
                    updateStatement.setInt(4, state.shadowMuted() ? 1 : 0);
                    updateStatement.setInt(5, state.privateSpy() ? 1 : 0);
                    updateStatement.setString(6, state.uuid().toString());
                    if (updateStatement.executeUpdate() == 0) {
                        try (PreparedStatement insertStatement = connection.prepareStatement(insert)) {
                            insertStatement.setString(1, state.uuid().toString());
                            insertStatement.setString(2, state.playerName());
                            insertStatement.setLong(3, state.muteUntil());
                            insertStatement.setString(4, state.muteReason());
                            insertStatement.setInt(5, state.shadowMuted() ? 1 : 0);
                            insertStatement.setInt(6, state.privateSpy() ? 1 : 0);
                            insertStatement.executeUpdate();
                        }
                    }
                }
                saveMembership(connection, state);
                saveIgnoredPlayers(connection, state);
                savePreferences(connection, state);
                connection.commit();
            } catch (SQLException exception) {
                connection.rollback();
                throw exception;
            }
        } catch (SQLException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to save player " + state.uuid(), exception);
        }
    }

    private ChannelMembership loadMembership(Connection connection, UUID uuid) throws SQLException {
        String sql = "SELECT channel_id,is_active FROM " + channelTable + " WHERE uuid=?";
        LinkedHashSet<String> joined = new LinkedHashSet<>();
        String active = "";
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            statement.setString(1, uuid.toString());
            try (ResultSet result = statement.executeQuery()) {
                while (result.next()) {
                    String channel = result.getString(1);
                    if (channel == null || channel.isBlank()) {
                        continue;
                    }
                    joined.add(channel);
                    if (result.getInt(2) != 0) {
                        active = channel;
                    }
                }
            }
        }
        return new ChannelMembership(active, Set.copyOf(joined));
    }

    private void saveMembership(Connection connection, PlayerState state) throws SQLException {
        String delete = "DELETE FROM " + channelTable + " WHERE uuid=?";
        try (PreparedStatement statement = connection.prepareStatement(delete)) {
            statement.setString(1, state.uuid().toString());
            statement.executeUpdate();
        }
        if (state.joinedChannels().isEmpty()) {
            return;
        }
        String insert = "INSERT INTO " + channelTable + " (uuid,channel_id,is_active) VALUES (?,?,?)";
        try (PreparedStatement statement = connection.prepareStatement(insert)) {
            for (String channel : state.joinedChannels().stream().sorted().toList()) {
                statement.setString(1, state.uuid().toString());
                statement.setString(2, channel);
                statement.setInt(3, channel.equalsIgnoreCase(state.activeChannel()) ? 1 : 0);
                statement.addBatch();
            }
            statement.executeBatch();
        }
    }

    private Set<IgnoredPlayer> loadIgnoredPlayers(Connection connection, UUID uuid) throws SQLException {
        String sql = "SELECT ignored_uuid,ignored_name FROM " + ignoredTable + " WHERE uuid=?";
        LinkedHashSet<IgnoredPlayer> ignored = new LinkedHashSet<>();
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            statement.setString(1, uuid.toString());
            try (ResultSet result = statement.executeQuery()) {
                while (result.next()) {
                    try {
                        ignored.add(new IgnoredPlayer(UUID.fromString(result.getString(1)), result.getString(2)));
                    } catch (IllegalArgumentException exception) {
                        LOGGER.log(System.Logger.Level.WARNING, "Ignoring invalid stored UUID for " + uuid);
                    }
                }
            }
        }
        return Set.copyOf(ignored);
    }

    private void saveIgnoredPlayers(Connection connection, PlayerState state) throws SQLException {
        String delete = "DELETE FROM " + ignoredTable + " WHERE uuid=?";
        try (PreparedStatement statement = connection.prepareStatement(delete)) {
            statement.setString(1, state.uuid().toString());
            statement.executeUpdate();
        }
        if (state.ignoredPlayers().isEmpty()) {
            return;
        }
        String insert = "INSERT INTO " + ignoredTable + " (uuid,ignored_uuid,ignored_name) VALUES (?,?,?)";
        try (PreparedStatement statement = connection.prepareStatement(insert)) {
            for (IgnoredPlayer ignored : state.ignoredPlayers()) {
                statement.setString(1, state.uuid().toString());
                statement.setString(2, ignored.uuid().toString());
                statement.setString(3, ignored.name());
                statement.addBatch();
            }
            statement.executeBatch();
        }
    }

    private String loadChatColor(Connection connection, UUID uuid) throws SQLException {
        String sql = "SELECT chat_color FROM " + preferenceTable + " WHERE uuid=?";
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            statement.setString(1, uuid.toString());
            try (ResultSet result = statement.executeQuery()) {
                return result.next() ? string(result.getString(1), "") : "";
            }
        }
    }

    private void savePreferences(Connection connection, PlayerState state) throws SQLException {
        String update = "UPDATE " + preferenceTable + " SET chat_color=? WHERE uuid=?";
        try (PreparedStatement statement = connection.prepareStatement(update)) {
            statement.setString(1, state.chatColor());
            statement.setString(2, state.uuid().toString());
            if (statement.executeUpdate() != 0) {
                return;
            }
        }
        String insert = "INSERT INTO " + preferenceTable + " (uuid,chat_color) VALUES (?,?)";
        try (PreparedStatement statement = connection.prepareStatement(insert)) {
            statement.setString(1, state.uuid().toString());
            statement.setString(2, state.chatColor());
            statement.executeUpdate();
        }
    }

    private Connection connection() throws SQLException {
        return user.isBlank()
            ? DriverManager.getConnection(jdbcUrl)
            : DriverManager.getConnection(jdbcUrl, user, password);
    }

    @Override
    public void close() {
        saveExecutor.shutdown();
        try {
            if (!saveExecutor.awaitTermination(10, TimeUnit.SECONDS)) {
                LOGGER.log(System.Logger.Level.WARNING, "Timed out waiting for queued player data saves");
            }
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
            LOGGER.log(System.Logger.Level.WARNING, "Interrupted while waiting for queued player data saves");
        }
    }

    private static String safeIdentifier(String value) {
        if (!value.matches("[A-Za-z0-9_]+")) throw new IllegalArgumentException("Invalid table prefix");
        return value;
    }

    private static Map<?, ?> map(Object value) {
        return value instanceof Map<?, ?> map ? map : Map.of();
    }

    private static String string(Object value, String fallback) {
        return value == null ? fallback : String.valueOf(value);
    }

    public record PlayerState(
        UUID uuid,
        String playerName,
        long muteUntil,
        String muteReason,
        boolean shadowMuted,
        boolean privateSpy,
        String activeChannel,
        Set<String> joinedChannels,
        Set<IgnoredPlayer> ignoredPlayers,
        String chatColor
    ) {
        public PlayerState {
            activeChannel = normalizeChannel(activeChannel);
            joinedChannels = joinedChannels == null
                ? Set.of()
                : joinedChannels.stream()
                    .map(PlayerState::normalizeChannel)
                    .filter(channel -> !channel.isBlank())
                    .collect(java.util.stream.Collectors.toUnmodifiableSet());
            if (!activeChannel.isBlank() && !joinedChannels.contains(activeChannel)) {
                LinkedHashSet<String> normalized = new LinkedHashSet<>(joinedChannels);
                normalized.add(activeChannel);
                joinedChannels = Set.copyOf(normalized);
            }
            ignoredPlayers = ignoredPlayers == null ? Set.of() : Set.copyOf(ignoredPlayers);
            chatColor = normalizeColor(chatColor);
        }

        public static PlayerState empty(UUID uuid, String name) {
            return new PlayerState(uuid, name, 0L, "", false, false, "", Set.of(), Set.of(), "");
        }

        public PlayerState withMute(long until, String reason) {
            return new PlayerState(
                uuid, playerName, until, reason, shadowMuted, privateSpy, activeChannel, joinedChannels, ignoredPlayers, chatColor
            );
        }

        public PlayerState withShadowMuted(boolean value) {
            return new PlayerState(
                uuid, playerName, muteUntil, muteReason, value, privateSpy, activeChannel, joinedChannels, ignoredPlayers, chatColor
            );
        }

        public PlayerState withPrivateSpy(boolean value) {
            return new PlayerState(
                uuid, playerName, muteUntil, muteReason, shadowMuted, value, activeChannel, joinedChannels, ignoredPlayers, chatColor
            );
        }

        public PlayerState withChannels(String active, Set<String> joined) {
            return new PlayerState(
                uuid, playerName, muteUntil, muteReason, shadowMuted, privateSpy, active, joined, ignoredPlayers, chatColor
            );
        }

        public PlayerState withIgnoredPlayers(Set<IgnoredPlayer> ignored) {
            return new PlayerState(
                uuid, playerName, muteUntil, muteReason, shadowMuted, privateSpy,
                activeChannel, joinedChannels, ignored, chatColor
            );
        }

        public PlayerState withChatColor(String color) {
            return new PlayerState(
                uuid, playerName, muteUntil, muteReason, shadowMuted, privateSpy,
                activeChannel, joinedChannels, ignoredPlayers, color
            );
        }

        private static String normalizeChannel(String channel) {
            return channel == null ? "" : channel.trim().toLowerCase(java.util.Locale.ROOT);
        }

        private static String normalizeColor(String color) {
            String normalized = color == null ? "" : color.trim().toLowerCase(java.util.Locale.ROOT);
            return normalized.matches("[0-9a-f]") ? normalized : "";
        }
    }

    public record IgnoredPlayer(UUID uuid, String name) {
        public IgnoredPlayer {
            if (uuid == null) {
                throw new IllegalArgumentException("Ignored player UUID cannot be null");
            }
            name = name == null || name.isBlank() ? uuid.toString() : name.trim();
        }
    }

    private record ChannelMembership(String activeChannel, Set<String> joinedChannels) {
    }
}
