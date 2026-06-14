package me.arasple.mc.trchat.module.display.format.obj

import me.arasple.mc.trchat.module.adventure.parseMiniMessage
import me.arasple.mc.trchat.module.internal.hook.hookItemsAdder
import me.arasple.mc.trchat.module.internal.script.Condition
import me.arasple.mc.trchat.module.internal.script.kether.KetherHandler
import me.arasple.mc.trchat.util.color.colorify
import me.arasple.mc.trchat.util.isDragonCoreHooked
import me.arasple.mc.trchat.util.papiRegex
import me.arasple.mc.trchat.util.setPlaceholders
import org.bukkit.command.CommandSender
import org.bukkit.entity.Player
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.util.replaceWithOrder
import taboolib.module.chat.ComponentText
import taboolib.module.chat.Components
import taboolib.module.configuration.ConfigNode

/**
 * @author ItsFlicker
 * @since 2022/1/21 23:21
 */
@PlatformSide(Platform.BUKKIT)
open class Text(val content: String, val condition: Condition?) {

    val dynamic = papiRegex.containsMatchIn(content)

    open fun process(sender: CommandSender, vararg vars: String): ComponentText {
        var text = KetherHandler.parseInline(content, sender)
        if (sender is Player && dynamic) {
            text = text.setPlaceholders(sender)
        }
        text = hookItemsAdder.replaceFontImages(text, null)
        text = text.replaceWithOrder(*vars)
        return if (miniMessage) {
            text.parseMiniMessage()
        } else if (isDragonCoreHooked) {
            Components.text(text.colorify(), color = false)
        } else {
            Components.text(text.colorify())
        }
    }

    companion object {

        @ConfigNode("MiniMessage.Text", "settings.yml")
        var miniMessage = false
            private set

    }
}