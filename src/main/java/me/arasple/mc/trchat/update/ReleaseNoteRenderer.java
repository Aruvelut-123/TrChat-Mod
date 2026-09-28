package me.arasple.mc.trchat.update;

import net.minecraft.ChatFormatting;
import net.minecraft.network.chat.Component;

final class ReleaseNoteRenderer {

    private ReleaseNoteRenderer() {
    }

    static Component render(String line) {
        ReleaseNotes.FormattedLine formatted = ReleaseNotes.parseLine(line);
        return switch (formatted.type()) {
            case LEVEL_TWO_HEADING -> Component.literal(formatted.text())
                .withStyle(ChatFormatting.AQUA, ChatFormatting.BOLD);
            case LEVEL_THREE_HEADING -> Component.literal(formatted.text())
                .withStyle(ChatFormatting.YELLOW, ChatFormatting.BOLD);
            case LIST_ITEM -> Component.literal("  • ")
                .withStyle(ChatFormatting.DARK_GRAY)
                .append(Component.literal(formatted.text()).withStyle(ChatFormatting.GRAY));
            case TEXT -> Component.literal(formatted.text()).withStyle(ChatFormatting.GRAY);
            case BLANK -> Component.empty();
        };
    }
}
