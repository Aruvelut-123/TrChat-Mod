package me.arasple.mc.trchat.filter;

import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.Set;

public final class TextFilter {

    private TextFilter() {
    }

    public static Result filter(
        String input,
        List<String> sensitiveWords,
        Set<Character> ignoredPunctuation,
        List<String> whiteList,
        char replacement
    ) {
        if (input == null || input.isEmpty() || sensitiveWords.isEmpty()) {
            return new Result(input == null ? "" : input, 0);
        }
        char[] output = input.toCharArray();
        String lower = input.toLowerCase(Locale.ROOT);
        boolean[] protectedCharacters = protectedCharacters(lower, whiteList);
        int matches = 0;

        List<String> words = sensitiveWords.stream()
            .filter(word -> word != null && !word.isBlank())
            .map(word -> word.toLowerCase(Locale.ROOT))
            .distinct()
            .sorted(Comparator.comparingInt(String::length).reversed())
            .toList();

        for (int start = 0; start < output.length; start++) {
            if (protectedCharacters[start] || ignoredPunctuation.contains(normalize(output[start]))) {
                continue;
            }
            for (String word : words) {
                Match match = match(lower, start, word, ignoredPunctuation, protectedCharacters);
                if (match == null) continue;
                for (int index = start; index <= match.end(); index++) {
                    if (!ignoredPunctuation.contains(normalize(output[index]))) {
                        output[index] = replacement;
                    }
                }
                matches++;
                start = match.end();
                break;
            }
        }
        return new Result(new String(output), matches);
    }

    private static Match match(
        String input,
        int start,
        String word,
        Set<Character> punctuation,
        boolean[] protectedCharacters
    ) {
        int inputIndex = start;
        int wordIndex = 0;
        while (inputIndex < input.length() && wordIndex < word.length()) {
            if (protectedCharacters[inputIndex]) return null;
            char actual = normalize(input.charAt(inputIndex));
            if (punctuation.contains(actual)) {
                inputIndex++;
                continue;
            }
            if (actual != normalize(word.charAt(wordIndex))) {
                return null;
            }
            inputIndex++;
            wordIndex++;
        }
        return wordIndex == word.length() ? new Match(inputIndex - 1) : null;
    }

    private static boolean[] protectedCharacters(String input, List<String> whiteList) {
        boolean[] protectedCharacters = new boolean[input.length()];
        for (String white : whiteList) {
            if (white == null || white.isBlank()) continue;
            String needle = white.toLowerCase(Locale.ROOT);
            int from = 0;
            while ((from = input.indexOf(needle, from)) >= 0) {
                for (int index = from; index < from + needle.length(); index++) {
                    protectedCharacters[index] = true;
                }
                from += needle.length();
            }
        }
        return protectedCharacters;
    }

    private static char normalize(char value) {
        char halfWidth = value;
        if (value == 12288) {
            halfWidth = ' ';
        } else if (value >= 65281 && value <= 65374) {
            halfWidth = (char) (value - 65248);
        }
        return Character.toLowerCase(halfWidth);
    }

    private record Match(int end) {
    }

    public record Result(String text, int matches) {
    }
}
