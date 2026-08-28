package me.arasple.mc.trchat.compat;

import me.arasple.mc.trchat.TrChatMod;
import me.arasple.mc.trchat.channel.ChannelDefinition;
import net.neoforged.fml.ModList;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Runtime-only format bridge for E33Chat's optional server component. */
public final class E33ChatCompat {

    private static final String MOD_ID = "e33chat";
    private static final int MAX_LAYOUTS_PER_FORMAT = 128;
    private static final Pattern PLACEHOLDER = Pattern.compile("%[^%]+%");
    private static final Pattern IDENTITY = Pattern.compile(
        "%(player_name|player_displayname|trchat_toplayer)%",
        Pattern.CASE_INSENSITIVE
    );

    private static boolean warningLogged;
    private static volatile boolean active;

    private E33ChatCompat() {
    }

    public static boolean applyTemplates(Collection<ChannelDefinition> channels) {
        if (!ModList.get().isLoaded(MOD_ID)) {
            active = false;
            return false;
        }
        Templates templates = deriveTemplates(channels);
        try {
            Class<?> config = Class.forName("com.niuqu.chatbubble.ChatServerConfig");
            setConfigValue(config, "CHAT_TEMPLATES", templates.publicChat());
            setConfigValue(config, "WHISPER_TEMPLATES", templates.whisper());

            Class<?> listener = Class.forName("com.niuqu.chatbubble.ChatServerListener");
            listener.getMethod("broadcastServerConfig").invoke(null);
            active = true;
            return true;
        } catch (ReflectiveOperationException | LinkageError exception) {
            active = false;
            if (!warningLogged) {
                warningLogged = true;
                TrChatMod.LOGGER.warn(
                    "E33Chat was detected, but its format templates could not be configured: {}",
                    exception.getMessage()
                );
            }
            return false;
        }
    }

    public static boolean isActive() {
        return active;
    }

    static Templates deriveTemplates(Collection<ChannelDefinition> channels) {
        Set<String> publicChat = new LinkedHashSet<>();
        Set<String> whisper = new LinkedHashSet<>();
        for (ChannelDefinition channel : channels) {
            if (channel.options().privateChannel()) {
                addTemplates(channel.senderFormats(), whisper, E33ChatCompat::privateTemplate);
                addTemplates(channel.receiverFormats(), whisper, E33ChatCompat::privateTemplate);
            } else {
                addTemplates(channel.formats(), publicChat, E33ChatCompat::publicTemplate);
            }
        }
        return new Templates(List.copyOf(publicChat), List.copyOf(whisper));
    }

    private static void addTemplates(
        List<ChannelDefinition.Format> formats,
        Set<String> output,
        TemplateFactory factory
    ) {
        for (ChannelDefinition.Format format : formats) {
            for (String layout : layouts(format)) {
                String template = factory.create(layout);
                if (template != null) {
                    output.add(template);
                }
            }
        }
    }

    private static List<String> layouts(ChannelDefinition.Format format) {
        List<String> layouts = List.of("");
        for (List<ChannelDefinition.ComponentPart> variants : format.prefix().values()) {
            List<String> expanded = new ArrayList<>();
            boolean full = false;
            for (String layout : layouts) {
                for (ChannelDefinition.ComponentPart part : variants) {
                    expanded.add(layout + part.text());
                    if (expanded.size() >= MAX_LAYOUTS_PER_FORMAT) {
                        full = true;
                        break;
                    }
                }
                if (full) {
                    break;
                }
            }
            layouts = expanded;
        }
        return layouts;
    }

    private static String publicTemplate(String rawLayout) {
        String layout = stripLegacyCodes(rawLayout);
        Matcher identity = IDENTITY.matcher(layout);
        IdentityMatch player = null;
        while (identity.find()) {
            if (!identity.group(1).equalsIgnoreCase("trchat_toplayer")) {
                if (player != null) {
                    return null;
                }
                player = new IdentityMatch(identity.start(), identity.end(), "{display_name}");
            }
        }
        if (player == null) {
            return null;
        }

        String before = literalTail(layout.substring(0, player.start()));
        String separator = literalTail(layout.substring(player.end()));
        if (separator.isEmpty()) {
            return null;
        }
        return "{prefix}" + before + player.field() + separator + "{content}";
    }

    private static String privateTemplate(String rawLayout) {
        String layout = stripLegacyCodes(rawLayout);
        Matcher matcher = IDENTITY.matcher(layout);
        List<IdentityMatch> identities = new ArrayList<>();
        Set<String> fields = new LinkedHashSet<>();
        while (matcher.find()) {
            String field = matcher.group(1).equalsIgnoreCase("trchat_toplayer")
                ? "{target}"
                : "{sender}";
            if (!fields.add(field)) {
                return null;
            }
            identities.add(new IdentityMatch(matcher.start(), matcher.end(), field));
        }
        if (identities.isEmpty()) {
            return null;
        }

        StringBuilder template = new StringBuilder("{prefix}");
        int cursor = 0;
        for (IdentityMatch identity : identities) {
            template.append(literalTail(layout.substring(cursor, identity.start())));
            template.append(identity.field());
            cursor = identity.end();
        }
        String separator = literalTail(layout.substring(cursor));
        if (separator.isEmpty()) {
            return null;
        }
        return template.append(separator).append("{content}").toString();
    }

    private static String literalTail(String text) {
        Matcher matcher = PLACEHOLDER.matcher(text);
        int start = 0;
        while (matcher.find()) {
            start = matcher.end();
        }
        return text.substring(start);
    }

    private static String stripLegacyCodes(String text) {
        StringBuilder stripped = new StringBuilder(text.length());
        for (int index = 0; index < text.length(); index++) {
            char current = text.charAt(index);
            if ((current == '&' || current == '§')
                && index + 1 < text.length()
                && "0123456789abcdefklmnorx".indexOf(
                    Character.toLowerCase(text.charAt(index + 1))
                ) >= 0) {
                index++;
                continue;
            }
            stripped.append(current);
        }
        return stripped.toString();
    }

    private static void setConfigValue(Class<?> owner, String fieldName, List<String> templates)
        throws ReflectiveOperationException {
        Field field = owner.getField(fieldName);
        Object configValue = field.get(null);
        Method setter = configValue.getClass().getMethod("set", Object.class);
        setter.invoke(configValue, templates);
    }

    record Templates(List<String> publicChat, List<String> whisper) {
    }

    private record IdentityMatch(int start, int end, String field) {
    }

    @FunctionalInterface
    private interface TemplateFactory {
        String create(String layout);
    }
}
