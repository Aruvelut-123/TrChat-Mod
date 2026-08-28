package me.arasple.mc.trchat.protocol;

import java.util.ArrayList;
import java.util.List;

/**
 * Encodes the exact {"data":[...]} envelope used by TabooLib Alkaid Redis in TrChat Bukkit 2.4.9.
 * A single scalar data value is accepted on decode because the Bukkit ArrayConverter emits it that way.
 */
public final class RedisEnvelopeCodec {

    private RedisEnvelopeCodec() {
    }

    public static String encode(TrChatMessage message) {
        StringBuilder json = new StringBuilder("{\"data\":[");
        for (int i = 0; i < message.data().size(); i++) {
            if (i > 0) {
                json.append(',');
            }
            appendString(json, message.data().get(i));
        }
        return json.append("]}").toString();
    }

    public static TrChatMessage decode(String json) {
        Parser parser = new Parser(json);
        return new TrChatMessage(parser.readEnvelope());
    }

    public static String quote(String value) {
        StringBuilder result = new StringBuilder(value.length() + 2);
        appendString(result, value);
        return result.toString();
    }

    private static void appendString(StringBuilder output, String value) {
        output.append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"' -> output.append("\\\"");
                case '\\' -> output.append("\\\\");
                case '\b' -> output.append("\\b");
                case '\f' -> output.append("\\f");
                case '\n' -> output.append("\\n");
                case '\r' -> output.append("\\r");
                case '\t' -> output.append("\\t");
                default -> {
                    if (c < 0x20) {
                        output.append(String.format("\\u%04x", (int) c));
                    } else {
                        output.append(c);
                    }
                }
            }
        }
        output.append('"');
    }

    private static final class Parser {

        private final String source;
        private int cursor;

        private Parser(String source) {
            this.source = source;
        }

        private List<String> readEnvelope() {
            skipWhitespace();
            expect('{');
            List<String> data = null;
            while (true) {
                skipWhitespace();
                if (consume('}')) {
                    break;
                }
                String key = readString();
                skipWhitespace();
                expect(':');
                skipWhitespace();
                if ("data".equals(key)) {
                    data = peek() == '[' ? readArray() : List.of(readString());
                } else {
                    skipValue();
                }
                skipWhitespace();
                if (consume('}')) {
                    break;
                }
                expect(',');
            }
            skipWhitespace();
            if (cursor != source.length() || data == null || data.isEmpty()) {
                throw error("Missing or empty data field");
            }
            return data;
        }

        private List<String> readArray() {
            expect('[');
            List<String> values = new ArrayList<>();
            skipWhitespace();
            if (consume(']')) {
                return values;
            }
            while (true) {
                skipWhitespace();
                values.add(readString());
                skipWhitespace();
                if (consume(']')) {
                    return values;
                }
                expect(',');
            }
        }

        private String readString() {
            expect('"');
            StringBuilder result = new StringBuilder();
            while (cursor < source.length()) {
                char c = source.charAt(cursor++);
                if (c == '"') {
                    return result.toString();
                }
                if (c != '\\') {
                    result.append(c);
                    continue;
                }
                if (cursor >= source.length()) {
                    throw error("Unterminated escape sequence");
                }
                char escaped = source.charAt(cursor++);
                switch (escaped) {
                    case '"' -> result.append('"');
                    case '\\' -> result.append('\\');
                    case '/' -> result.append('/');
                    case 'b' -> result.append('\b');
                    case 'f' -> result.append('\f');
                    case 'n' -> result.append('\n');
                    case 'r' -> result.append('\r');
                    case 't' -> result.append('\t');
                    case 'u' -> {
                        if (cursor + 4 > source.length()) {
                            throw error("Incomplete unicode escape");
                        }
                        try {
                            result.append((char) Integer.parseInt(source.substring(cursor, cursor + 4), 16));
                        } catch (NumberFormatException exception) {
                            throw error("Invalid unicode escape");
                        }
                        cursor += 4;
                    }
                    default -> throw error("Invalid escape sequence");
                }
            }
            throw error("Unterminated string");
        }

        private void skipValue() {
            if (peek() == '"') {
                readString();
                return;
            }
            int depth = 0;
            boolean string = false;
            while (cursor < source.length()) {
                char c = source.charAt(cursor);
                if (c == '"' && (cursor == 0 || source.charAt(cursor - 1) != '\\')) {
                    string = !string;
                } else if (!string) {
                    if (c == '[' || c == '{') {
                        depth++;
                    } else if (c == ']' || c == '}') {
                        if (depth == 0) {
                            return;
                        }
                        depth--;
                    } else if (c == ',' && depth == 0) {
                        return;
                    }
                }
                cursor++;
            }
        }

        private void skipWhitespace() {
            while (cursor < source.length() && Character.isWhitespace(source.charAt(cursor))) {
                cursor++;
            }
        }

        private char peek() {
            if (cursor >= source.length()) {
                throw error("Unexpected end of JSON");
            }
            return source.charAt(cursor);
        }

        private boolean consume(char expected) {
            if (cursor < source.length() && source.charAt(cursor) == expected) {
                cursor++;
                return true;
            }
            return false;
        }

        private void expect(char expected) {
            if (!consume(expected)) {
                throw error("Expected '" + expected + "'");
            }
        }

        private IllegalArgumentException error(String message) {
            return new IllegalArgumentException(message + " at character " + cursor);
        }
    }
}
