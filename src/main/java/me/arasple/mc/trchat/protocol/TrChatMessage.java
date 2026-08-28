package me.arasple.mc.trchat.protocol;

import java.util.List;

public record TrChatMessage(List<String> data) {

    public TrChatMessage {
        data = List.copyOf(data);
        if (data.isEmpty()) {
            throw new IllegalArgumentException("TrChat message data must not be empty");
        }
    }

    public static TrChatMessage of(String... data) {
        return new TrChatMessage(List.of(data));
    }

    public String type() {
        return data.getFirst();
    }
}
