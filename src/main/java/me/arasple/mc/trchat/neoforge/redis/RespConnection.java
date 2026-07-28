package me.arasple.mc.trchat.neoforge.redis;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.Closeable;
import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

final class RespConnection implements Closeable {

    private final Socket socket;
    private final InputStream input;
    private final OutputStream output;

    RespConnection(RedisSettings settings) throws IOException {
        socket = new Socket();
        socket.connect(new InetSocketAddress(settings.host(), settings.port()), settings.connectTimeoutMillis());
        socket.setKeepAlive(true);
        socket.setTcpNoDelay(true);
        socket.setSoTimeout(settings.socketTimeoutMillis());
        input = new BufferedInputStream(socket.getInputStream());
        output = new BufferedOutputStream(socket.getOutputStream());

        if (!settings.password().isBlank()) {
            Object auth = settings.username().isBlank()
                ? command("AUTH", settings.password())
                : command("AUTH", settings.username(), settings.password());
            requireOkay(auth, "AUTH");
        }
        if (settings.database() != 0) {
            requireOkay(command("SELECT", Integer.toString(settings.database())), "SELECT");
        }
    }

    synchronized Object command(String... parts) throws IOException {
        write(parts);
        return read();
    }

    synchronized void write(String... parts) throws IOException {
        output.write(('*' + Integer.toString(parts.length) + "\r\n").getBytes(StandardCharsets.US_ASCII));
        for (String part : parts) {
            byte[] bytes = part.getBytes(StandardCharsets.UTF_8);
            output.write(('$' + Integer.toString(bytes.length) + "\r\n").getBytes(StandardCharsets.US_ASCII));
            output.write(bytes);
            output.write('\r');
            output.write('\n');
        }
        output.flush();
    }

    Object read() throws IOException {
        int marker = input.read();
        if (marker < 0) {
            throw new EOFException("Redis closed the connection");
        }
        return switch (marker) {
            case '+' -> readLine();
            case '-' -> throw new IOException("Redis error: " + readLine());
            case ':' -> Long.parseLong(readLine());
            case '$' -> readBulkString();
            case '*' -> readArray();
            default -> throw new IOException("Unsupported Redis RESP marker: " + (char) marker);
        };
    }

    @Override
    public void close() throws IOException {
        socket.close();
    }

    private String readLine() throws IOException {
        StringBuilder line = new StringBuilder();
        int previous = -1;
        while (true) {
            int current = input.read();
            if (current < 0) {
                throw new EOFException("Redis closed the connection");
            }
            if (previous == '\r' && current == '\n') {
                line.setLength(line.length() - 1);
                return line.toString();
            }
            line.append((char) current);
            previous = current;
        }
    }

    private String readBulkString() throws IOException {
        int length = Integer.parseInt(readLine());
        if (length == -1) {
            return null;
        }
        byte[] bytes = input.readNBytes(length);
        if (bytes.length != length || input.read() != '\r' || input.read() != '\n') {
            throw new EOFException("Incomplete Redis bulk string");
        }
        return new String(bytes, StandardCharsets.UTF_8);
    }

    private List<Object> readArray() throws IOException {
        int length = Integer.parseInt(readLine());
        if (length == -1) {
            return null;
        }
        List<Object> values = new ArrayList<>(length);
        for (int i = 0; i < length; i++) {
            values.add(read());
        }
        return values;
    }

    private static void requireOkay(Object response, String command) throws IOException {
        if (!(response instanceof String text) || !"OK".equalsIgnoreCase(text)) {
            throw new IOException("Unexpected Redis " + command + " response: " + response);
        }
    }
}
