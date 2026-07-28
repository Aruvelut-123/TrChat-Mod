package me.arasple.mc.trchat.neoforge.permission;

import net.minecraft.server.level.ServerPlayer;
import net.neoforged.neoforge.server.permission.PermissionAPI;
import net.neoforged.neoforge.server.permission.events.PermissionGatherEvent;
import net.neoforged.neoforge.server.permission.nodes.PermissionNode;
import net.neoforged.neoforge.server.permission.nodes.PermissionTypes;

public final class TrChatPermissions {

    public static final PermissionNode<Boolean> GLOBAL = new PermissionNode<>(
        "trchat", "global", PermissionTypes.BOOLEAN, (player, uuid, context) -> true
    );
    public static final PermissionNode<Boolean> PRIVATE = new PermissionNode<>(
        "trchat", "private", PermissionTypes.BOOLEAN, (player, uuid, context) -> true
    );
    public static final PermissionNode<Boolean> ADMIN = new PermissionNode<>(
        "trchat", "admin", PermissionTypes.BOOLEAN,
        (player, uuid, context) -> player != null && player.hasPermissions(2)
    );
    public static final PermissionNode<Boolean> MENTION_ALL = restricted("function.mentionall");
    public static final PermissionNode<Boolean> INVENTORY_SHOW = restricted("function.inventoryshow");
    public static final PermissionNode<Boolean> ENDER_CHEST_SHOW = restricted("function.enderchestshow");
    public static final PermissionNode<Boolean> MUTE = restricted("mute");
    public static final PermissionNode<Boolean> SHADOW_MUTE = restricted("shadowmute");
    public static final PermissionNode<Boolean> PRIVATE_SPY = restricted("spy");

    private TrChatPermissions() {
    }

    public static void register(PermissionGatherEvent.Nodes event) {
        event.addNodes(
            GLOBAL, PRIVATE, ADMIN, MENTION_ALL, INVENTORY_SHOW, ENDER_CHEST_SHOW,
            MUTE, SHADOW_MUTE, PRIVATE_SPY
        );
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
        return player.hasPermissions(2);
    }

    private static PermissionNode<Boolean> restricted(String name) {
        return new PermissionNode<>(
            "trchat", name, PermissionTypes.BOOLEAN,
            (player, uuid, context) -> player != null && player.hasPermissions(2)
        );
    }
}
