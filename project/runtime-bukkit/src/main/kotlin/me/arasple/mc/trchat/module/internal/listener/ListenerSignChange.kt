package me.arasple.mc.trchat.module.internal.listener

import io.papermc.paper.event.player.PlayerOpenSignEvent
import me.arasple.mc.trchat.TrChat
import me.arasple.mc.trchat.module.adventure.toAdventure
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.util.color.MessageColors
import me.arasple.mc.trchat.util.data
import me.arasple.mc.trchat.util.parseSimple
import me.arasple.mc.trchat.util.session
import org.bukkit.entity.Player
import org.bukkit.event.block.SignChangeEvent
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.EventPriority
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.platform.function.adaptPlayer
import taboolib.module.configuration.ConfigNode
import taboolib.platform.util.sendLang

/**
 * @author ItsFlicker
 * @date 2019/8/15 21:18
 */
@PlatformSide(Platform.BUKKIT)
object ListenerSignChange {

    @ConfigNode("Enable.Sign", "filter.yml")
    var filter = true
        private set

    @ConfigNode("Chat.Permission-Check.Sign", "settings.yml")
    var signEditPermissionCheck = false

    @ConfigNode("Color.Sign", "settings.yml")
    var color = true
        private set

    @ConfigNode("Simple-Component.Sign", "settings.yml")
    var simple = false
        private set

    @Suppress("Deprecation")
    @SubscribeEvent(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    fun onSignChange(e: SignChangeEvent) {
        val p = e.player

        var index = -1
        for (origin in e.lines) {
            index++
            if (origin.isBlank()) continue
            var edited = origin
            if (filter) {
                edited = TrChat.api().getFilterManager().filter(origin, adaptPlayer(p)).filtered
                if (edited != origin) {
                    e.setLine(index, edited)
                }
            }
            if (simple && TrChatBukkit.isPaperEnv && p.hasPermission("trchat.simple.sign")) {
                e.line(index, edited.parseSimple().toAdventure())
            } else if (color) {
                val colored = MessageColors.replaceWithPermission(p, edited, MessageColors.Type.SIGN)
                if (colored != edited) {
                    e.setLine(index, colored)
                }
            }
        }
    }

    @SubscribeEvent(priority = EventPriority.LOW, ignoreCancelled = true)
    fun onPlayerOpenSign(e: PlayerOpenSignEvent) {
        if (!signEditPermissionCheck) return
        val player = e.player
        if (!player.hasPermission("trchat.bypass.signedit") && !canSpeak(player)) {
            e.isCancelled = true
            player.sendLang("Sign-Edit-No-Permission")
        }
    }

    private fun canSpeak(player: Player): Boolean {
        if (TrChatBukkit.isGlobalMuting && !player.hasPermission("trchat.bypass.globalmute")) return false
        if (player.data.isMuted) return false
        val channel = player.session.getChannel()
        return channel == null || channel.canSpeak(player)
    }
}