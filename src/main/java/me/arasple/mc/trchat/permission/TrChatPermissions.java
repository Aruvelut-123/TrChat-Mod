package me.arasple.mc.trchat.permission;

import net.minecraft.server.level.ServerPlayer;

import java.util.List;

//? if neoforge {
import net.neoforged.neoforge.server.permission.PermissionAPI;
import net.neoforged.neoforge.server.permission.events.PermissionGatherEvent;
import net.neoforged.neoforge.server.permission.nodes.PermissionNode;
import net.neoforged.neoforge.server.permission.nodes.PermissionTypes;
//? } else if forge {
import net.minecraftforge.server.permission.PermissionAPI;
import net.minecraftforge.server.permission.events.PermissionGatherEvent;
import net.minecraftforge.server.permission.nodes.PermissionNode;
import net.minecraftforge.server.permission.nodes.PermissionTypes;
//? }

//? if neoforge || forge {
public final class TrChatPermissions {

    public static final PermissionNode<Boolean> GLOBAL = new PermissionNode<>(
        "trchat", "global", PermissionTypes.BOOLEAN, (player, uuid, context) -> true
    );
    public static final PermissionNode<Boolean> PRIVATE = new PermissionNode<>(
        "trchat", "private", PermissionTypes.BOOLEAN, (player, uuid, context) -> true
    );
    public static final PermissionNode<Boolean> ADMIN = new PermissionNode<>(
        "trchat", "admin", PermissionTypes.BOOLEAN,
        //? if >=1.21.11 {
        (player, uuid, context) -> player != null && player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        )
        //? } else {
        (player, uuid, context) -> player != null && player.hasPermissions(2)
        //? }
    );
    public static final PermissionNode<Boolean> MENTION_ALL = restricted("function.mentionall");
    public static final PermissionNode<Boolean> INVENTORY_SHOW = restricted("function.inventoryshow");
    public static final PermissionNode<Boolean> ENDER_CHEST_SHOW = restricted("function.enderchestshow");
    public static final PermissionNode<Boolean> MUTE = restricted("mute");
    public static final PermissionNode<Boolean> SHADOW_MUTE = restricted("shadowmute");
    public static final PermissionNode<Boolean> PRIVATE_SPY = restricted("spy");
    public static final PermissionNode<Boolean> IGNORE = new PermissionNode<>(
        "trchat", "command.ignore", PermissionTypes.BOOLEAN, (player, uuid, context) -> true
    );
    public static final PermissionNode<Boolean> CHAT_COLOR_COMMAND = restricted("command.color");
    public static final PermissionNode<Boolean> CLEAR_CHAT = restricted("command.clear");
    public static final PermissionNode<Boolean> CHANNEL_OTHER = restricted("command.channel.other");
    public static final PermissionNode<Boolean> COMMAND_COOLDOWN_BYPASS = restricted("bypass.cmdcooldown");
    private static final List<PermissionNode<Boolean>> CHAT_COLORS = "0123456789abcdef".chars()
        .mapToObj(code -> new PermissionNode<>(
            "trchat", "color." + (char) code, PermissionTypes.BOOLEAN,
            //? if >=1.21.11 {
            (player, uuid, context) -> player != null && player.permissions().hasPermission(
                new net.minecraft.server.permissions.Permission.HasCommandLevel(
                    net.minecraft.server.permissions.PermissionLevel.byId(2)
                )
            )
            //? } else {
            (player, uuid, context) -> player != null && player.hasPermissions(2)
            //? }
        ))
        .toList();

    private TrChatPermissions() {
    }

    public static void register(PermissionGatherEvent.Nodes event) {
        event.addNodes(
            GLOBAL, PRIVATE, ADMIN, MENTION_ALL, INVENTORY_SHOW, ENDER_CHEST_SHOW,
            MUTE, SHADOW_MUTE, PRIVATE_SPY, IGNORE, CHAT_COLOR_COMMAND, CLEAR_CHAT,
            CHANNEL_OTHER, COMMAND_COOLDOWN_BYPASS
        );
        CHAT_COLORS.forEach(event::addNodes);
    }

    public static boolean check(ServerPlayer player, String permission) {
        if (permission == null || permission.isBlank()) {
            return true;
        }
        for (PermissionNode<?> node : PermissionAPI.getRegisteredNodes()) {
            if (node.getNodeName().equalsIgnoreCase(permission) && node.getType().equals(PermissionTypes.BOOLEAN)) {
                @SuppressWarnings("unchecked")
                PermissionNode<Boolean> booleanNode = (PermissionNode<Boolean>) node;
                return PermissionAPI.getPermission(player, booleanNode);
            }
        }
        //? if >=1.21.11 {
        return player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        );
        //? } else {
        return player.hasPermissions(2);
        //? }
    }

    private static PermissionNode<Boolean> restricted(String name) {
        return new PermissionNode<>(
            "trchat", name, PermissionTypes.BOOLEAN,
            //? if >=1.21.11 {
            (player, uuid, context) -> player != null && player.permissions().hasPermission(
                new net.minecraft.server.permissions.Permission.HasCommandLevel(
                    net.minecraft.server.permissions.PermissionLevel.byId(2)
                )
            )
            //? } else {
            (player, uuid, context) -> player != null && player.hasPermissions(2)
            //? }
        );
    }
}
//? } else {

/**
 * Fabric permission backend. Fabric has no built-in permission API, so TrChat
 * keeps its own node list: unrestricted nodes (chat, private, ignore) are always
 * granted, while administrative nodes require command level 2 (OP). This matches
 * the default behavior of the NeoForge backend for servers without a permission mod.
 */
public final class TrChatPermissions {

    /** Nodes granted to every player without an OP check. */
    private static final List<String> OPEN_NODES = List.of(
        "trchat.global",
        "trchat.private",
        "trchat.command.ignore"
    );

    private static final List<String> OPEN_PREFIXES = List.of(
        "trchat.color."
    );

    private TrChatPermissions() {
    }

    public static boolean check(ServerPlayer player, String permission) {
        if (permission == null || permission.isBlank()) {
            return true;
        }
        String node = permission.toLowerCase(java.util.Locale.ROOT);
        if (OPEN_NODES.contains(node)) {
            return true;
        }
        for (String prefix : OPEN_PREFIXES) {
            if (node.startsWith(prefix)) {
                return true;
            }
        }
        //? if >=1.21.11 {
        return player != null && player.permissions().hasPermission(
            new net.minecraft.server.permissions.Permission.HasCommandLevel(
                net.minecraft.server.permissions.PermissionLevel.byId(2)
            )
        );
        //? } else {
        return player != null && player.hasPermissions(2);
        //? }
    }
}
//? }