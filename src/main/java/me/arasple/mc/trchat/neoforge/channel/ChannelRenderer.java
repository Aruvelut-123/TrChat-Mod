package me.arasple.mc.trchat.neoforge.channel;

import me.arasple.mc.trchat.neoforge.chat.LegacyText;
import me.arasple.mc.trchat.neoforge.placeholder.PlaceholderResolver;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.HoverEvent;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.network.chat.Style;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.level.ServerPlayer;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class ChannelRenderer {

    public static final String MESSAGE_COLOR = "trchat_message_color";

    private final PlaceholderResolver placeholders;

    public ChannelRenderer(PlaceholderResolver placeholders) {
        this.placeholders = placeholders;
    }

    public Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer player,
        String message,
        Map<String, String> local
    ) {
        return render(channel, audience, player, message, local, null);
    }

    public Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer player,
        Component messageComponent,
        String rawMessage,
        Map<String, String> local
    ) {
        return render(channel, audience, player, rawMessage, local, messageComponent);
    }

    private Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer player,
        String message,
        Map<String, String> local,
        Component messageComponent
    ) {
        List<ChannelDefinition.Format> candidates = switch (audience) {
            case SENDER -> channel.senderFormats();
            case RECEIVER -> channel.receiverFormats();
            case CONSOLE -> channel.consoleFormats().isEmpty() ? channel.formats() : channel.consoleFormats();
            case CHAT -> channel.formats();
        };
        ChannelDefinition.Format format = candidates.stream()
            .filter(candidate -> ConditionEvaluator.test(candidate.condition(), player))
            .findFirst()
            .orElse(null);
        if (format == null) {
            Component fallback = LegacyText.parse(placeholders.resolve(message, player, local));
            return new Rendered(fallback, fallback.getString());
        }

        MutableComponent result = Component.empty();
        appendGroups(result, format.prefix(), player, local);

        ChannelDefinition.MessagePart messagePart = format.message();
        String color = local.getOrDefault(MESSAGE_COLOR, messagePart.defaultColor());
        if (color.startsWith("&") || color.startsWith("§")) {
            color = color.substring(1);
        }
        net.minecraft.ChatFormatting defaultFormatting = net.minecraft.ChatFormatting.getByCode(
            color.isEmpty() ? 'f' : color.charAt(0)
        );
        if (defaultFormatting == null) {
            defaultFormatting = net.minecraft.ChatFormatting.WHITE;
        }
        net.minecraft.ChatFormatting finalFormatting = defaultFormatting;
        String cleanMessage = LegacyText.stripLegacyCodes(placeholders.resolve(message, player, local));
        MutableComponent body = messageComponent == null
            ? LegacyText.parse("&" + color + cleanMessage).copy()
            : Component.empty()
                .withStyle(style -> style.applyLegacyFormat(
                    finalFormatting
                ))
                .append(messageComponent.copy());
        if (!messagePart.hover().isBlank()) {
            Component hover = LegacyText.parse(placeholders.resolve(messagePart.hover(), player, local));
            body.withStyle(style -> style.withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_TEXT, hover)));
        }
        result.append(body);

        appendGroups(result, format.suffix(), player, local);
        return new Rendered(result, result.getString());
    }

    private void appendGroups(
        MutableComponent output,
        LinkedHashMap<String, List<ChannelDefinition.ComponentPart>> groups,
        ServerPlayer player,
        Map<String, String> local
    ) {
        for (List<ChannelDefinition.ComponentPart> variants : groups.values()) {
            variants.stream()
                .filter(part -> ConditionEvaluator.test(part.condition(), player))
                .findFirst()
                .ifPresent(part -> output.append(component(part, player, local)));
        }
    }

    private Component component(
        ChannelDefinition.ComponentPart part,
        ServerPlayer player,
        Map<String, String> local
    ) {
        MutableComponent component = LegacyText.parse(placeholders.resolve(part.text(), player, local)).copy();
        Style style = component.getStyle();

        if (!part.hover().isBlank()) {
            Component hover = LegacyText.parse(placeholders.resolve(part.hover(), player, local));
            style = style.withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_TEXT, hover));
        }
        ClickEvent click = clickEvent(part, player, local);
        if (click != null) {
            style = style.withClickEvent(click);
        }
        if (!part.insertion().isBlank()) {
            style = style.withInsertion(placeholders.resolve(part.insertion(), player, local));
        }
        if (!part.font().isBlank()) {
            style = style.withFont(ResourceLocation.parse(placeholders.resolve(part.font(), player, local)));
        }
        return component.setStyle(style);
    }

    private ClickEvent clickEvent(
        ChannelDefinition.ComponentPart part,
        ServerPlayer player,
        Map<String, String> local
    ) {
        if (!part.suggest().isBlank()) {
            return new ClickEvent(
                ClickEvent.Action.SUGGEST_COMMAND,
                placeholders.resolve(part.suggest(), player, local)
            );
        }
        if (!part.command().isBlank()) {
            return new ClickEvent(
                ClickEvent.Action.RUN_COMMAND,
                placeholders.resolve(part.command(), player, local)
            );
        }
        if (!part.url().isBlank()) {
            return new ClickEvent(ClickEvent.Action.OPEN_URL, placeholders.resolve(part.url(), player, local));
        }
        if (!part.copy().isBlank()) {
            return new ClickEvent(ClickEvent.Action.COPY_TO_CLIPBOARD, placeholders.resolve(part.copy(), player, local));
        }
        if (!part.file().isBlank()) {
            return new ClickEvent(ClickEvent.Action.OPEN_FILE, placeholders.resolve(part.file(), player, local));
        }
        return null;
    }

    public enum Audience {
        CHAT,
        SENDER,
        RECEIVER,
        CONSOLE
    }

    public record Rendered(Component component, String fallback) {
    }
}
