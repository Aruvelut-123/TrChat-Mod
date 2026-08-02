package me.arasple.mc.trchat.neoforge.update;

import java.util.ArrayList;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

final class ReleaseNotes {

    private static final Pattern LEVEL_TWO_HEADING = Pattern.compile("^\\s*##(?!#)\\s+(.+?)\\s*$");
    private static final Pattern LEVEL_THREE_HEADING = Pattern.compile("^\\s*###(?!#)\\s+(.+?)\\s*$");
    private static final Pattern LIST_ITEM = Pattern.compile("^\\s*-\\s+(.+?)\\s*$");
    private static final Pattern TRAILING_HEADING_MARKERS = Pattern.compile("\\s+#+\\s*$");

    private ReleaseNotes() {
    }

    static List<String> normalize(String body) {
        if (body == null || body.isBlank()) {
            return List.of();
        }
        String[] lines = body.replace("\r\n", "\n").replace('\r', '\n').split("\n", -1);
        int first = 0;
        int last = lines.length;
        while (first < last && lines[first].isBlank()) {
            first++;
        }
        while (last > first && lines[last - 1].isBlank()) {
            last--;
        }
        List<String> normalized = new ArrayList<>(last - first);
        for (int index = first; index < last; index++) {
            normalized.add(lines[index].stripTrailing());
        }
        return List.copyOf(normalized);
    }

    static FormattedLine parseLine(String line) {
        if (line == null || line.isBlank()) {
            return new FormattedLine(LineType.BLANK, "");
        }
        Matcher levelThree = LEVEL_THREE_HEADING.matcher(line);
        if (levelThree.matches()) {
            return new FormattedLine(LineType.LEVEL_THREE_HEADING, headingText(levelThree.group(1)));
        }
        Matcher levelTwo = LEVEL_TWO_HEADING.matcher(line);
        if (levelTwo.matches()) {
            return new FormattedLine(LineType.LEVEL_TWO_HEADING, headingText(levelTwo.group(1)));
        }
        Matcher listItem = LIST_ITEM.matcher(line);
        if (listItem.matches()) {
            return new FormattedLine(LineType.LIST_ITEM, listItem.group(1));
        }
        return new FormattedLine(LineType.TEXT, line);
    }

    private static String headingText(String value) {
        return TRAILING_HEADING_MARKERS.matcher(value).replaceFirst("").strip();
    }

    enum LineType {
        LEVEL_TWO_HEADING,
        LEVEL_THREE_HEADING,
        LIST_ITEM,
        TEXT,
        BLANK
    }

    record FormattedLine(LineType type, String text) {
    }
}
