package me.arasple.mc.trchat.chat;

import net.minecraft.ChatFormatting;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.network.chat.Style;

public final class LegacyText {

    private LegacyText() {
    }

    public static Component parse(String input) {
        MutableComponent root = Component.empty();
        StringBuilder part = new StringBuilder();
        Style style = Style.EMPTY;

        for (int i = 0; i < input.length(); i++) {
            char current = input.charAt(i);
            if ((current == '&' || current == '\u00a7') && i + 1 < input.length()) {
                ChatFormatting formatting = ChatFormatting.getByCode(input.charAt(i + 1));
                if (formatting != null) {
                    append(root, part, style);
                    style = style.applyLegacyFormat(formatting);
                    i++;
                    continue;
                }
            }
            part.append(current);
        }
        append(root, part, style);
        return root;
    }

    public static String render(String format, String player, String displayName, String message, int serverId) {
        return format
            .replace("%player%", player)
            .replace("%display_name%", displayName)
            .replace("%message%", stripLegacyCodes(message))
            .replace("%server%", Integer.toString(serverId));
    }

    public static String stripLegacyCodes(String input) {
        StringBuilder result = new StringBuilder(input.length());
        for (int i = 0; i < input.length(); i++) {
            char current = input.charAt(i);
            if ((current == '&' || current == '\u00a7')
                && i + 1 < input.length()
                && ChatFormatting.getByCode(input.charAt(i + 1)) != null) {
                i++;
            } else {
                result.append(current);
            }
        }
        return result.toString();
    }

    private static void append(MutableComponent root, StringBuilder part, Style style) {
        if (!part.isEmpty()) {
            root.append(Component.literal(part.toString()).setStyle(style));
            part.setLength(0);
        }
    }
}
