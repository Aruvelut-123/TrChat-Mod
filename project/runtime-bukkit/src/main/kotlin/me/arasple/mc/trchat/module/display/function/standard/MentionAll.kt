package me.arasple.mc.trchat.module.display.function.standard

import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.conf.file.Functions
import me.arasple.mc.trchat.module.display.function.Function
import me.arasple.mc.trchat.module.display.function.StandardFunction
import me.arasple.mc.trchat.module.internal.script.Reaction
import me.arasple.mc.trchat.util.*
import org.bukkit.entity.Player
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.util.asList
import taboolib.common.util.resettableLazy
import taboolib.common5.util.parseMillis
import taboolib.module.chat.ComponentText
import taboolib.module.configuration.ConfigNode
import taboolib.module.configuration.ConfigNodeTransfer
import taboolib.platform.util.sendLang

/**
 * @author ItsFlicker
 * @since 2022/3/18 19:14
 */
@StandardFunction
@PlatformSide(Platform.BUKKIT)
object MentionAll : Function("MENTIONALL") {

    override val alias = "Mention-All"

    override val reaction by resettableLazy("functions") {
        Functions.conf["General.Mention-All.Action"]?.let { Reaction(it.asList()) }
    }

    @ConfigNode("General.Mention-All.Enabled", "function.yml")
    var enabled = true

    @ConfigNode("General.Mention-All.Permission", "function.yml")
    var permission = "trchat.function.mentionall"

    @ConfigNode("General.Mention-All.Notify", "function.yml")
    var notify = true

    @ConfigNode("General.Mention-All.Cooldown", "function.yml")
    val cooldown = ConfigNodeTransfer<String, Long> { parseMillis() }

    @ConfigNode("General.Mention-All.Keys", "function.yml")
    var keys = emptyList<String>()

    val keysRegex by resettableLazy("functions") {
        keys.map { Regex(Regex.escape(it), RegexOption.IGNORE_CASE) }
    }

    override fun createVariable(sender: Player, message: String): String {
        if (!enabled) {
            return message
        }
        var result = message
        keysRegex.forEach {
            result = result.replace(it) { "{{MENTIONALL:${push(sender.name)}}}" }
        }
        return result
    }

    override fun parseVariable(sender: Player, arg: String): ComponentText? {
        if (notify) {
            BukkitProxyManager.getPlayerNames().keys.filter { it != arg }.forEach {
                recordMention(it)
            }
        }
        return sender.getComponentFromLang("Function-Mention-All-Format", sender.name)
    }

    override fun canUse(sender: Player): Boolean {
        return sender.passPermission(permission)
    }

    override fun checkCooldown(sender: Player, message: String): Boolean {
        if (enabled && keys.any { message.contains(it, ignoreCase = true) } && !sender.hasPermission("trchat.bypass.mentionallcd")) {
            val mentionAllCooldown = sender.getCooldownLeft(CooldownType.MENTION_ALL)
            if (mentionAllCooldown > 0) {
                sender.sendLang("Cooldowns-Mention-All", mentionAllCooldown / 1000)
                return false
            } else {
                sender.updateCooldown(CooldownType.MENTION_ALL, cooldown.get())
            }
        }
        return true
    }

}