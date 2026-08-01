package me.arasple.mc.trchat.neoforge.update;

import java.util.ArrayList;
import java.util.List;

final class ReleaseNotes {

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
}
