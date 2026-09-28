package me.arasple.mc.trchat.channel;

import me.arasple.mc.trchat.permission.TrChatPermissions;
import net.minecraft.server.level.ServerPlayer;

import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class ConditionEvaluator {

    private static final Pattern PERMISSION = Pattern.compile(
        "(?:perm|permission)\\s+[\"']?([^\"'\\s]+)[\"']?",
        Pattern.CASE_INSENSITIVE
    );

    private ConditionEvaluator() {
    }

    public static boolean test(String condition, ServerPlayer player) {
        if (condition == null || condition.isBlank() || "~".equals(condition)) {
            return true;
        }
        String normalized = condition.trim();
        if (normalized.equalsIgnoreCase("player op") || normalized.equalsIgnoreCase("player is op")) {
            //? if >=1.21.11 {
            return player != null && player.permissions().hasPermission(
                new net.minecraft.server.permissions.Permission.HasCommandLevel(
                    net.minecraft.server.permissions.PermissionLevel.ADMINS
                )
            );
            //? } else {
            return player != null && player.hasPermissions(2);
            //? }
        }
        Matcher permission = PERMISSION.matcher(normalized);
        if (permission.matches()) {
            String node = permission.group(1);
            if (node.startsWith("*")) {
                node = node.substring(1);
            }
            return player != null && !node.isBlank() && TrChatPermissions.check(player, node);
        }
        if (normalized.startsWith("!")) {
            return !test(normalized.substring(1).trim(), player);
        }
        return false;
    }
}
