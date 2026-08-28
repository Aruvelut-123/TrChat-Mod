package me.arasple.mc.trchat.compat;

import me.arasple.mc.trchat.channel.ChannelDefinition;
import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;

class E33ChatCompatTest {

    @Test
    void derivesTemplatesFromConfiguredPublicAndPrivateFormats() {
        LinkedHashMap<String, Object> publicPrefix = new LinkedHashMap<>();
        publicPrefix.put("world", Map.of("text", "&8[&3%player_world%&8] "));
        publicPrefix.put("player", List.of(
            Map.of("condition", "player op", "text", "&c<%player_name%>"),
            Map.of("text", "&7<%player_name%>")
        ));
        publicPrefix.put("separator", Map.of("text", " &f-> "));
        ChannelDefinition publicChannel = ChannelDefinition.from("Trade", Map.of(
            "Formats", List.of(Map.of(
                "prefix", publicPrefix,
                "msg", Map.of("default-color", "f")
            ))
        ));
        ChannelDefinition privateChannel = ChannelDefinition.from("Private", Map.of(
            "Options", Map.of("Private", true),
            "Sender", List.of(Map.of(
                "prefix", Map.of("main", Map.of(
                    "text", "&8[&a%player_name% &7=> %trchat_toplayer%&8] "
                )),
                "msg", Map.of("default-color", "f")
            )),
            "Receiver", List.of(Map.of(
                "prefix", Map.of("main", Map.of(
                    "text", "&8[&6%player_name% &7<= %trchat_toplayer%&8] "
                )),
                "msg", Map.of("default-color", "f")
            ))
        ));

        E33ChatCompat.Templates templates = E33ChatCompat.deriveTemplates(
            List.of(publicChannel, privateChannel)
        );

        assertEquals(
            List.of("{prefix}] <{display_name}> -> {content}"),
            templates.publicChat()
        );
        assertEquals(
            List.of(
                "{prefix}[{sender} => {target}] {content}",
                "{prefix}[{sender} <= {target}] {content}"
            ),
            templates.whisper()
        );
    }

    @Test
    void skipsLayoutsThatCannotSeparatePlayerNameFromContent() {
        ChannelDefinition channel = ChannelDefinition.from("Broken", Map.of(
            "Formats", List.of(Map.of(
                "prefix", Map.of("player", Map.of("text", "%player_name%")),
                "msg", Map.of("default-color", "f")
            ))
        ));

        assertEquals(List.of(), E33ChatCompat.deriveTemplates(List.of(channel)).publicChat());
    }
}
