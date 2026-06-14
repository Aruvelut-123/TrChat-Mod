package me.arasple.mc.trchat.module.internal.listener

import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.display.channel.Channel
import me.arasple.mc.trchat.module.internal.service.Updater
import me.arasple.mc.trchat.util.data
import me.arasple.mc.trchat.util.session
import org.bukkit.event.player.PlayerJoinEvent
import taboolib.common.platform.Ghost
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.EventPriority
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.platform.function.adaptPlayer
import taboolib.common.platform.function.submit
import taboolib.common.platform.function.submitAsync

/**
 * @author ItsFlicker
 * @since 2021/12/11 23:19
 */
@PlatformSide(Platform.BUKKIT)
object ListenerJoin {

    @Ghost
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    fun onJoin(e: PlayerJoinEvent) {
        val player = e.player

        submitAsync {
            player.data
            player.session
        }

        submit(delay = 20) {
            if (!player.isOnline) return@submit
            Channel.channels.values.filter { it.settings.alwaysListen }.forEach {
                if (it.canListen(player)) {
                    it.listeners.add(player.name)
                }
            }
            Updater.notifyPlayer(adaptPlayer(player))
            BukkitProxyManager.updateNames()
        }
    }
}
