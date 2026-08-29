package me.arasple.mc.trchat.channel;

import me.arasple.mc.trchat.chat.LegacyText;
import me.arasple.mc.trchat.config.SpecialChars;
import me.arasple.mc.trchat.placeholder.PlaceholderResolver;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.Component;
//? if >=1.21.11 {
import net.minecraft.network.chat.FontDescription;
//? }
import net.minecraft.network.chat.HoverEvent;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.network.chat.Style;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.level.ServerPlayer;

import java.net.URI;
import java.net.URISyntaxException;
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
        ServerPlayer subject,
        String message,
        Map<String, String> local
    ) {
        return render(channel, audience, subject, subject, message, local, null);
    }

    public Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer subject,
        ServerPlayer viewer,
        String message,
        Map<String, String> local
    ) {
        return render(channel, audience, subject, viewer, message, local, null);
    }

    public Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer subject,
        Component messageComponent,
        String rawMessage,
        Map<String, String> local
    ) {
        return render(channel, audience, subject, subject, rawMessage, local, messageComponent);
    }

    public Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer subject,
        ServerPlayer viewer,
        Component messageComponent,
        String rawMessage,
        Map<String, String> local
    ) {
        return render(channel, audience, subject, viewer, rawMessage, local, messageComponent);
    }

    private Rendered render(
        ChannelDefinition channel,
        Audience audience,
        ServerPlayer subject,
        ServerPlayer viewer,
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
            .filter(candidate -> ConditionEvaluator.test(candidate.condition(), subject))
            .findFirst()
            .orElse(null);
        if (format == null) {
            Component fallback = LegacyText.parse(placeholders.resolve(message, subject, viewer, local));
            return new Rendered(fallback, fallback.getString());
        }

        MutableComponent result = Component.empty();
        appendGroups(result, format.prefix(), subject, viewer, local);

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
        String cleanMessage = LegacyText.stripLegacyCodes(placeholders.resolve(message, subject, viewer, local));
        String bodyText = "&" + color + cleanMessage;
        if (messagePart.specialCharsEnabled()
            && messageComponent == null
            && SpecialChars.hasSpecialChars(cleanMessage)) {
            bodyText = SpecialChars.wrapSpecialChars(
                bodyText,
                messagePart.specialCharsColor(),
                "&" + color
            );
        }
        MutableComponent body = messageComponent == null
            ? LegacyText.parse(bodyText).copy()
            : Component.empty()
                .withStyle(style -> style.applyLegacyFormat(
                    finalFormatting
                ))
                .append(messageComponent.copy());
        if (!messagePart.hover().isBlank()) {
            Component hover = LegacyText.parse(placeholders.resolve(messagePart.hover(), subject, viewer, local));
            //? if >=1.21.11 {
            body.withStyle(style -> style.withHoverEvent(new HoverEvent.ShowText(hover)));
            //? } else {
            body.withStyle(style -> style.withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_TEXT, hover)));
            //? }
        }
        result.append(body);

        appendGroups(result, format.suffix(), subject, viewer, local);
        return new Rendered(result, result.getString());
    }

    private void appendGroups(
        MutableComponent output,
        LinkedHashMap<String, List<ChannelDefinition.ComponentPart>> groups,
        ServerPlayer subject,
        ServerPlayer viewer,
        Map<String, String> local
    ) {
        for (List<ChannelDefinition.ComponentPart> variants : groups.values()) {
            variants.stream()
                .filter(part -> ConditionEvaluator.test(part.condition(), subject))
                .findFirst()
                .ifPresent(part -> output.append(component(part, subject, viewer, local)));
        }
    }

    private Component component(
        ChannelDefinition.ComponentPart part,
        ServerPlayer subject,
        ServerPlayer viewer,
        Map<String, String> local
    ) {
        MutableComponent component = LegacyText.parse(
            placeholders.resolve(part.text(), subject, viewer, local)
        ).copy();
        Style style = component.getStyle();

        if (!part.hover().isBlank()) {
            Component hover = LegacyText.parse(placeholders.resolve(part.hover(), subject, viewer, local));
            //? if >=1.21.11 {
            style = style.withHoverEvent(new HoverEvent.ShowText(hover));
            //? } else {
            style = style.withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_TEXT, hover));
            //? }
        }
        ClickEvent click = clickEvent(part, subject, viewer, local);
        if (click != null) {
            style = style.withClickEvent(click);
        }
        if (!part.insertion().isBlank()) {
            style = style.withInsertion(placeholders.resolve(part.insertion(), subject, viewer, local));
        }
        if (!part.font().isBlank()) {
            //? if >=1.21.11 {
            style = style.withFont(new FontDescription.Resource(ResourceLocation.parse(
                placeholders.resolve(part.font(), subject, viewer, local)
            )));
            //? } else {
            style = style.withFont(ResourceLocation.parse(
                placeholders.resolve(part.font(), subject, viewer, local)
            ));
            //? }
        }
        return component.setStyle(style);
    }

    private ClickEvent clickEvent(
        ChannelDefinition.ComponentPart part,
        ServerPlayer subject,
        ServerPlayer viewer,
        Map<String, String> local
    ) {
        if (!part.suggest().isBlank()) {
            //? if >=1.21.11 {
            return new ClickEvent.SuggestCommand(
                placeholders.resolve(part.suggest(), subject, viewer, local)
            );
            //? } else {
            return new ClickEvent(
                ClickEvent.Action.SUGGEST_COMMAND,
                placeholders.resolve(part.suggest(), subject, viewer, local)
            );
            //? }
        }
        if (!part.command().isBlank()) {
            //? if >=1.21.11 {
            return new ClickEvent.RunCommand(
                placeholders.resolve(part.command(), subject, viewer, local)
            );
            //? } else {
            return new ClickEvent(
                ClickEvent.Action.RUN_COMMAND,
                placeholders.resolve(part.command(), subject, viewer, local)
            );
            //? }
        }
        if (!part.url().isBlank()) {
            String resolved = placeholders.resolve(part.url(), subject, viewer, local).trim();
            int space = resolved.indexOf(' ');
            String url = space < 0 ? resolved : resolved.substring(0, space);
            if (!url.isBlank() && isValidUrl(url)) {
                //? if >=1.21.11 {
                return new ClickEvent.OpenUrl(URI.create(url));
                //? } else {
                return new ClickEvent(
                    ClickEvent.Action.OPEN_URL,
                    url
                );
                //? }
            }
            return null;
        }
        if (!part.copy().isBlank()) {
            //? if >=1.21.11 {
            return new ClickEvent.CopyToClipboard(
                placeholders.resolve(part.copy(), subject, viewer, local)
            );
            //? } else {
            return new ClickEvent(
                ClickEvent.Action.COPY_TO_CLIPBOARD,
                placeholders.resolve(part.copy(), subject, viewer, local)
            );
            //? }
        }
        if (!part.file().isBlank()) {
            //? if >=1.21.11 {
            return new ClickEvent.OpenFile(
                placeholders.resolve(part.file(), subject, viewer, local)
            );
            //? } else {
            return new ClickEvent(
                ClickEvent.Action.OPEN_FILE,
                placeholders.resolve(part.file(), subject, viewer, local)
            );
            //? }
        }
        return null;
    }

    public enum Audience {
        CHAT,
        SENDER,
        RECEIVER,
        CONSOLE
    }

    private static boolean isValidUrl(String url) {
        try {
            new URI(url);
            return true;
        } catch (URISyntaxException exception) {
            return false;
        }
    }

    public record Rendered(Component component, String fallback) {
    }
}
