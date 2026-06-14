package me.arasple.mc.trchat.module.internal.listener

import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.conf.file.Functions
import me.arasple.mc.trchat.module.display.ChatSession
import me.arasple.mc.trchat.module.display.channel.Channel
import me.arasple.mc.trchat.module.internal.data.PlayerData
import me.arasple.mc.trchat.util.Cooldowns
import org.bukkit.event.player.PlayerQuitEvent
import taboolib.common.platform.Ghost
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.EventPriority
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.platform.function.submit

/**
 * @author ItsFlicker
 * @since 2021/12/11 23:19
 */
@PlatformSide(Platform.BUKKIT)
object ListenerQuit {

    @Ghost
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    fun onQuit(e: PlayerQuitEvent) {
        val player = e.player
        val name = player.name

        Functions.commandController.get().values.forEach { it.baffle?.reset(name) }
        Cooldowns.COOLDOWNS.remove(player.uniqueId)

        Channel.channels.values.forEach { it.listeners -= name }
        ChatSession.removeSession(player)
        PlayerData.removeData(player)
        submit(delay = 10) {
            BukkitProxyManager.updateNames()
        }
    }

}
