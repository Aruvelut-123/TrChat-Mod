package me.arasple.mc.trchat.neoforge.channel;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

public record ChannelDefinition(
    String id,
    Options options,
    Bindings bindings,
    List<Format> formats,
    List<Format> senderFormats,
    List<Format> receiverFormats,
    List<Format> consoleFormats
) {

    public static ChannelDefinition from(String id, Map<?, ?> root) {
        Map<?, ?> options = map(root.get("Options"));
        Map<?, ?> bindings = map(root.get("Bindings"));
        return new ChannelDefinition(
            id,
            new Options(
                string(options.get("Join-Permission")),
                string(options.get("Listen-Permission")),
                string(options.get("Speak-Condition")),
                bool(options.get("Always-Listen"), false),
                !id.equalsIgnoreCase("Server") && bool(options.get("Auto-Join"), false),
                bool(options.get("Private"), false),
                string(options.containsKey("Target") ? options.get("Target") : "ALL"),
                bool(options.get("Proxy"), false),
                bool(options.get("Force-Proxy"), false),
                bool(options.get("Double-Transfer"), false),
                strings(options.get("Ports")),
                strings(options.get("Disabled-Functions"))
            ),
            new Bindings(strings(bindings.get("Prefix")), strings(bindings.get("Command"))),
            formats(root.get("Formats")),
            formats(root.get("Sender")),
            formats(root.get("Receiver")),
            formats(root.get("Console"))
        );
    }

    public boolean isNormal() {
        return id.equalsIgnoreCase("Normal");
    }

    public boolean isJoinable() {
        return !options.privateChannel() && !id.equalsIgnoreCase("Server");
    }

    public record Options(
        String joinPermission,
        String listenPermission,
        String speakCondition,
        boolean alwaysListen,
        boolean autoJoin,
        boolean privateChannel,
        String target,
        boolean redis,
        boolean forceRedis,
        boolean doubleTransfer,
        List<String> ports,
        List<String> disabledFunctions
    ) {
        public Options {
            target = target == null || target.isBlank() ? "ALL" : target.toUpperCase(Locale.ROOT);
            ports = List.copyOf(ports);
            disabledFunctions = List.copyOf(disabledFunctions);
        }
    }

    public record Bindings(List<String> prefixes, List<String> commands) {
        public Bindings {
            prefixes = List.copyOf(prefixes);
            commands = List.copyOf(commands);
        }
    }

    public record Format(
        String condition,
        int priority,
        LinkedHashMap<String, List<ComponentPart>> prefix,
        MessagePart message,
        LinkedHashMap<String, List<ComponentPart>> suffix
    ) {
    }

    public record ComponentPart(
        String condition,
        int priority,
        String text,
        String hover,
        String suggest,
        String command,
        String url,
        String copy,
        String file,
        String insertion,
        String font
    ) {
    }

    public record MessagePart(String defaultColor, String hover) {
    }

    private static List<Format> formats(Object value) {
        List<Format> result = new ArrayList<>();
        for (Object entry : list(value)) {
            Map<?, ?> format = map(entry);
            result.add(new Format(
                string(format.get("condition")),
                integer(format.get("priority"), 0),
                componentGroups(format.get("prefix")),
                message(format.get("msg")),
                componentGroups(format.get("suffix"))
            ));
        }
        result.sort((left, right) -> Integer.compare(right.priority(), left.priority()));
        return List.copyOf(result);
    }

    private static LinkedHashMap<String, List<ComponentPart>> componentGroups(Object value) {
        LinkedHashMap<String, List<ComponentPart>> result = new LinkedHashMap<>();
        for (Map.Entry<?, ?> entry : map(value).entrySet()) {
            List<ComponentPart> parts = new ArrayList<>();
            Object raw = entry.getValue();
            if (raw instanceof List<?>) {
                for (Object item : list(raw)) {
                    parts.add(componentPart(map(item)));
                }
            } else {
                parts.add(componentPart(map(raw)));
            }
            parts.sort((left, right) -> Integer.compare(right.priority(), left.priority()));
            result.put(String.valueOf(entry.getKey()), List.copyOf(parts));
        }
        return result;
    }

    private static ComponentPart componentPart(Map<?, ?> value) {
        return new ComponentPart(
            string(value.get("condition")),
            integer(value.get("priority"), 0),
            string(value.get("text")),
            string(value.get("hover")),
            string(value.get("suggest")),
            string(value.get("command")),
            string(value.get("url")),
            string(value.get("copy")),
            string(value.get("file")),
            string(value.get("insertion")),
            string(value.get("font"))
        );
    }

    private static MessagePart message(Object value) {
        Map<?, ?> map = map(value);
        String color = string(map.get("default-color"));
        return new MessagePart(color.isBlank() ? "f" : color, string(map.get("hover")));
    }

    @SuppressWarnings("unchecked")
    private static Map<?, ?> map(Object value) {
        return value instanceof Map<?, ?> map ? map : Map.of();
    }

    private static List<?> list(Object value) {
        if (value instanceof List<?> list) {
            return list;
        }
        return value == null ? List.of() : List.of(value);
    }

    private static List<String> strings(Object value) {
        return list(value).stream()
            .map(String::valueOf)
            .filter(item -> !item.isBlank() && !"null".equalsIgnoreCase(item) && !"~".equals(item))
            .toList();
    }

    private static String string(Object value) {
        return value == null || "~".equals(value) ? "" : String.valueOf(value);
    }

    private static boolean bool(Object value, boolean fallback) {
        return value == null ? fallback : Boolean.parseBoolean(String.valueOf(value));
    }

    private static int integer(Object value, int fallback) {
        if (value instanceof Number number) {
            return number.intValue();
        }
        try {
            return value == null ? fallback : Integer.parseInt(String.valueOf(value));
        } catch (NumberFormatException ignored) {
            return fallback;
        }
    }
}
