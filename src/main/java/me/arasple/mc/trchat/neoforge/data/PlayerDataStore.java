package me.arasple.mc.trchat.neoforge.data;

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
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.Map;
import java.util.UUID;

public final class PlayerDataStore implements AutoCloseable {

    private static final System.Logger LOGGER = System.getLogger(PlayerDataStore.class.getName());

    private final Path folder;
    private String jdbcUrl;
    private String user;
    private String password;
    private String table;
    private String driver;

    public PlayerDataStore() {
        this.folder = FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge");
    }

    public synchronized void initialize() {
        Path config = folder.resolve("datasource.yml");
        try {
            Files.createDirectories(folder);
            if (!Files.exists(config)) {
                try (InputStream input = PlayerDataStore.class.getResourceAsStream("/defaults/datasource.yml")) {
                    if (input == null) throw new IOException("Missing bundled datasource.yml");
                    Files.copy(input, config);
                }
            }
            Map<?, ?> root;
            try (Reader reader = Files.newBufferedReader(config, StandardCharsets.UTF_8)) {
                root = new Yaml(new SafeConstructor(new LoaderOptions())).load(reader);
            }
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
                table = safeIdentifier(string(jdbc.get("Table-Prefix"), "trchat_") + "player_state");
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
        table = safeIdentifier(string(jdbc.get("Table-Prefix"), "trchat_") + "player_state");
        driver = driverClass;
    }

    public PlayerState load(UUID uuid, String playerName) {
        String sql = "SELECT mute_until,mute_reason,shadow_muted,private_spy FROM " + table + " WHERE uuid=?";
        try (Connection connection = connection(); PreparedStatement statement = connection.prepareStatement(sql)) {
            statement.setString(1, uuid.toString());
            try (ResultSet result = statement.executeQuery()) {
                if (result.next()) {
                    return new PlayerState(
                        uuid,
                        playerName,
                        result.getLong(1),
                        result.getString(2),
                        result.getInt(3) != 0,
                        result.getInt(4) != 0
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
        Thread.ofVirtual().name("TrChat-Data-Save").start(() -> save(state));
    }

    public synchronized void save(PlayerState state) {
        String update = "UPDATE " + table
            + " SET player_name=?,mute_until=?,mute_reason=?,shadow_muted=?,private_spy=? WHERE uuid=?";
        String insert = "INSERT INTO " + table
            + " (uuid,player_name,mute_until,mute_reason,shadow_muted,private_spy) VALUES (?,?,?,?,?,?)";
        try (Connection connection = connection();
             PreparedStatement updateStatement = connection.prepareStatement(update)) {
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
        } catch (SQLException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to save player " + state.uuid(), exception);
        }
    }

    private Connection connection() throws SQLException {
        return user.isBlank()
            ? DriverManager.getConnection(jdbcUrl)
            : DriverManager.getConnection(jdbcUrl, user, password);
    }

    @Override
    public void close() {
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
        boolean privateSpy
    ) {
        public static PlayerState empty(UUID uuid, String name) {
            return new PlayerState(uuid, name, 0L, "", false, false);
        }

        public PlayerState withMute(long until, String reason) {
            return new PlayerState(uuid, playerName, until, reason, shadowMuted, privateSpy);
        }

        public PlayerState withShadowMuted(boolean value) {
            return new PlayerState(uuid, playerName, muteUntil, muteReason, value, privateSpy);
        }

        public PlayerState withPrivateSpy(boolean value) {
            return new PlayerState(uuid, playerName, muteUntil, muteReason, shadowMuted, value);
        }
    }
}
