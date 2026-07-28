package me.arasple.mc.trchat.neoforge.redis;

import org.junit.jupiter.api.Test;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.IOException;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

import static org.junit.jupiter.api.Assertions.assertEquals;

class RespConnectionTest {

    @Test
    void authenticatesSelectsAndPublishesUsingResp2() throws Exception {
        try (ServerSocket server = new ServerSocket(0)) {
            CompletableFuture<List<List<String>>> received = CompletableFuture.supplyAsync(() -> {
                try (Socket socket = server.accept()) {
                    BufferedInputStream input = new BufferedInputStream(socket.getInputStream());
                    BufferedOutputStream output = new BufferedOutputStream(socket.getOutputStream());
                    List<List<String>> commands = new ArrayList<>();
                    commands.add(readCommand(input));
                    respond(output, "+OK\r\n");
                    commands.add(readCommand(input));
                    respond(output, "+OK\r\n");
                    commands.add(readCommand(input));
                    respond(output, ":1\r\n");
                    return commands;
                } catch (IOException exception) {
                    throw new RuntimeException(exception);
                }
            });

            RedisSettings settings = new RedisSettings(
                "127.0.0.1",
                server.getLocalPort(),
                "alice",
                "secret",
                2,
                1000,
                1000,
                100,
                "trchat-message"
            );
            try (RespConnection connection = new RespConnection(settings)) {
                assertEquals(1L, connection.command("PUBLISH", settings.channel(), "{\"data\":[\"test\"]}"));
            }

            assertEquals(List.of(
                List.of("AUTH", "alice", "secret"),
                List.of("SELECT", "2"),
                List.of("PUBLISH", "trchat-message", "{\"data\":[\"test\"]}")
            ), received.get(3, TimeUnit.SECONDS));
        }
    }

    private static List<String> readCommand(BufferedInputStream input) throws IOException {
        int marker = input.read();
        if (marker != '*') {
            throw new IOException("Expected RESP array");
        }
        int count = Integer.parseInt(readLine(input));
        List<String> command = new ArrayList<>(count);
        for (int index = 0; index < count; index++) {
            if (input.read() != '$') {
                throw new IOException("Expected RESP bulk string");
            }
            int length = Integer.parseInt(readLine(input));
            byte[] bytes = input.readNBytes(length);
            if (bytes.length != length || input.read() != '\r' || input.read() != '\n') {
                throw new IOException("Incomplete RESP bulk string");
            }
            command.add(new String(bytes, StandardCharsets.UTF_8));
        }
        return command;
    }

    private static String readLine(BufferedInputStream input) throws IOException {
        StringBuilder line = new StringBuilder();
        while (true) {
            int current = input.read();
            if (current < 0) {
                throw new IOException("Unexpected end of stream");
            }
            if (current == '\r') {
                if (input.read() != '\n') {
                    throw new IOException("Invalid RESP line ending");
                }
                return line.toString();
            }
            line.append((char) current);
        }
    }

    private static void respond(BufferedOutputStream output, String response) throws IOException {
        output.write(response.getBytes(StandardCharsets.US_ASCII));
        output.flush();
    }
}
