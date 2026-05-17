package me.arasple.mc.trchat.util

import me.arasple.mc.trchat.api.impl.BukkitComponentManager
import me.arasple.mc.trchat.module.display.ChatSession
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.module.internal.command.main.CommandMute
import me.arasple.mc.trchat.module.internal.data.PlayerData
import me.arasple.mc.trchat.module.internal.script.Condition
import me.arasple.mc.trchat.module.internal.script.kether.KetherHandler
import me.clip.placeholderapi.PlaceholderAPI
import org.bukkit.Bukkit
import org.bukkit.OfflinePlayer
import org.bukkit.command.CommandSender
import org.bukkit.entity.Player
import taboolib.common.util.sync
import taboolib.common.util.unsafeLazy
import taboolib.module.chat.ComponentText
import taboolib.module.nms.MinecraftVersion
import taboolib.platform.Folia
import taboolib.platform.util.sendLang

val isDragonCoreHooked by unsafeLazy { Bukkit.getPluginManager().isPluginEnabled("DragonCore") && MinecraftVersion.isLower(MinecraftVersion.V1_16)  }

fun String?.toCondition() = if (this == null) Condition.EMPTY else Condition(this)

fun Condition?.pass(commandSender: CommandSender): Boolean {
    return if (commandSender is Player) {
        this?.eval(commandSender) != false
    } else {
        true
    }
}

fun Player.passPermission(permission: String?): Boolean {
    return permission.isNullOrEmpty()
            || permission.equals("null", ignoreCase = true)
            || permission.equals("none", ignoreCase = true)
            || hasPermission(permission)
}

fun String.setPlaceholders(sender: CommandSender?): String {
    try {
        return setPlaceholders(sender, this)
    } catch (t: Throwable) {
        if (t is UnsupportedOperationException && Folia.isFolia) {
            val result = sync {
                runCatching { setPlaceholders(sender, this) }.getOrNull()
            }
            if (result != null) return result
        }
        t.print("Error occurred when parsing placeholder! This is not a bug of TrChat! 这很可能不是 TrChat 的问题!")
        return this
    }
}

private fun setPlaceholders(sender: CommandSender?, text: String): String {
    return if (sender is Player || sender is OfflinePlayer) {
        PlaceholderAPI.setPlaceholders(sender, text)
    } else {
        PlaceholderAPI.setPlaceholders(null, text)
    }
}

fun String.parseInline(sender: CommandSender, vars: Map<String, Any> = emptyMap()): String {
    return KetherHandler.parseInline(this, sender, vars)
}

inline val Player.session get() = ChatSession.getSession(this)

inline val OfflinePlayer.data get() = PlayerData.getData(this)

fun Player.checkMute(hint: Boolean = true): Boolean {
    if (TrChatBukkit.isGlobalMuting && !hasPermission("trchat.bypass.globalmute")) {
        if (hint) sendLang("General-Global-Muting")
        return false
    }
    val data = data
    if (data.isMuted) {
        if (hint) sendLang("General-Muted", CommandMute.muteDateFormat.format(data.muteTime), data.muteReason)
        return false
    }
    return true
}

fun Any.sendComponent(sender: Any?, component: ComponentText) {
    BukkitComponentManager.sendComponent(this, component, sender)
}

fun Player.getCooldownLeft(type: CooldownType) = Cooldowns.getCooldownLeft(uniqueId, type.alias)

fun Player.isInCooldown(type: CooldownType) = Cooldowns.isInCooldown(uniqueId, type.alias)

fun Player.updateCooldown(type: CooldownType, lasts: Long) = Cooldowns.updateCooldown(uniqueId, type.alias, lasts)

fun Player.getCooldownLeft(type: String) = Cooldowns.getCooldownLeft(uniqueId, type)

fun Player.isInCooldown(type: String) = Cooldowns.isInCooldown(uniqueId, type)

fun Player.updateCooldown(type: String, lasts: Long) = Cooldowns.updateCooldown(uniqueId, type, lasts)