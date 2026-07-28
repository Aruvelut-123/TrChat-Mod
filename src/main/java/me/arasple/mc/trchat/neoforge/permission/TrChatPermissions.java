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

    private TrChatPermissions() {
    }

    public static void register(PermissionGatherEvent.Nodes event) {
        event.addNodes(GLOBAL, PRIVATE, ADMIN);
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
}
