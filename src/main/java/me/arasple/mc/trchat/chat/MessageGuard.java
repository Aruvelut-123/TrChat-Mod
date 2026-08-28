package me.arasple.mc.trchat.chat;

import java.util.List;
import java.util.Locale;

public final class MessageGuard {

    private MessageGuard() {
    }

    public static String filter(String message, List<? extends String> blockedWords, String replacement) {
        String filtered = message;
        for (String blocked : blockedWords) {
            if (blocked == null || blocked.isBlank()) {
                continue;
            }
            filtered = replaceIgnoreCase(filtered, blocked, replacement.repeat(Math.max(1, blocked.length())));
        }
        return filtered;
    }

    public static double similarity(String left, String right) {
        String a = normalize(left);
        String b = normalize(right);
        if (a.equals(b)) {
            return 1.0D;
        }
        int longest = Math.max(a.length(), b.length());
        if (longest == 0) {
            return 1.0D;
        }
        return 1.0D - (levenshtein(a, b) / (double) longest);
    }

    private static String replaceIgnoreCase(String source, String target, String replacement) {
        String lowerSource = source.toLowerCase(Locale.ROOT);
        String lowerTarget = target.toLowerCase(Locale.ROOT);
        StringBuilder result = new StringBuilder(source.length());
        int cursor = 0;
        int match;
        while ((match = lowerSource.indexOf(lowerTarget, cursor)) >= 0) {
            result.append(source, cursor, match).append(replacement);
            cursor = match + target.length();
        }
        return result.append(source, cursor, source.length()).toString();
    }

    private static int levenshtein(String left, String right) {
        int[] previous = new int[right.length() + 1];
        int[] current = new int[right.length() + 1];
        for (int j = 0; j <= right.length(); j++) {
            previous[j] = j;
        }
        for (int i = 1; i <= left.length(); i++) {
            current[0] = i;
            for (int j = 1; j <= right.length(); j++) {
                int substitution = previous[j - 1] + (left.charAt(i - 1) == right.charAt(j - 1) ? 0 : 1);
                current[j] = Math.min(Math.min(previous[j] + 1, current[j - 1] + 1), substitution);
            }
            int[] swap = previous;
            previous = current;
            current = swap;
        }
        return previous[right.length()];
    }

    private static String normalize(String value) {
        return value.toLowerCase(Locale.ROOT).replaceAll("\\s+", "");
    }
}
