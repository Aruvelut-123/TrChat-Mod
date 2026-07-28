package me.arasple.mc.trchat.neoforge.placeholder;

import java.util.ArrayDeque;
import java.util.Deque;

public final class ServerMetrics {

    private static final long FIFTEEN_MINUTES_NANOS = 15L * 60L * 1_000_000_000L;

    private final Deque<Long> ticks = new ArrayDeque<>();

    public void tick() {
        long now = System.nanoTime();
        ticks.addLast(now);
        long oldest = now - FIFTEEN_MINUTES_NANOS;
        while (!ticks.isEmpty() && ticks.getFirst() < oldest) {
            ticks.removeFirst();
        }
    }

    public double tps(int minutes) {
        if (ticks.size() < 2) {
            return 20.0D;
        }
        long now = System.nanoTime();
        long cutoff = now - minutes * 60L * 1_000_000_000L;
        Long first = null;
        int count = 0;
        for (long tick : ticks) {
            if (tick >= cutoff) {
                if (first == null) {
                    first = tick;
                }
                count++;
            }
        }
        if (first == null || count < 2 || now <= first) {
            return 20.0D;
        }
        return Math.min(20.0D, (count - 1) * 1_000_000_000.0D / (now - first));
    }
}
