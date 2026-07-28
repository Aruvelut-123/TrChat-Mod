package me.arasple.mc.trchat.neoforge.update;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

final class SemanticVersion implements Comparable<SemanticVersion> {

    private final List<Integer> numbers;
    private final List<String> preRelease;

    private SemanticVersion(List<Integer> numbers, List<String> preRelease) {
        this.numbers = numbers;
        this.preRelease = preRelease;
    }

    static SemanticVersion parse(String value) {
        String normalized = value == null ? "" : value.trim().toLowerCase(Locale.ROOT);
        if (normalized.startsWith("v")) {
            normalized = normalized.substring(1);
        }
        normalized = normalized.split("\\+", 2)[0];
        String[] parts = normalized.split("-", 2);
        List<Integer> numbers = new ArrayList<>();
        for (String part : parts[0].split("\\.")) {
            try {
                numbers.add(Integer.parseInt(part.replaceFirst("^(\\d+).*$", "$1")));
            } catch (NumberFormatException ignored) {
                numbers.add(0);
            }
        }
        if (numbers.isEmpty()) {
            numbers.add(0);
        }
        List<String> preRelease = parts.length < 2 || parts[1].isBlank()
            ? List.of()
            : List.of(parts[1].split("\\."));
        return new SemanticVersion(List.copyOf(numbers), preRelease);
    }

    @Override
    public int compareTo(SemanticVersion other) {
        int size = Math.max(numbers.size(), other.numbers.size());
        for (int index = 0; index < size; index++) {
            int left = index < numbers.size() ? numbers.get(index) : 0;
            int right = index < other.numbers.size() ? other.numbers.get(index) : 0;
            int compared = Integer.compare(left, right);
            if (compared != 0) {
                return compared;
            }
        }
        if (preRelease.isEmpty() && other.preRelease.isEmpty()) {
            return 0;
        }
        if (preRelease.isEmpty() || other.preRelease.isEmpty()) {
            return preRelease.isEmpty() ? 1 : -1;
        }
        size = Math.max(preRelease.size(), other.preRelease.size());
        for (int index = 0; index < size; index++) {
            if (index >= preRelease.size()) return -1;
            if (index >= other.preRelease.size()) return 1;
            String left = preRelease.get(index);
            String right = other.preRelease.get(index);
            boolean leftNumeric = left.chars().allMatch(Character::isDigit);
            boolean rightNumeric = right.chars().allMatch(Character::isDigit);
            int compared;
            if (leftNumeric && rightNumeric) {
                compared = Long.compare(Long.parseLong(left), Long.parseLong(right));
            } else if (leftNumeric != rightNumeric) {
                compared = leftNumeric ? -1 : 1;
            } else {
                compared = left.compareTo(right);
            }
            if (compared != 0) {
                return compared;
            }
        }
        return 0;
    }
}
