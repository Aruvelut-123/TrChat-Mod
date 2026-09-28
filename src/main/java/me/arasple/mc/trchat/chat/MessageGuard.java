package me.arasple.mc.trchat.chat;

import java.util.List;
import java.util.Locale;
import java.util.Set;

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

    /**
     * Counts the highest number of consecutive repetitions of any substring within a message.
     * Ported from the upstream TrChat 2.5.2 anti-spam guard.
     *
     * @param message the message to inspect
     * @param whitelist phrases that may repeat without being counted
     * @return the maximum consecutive repeat count (1 when no repeat is found)
     */
    public static int maxConsecutiveRepeat(String message, Set<String> whitelist) {
        int n = message.length();
        if (n < 2) {
            return 1;
        }
        int max = 1;
        boolean checkWhitelist = whitelist != null && !whitelist.isEmpty();
        int i = 0;
        while (i < n) {
            // Prune 1: not enough remaining characters to beat the current maximum.
            if (n - i <= max) {
                break;
            }
            int maxLen = (n - i) / 2;
            int len = 1;
            while (len <= maxLen) {
                // Prune 2: theoretical maximum repeats at this length cannot beat the current maximum.
                int maxPossible = (n - i) / len;
                if (maxPossible <= max) {
                    break;
                }
                if (checkWhitelist && isWhitelistedUnit(message, i, len, whitelist)) {
                    len++;
                    continue;
                }
                int count = 1;
                int j = i + len;
                while (j + len <= n && regionMatches(message, j, message, j - len, len)) {
                    count++;
                    j += len;
                }
                if (count > max) {
                    max = count;
                }
                len++;
            }
            i++;
        }
        return max;
    }

    /**
     * Whether the repeated unit at the given position is exempted by the whitelist.
     * A unit of length {@code len} is whitelisted when it is some whitelisted phrase repeated
     * an integer number of times.
     */
    private static boolean isWhitelistedUnit(String message, int start, int len, Set<String> whitelist) {
        for (String w : whitelist) {
            int wl = w.length();
            if (wl == 0 || len < wl || len % wl != 0) {
                continue;
            }
            int repeat = len / wl;
            boolean ok = true;
            int k = 0;
            while (k < repeat) {
                if (!regionMatches(message, start + k * wl, w, 0, wl)) {
                    ok = false;
                    break;
                }
                k++;
            }
            if (ok) {
                return true;
            }
        }
        return false;
    }

    /** Case-sensitive region match mirroring String.regionMatches semantics for this guard. */
    private static boolean regionMatches(String source, int sourceStart, String target, int targetStart, int length) {
        int sourceEnd = sourceStart + length;
        int targetEnd = targetStart + length;
        if (sourceEnd > source.length() || targetEnd > target.length()) {
            return false;
        }
        for (int i = 0; i < length; i++) {
            if (source.charAt(sourceStart + i) != target.charAt(targetStart + i)) {
                return false;
            }
        }
        return true;
    }
}
