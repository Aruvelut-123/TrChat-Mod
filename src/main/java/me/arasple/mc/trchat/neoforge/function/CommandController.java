package me.arasple.mc.trchat.neoforge.function;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

final class CommandController {

    private static final System.Logger LOGGER = System.getLogger(CommandController.class.getName());
    private static final Pattern PROPERTY = Pattern.compile("\\{([^}:]+):\\s*([^}]*)}");

    private CommandController() {
    }

    static Configuration from(Map<?, ?> command) {
        List<Rule> rules = new ArrayList<>();
        for (String source : strings(command.get("List"))) {
            String expression = source.substring(0, source.indexOf('{') >= 0 ? source.indexOf('{') : source.length());
            Map<String, String> properties = properties(source);
            try {
                rules.add(new Rule(
                    source,
                    Pattern.compile(expression, Pattern.CASE_INSENSITIVE),
                    Boolean.parseBoolean(properties.getOrDefault("exact", "false")),
                    properties.getOrDefault("condition", ""),
                    secondsMillis(properties.getOrDefault("cooldown", "0"))
                ));
            } catch (PatternSyntaxException exception) {
                LOGGER.log(System.Logger.Level.WARNING, "Ignoring invalid command pattern: " + expression);
            }
        }
        return new Configuration(
            bool(command.get("Enabled"), bool(command.get("Enable"), true)),
            List.copyOf(rules)
        );
    }

    static Rule matching(String commandLine, List<Rule> rules) {
        String input = commandLine.startsWith("/") ? commandLine.substring(1) : commandLine;
        input = input.trim();
        if (input.isEmpty()) {
            return null;
        }
        String label = input.split("\\s+", 2)[0];
        for (Rule rule : rules) {
            boolean matches = rule.exact()
                ? rule.pattern().matcher(input).matches()
                : rule.pattern().matcher(label).matches();
            if (matches) {
                return rule;
            }
        }
        return null;
    }

    private static Map<String, String> properties(String value) {
        Map<String, String> result = new HashMap<>();
        Matcher matcher = PROPERTY.matcher(value);
        while (matcher.find()) {
            result.put(matcher.group(1).trim().toLowerCase(Locale.ROOT), matcher.group(2).trim());
        }
        return result;
    }

    private static long secondsMillis(String value) {
        try {
            return Math.round(Double.parseDouble(value) * 1000.0D);
        } catch (NumberFormatException ignored) {
            return 0;
        }
    }

    private static boolean bool(Object value, boolean fallback) {
        return value == null ? fallback : Boolean.parseBoolean(String.valueOf(value));
    }

    private static List<String> strings(Object value) {
        if (value instanceof List<?> list) {
            return list.stream().map(String::valueOf).toList();
        }
        return value == null ? List.of() : List.of(String.valueOf(value));
    }

    record Rule(
        String source,
        Pattern pattern,
        boolean exact,
        String condition,
        long cooldownMillis
    ) {
    }

    record Configuration(boolean enabled, List<Rule> rules) {
    }
}
