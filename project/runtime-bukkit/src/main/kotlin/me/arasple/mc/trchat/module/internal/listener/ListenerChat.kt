@file:Suppress("Deprecation")

package me.arasple.mc.trchat.module.internal.listener

import io.papermc.paper.event.player.AsyncChatEvent
import me.arasple.mc.trchat.module.adventure.toNative
import me.arasple.mc.trchat.module.display.channel.Channel
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.util.session
import org.bukkit.entity.Player
import org.bukkit.event.player.AsyncPlayerChatEvent
import taboolib.common.platform.Ghost
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.EventPriority
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.util.resettableLazy
import taboolib.module.configuration.ConfigNode
import java.util.*
import java.util.concurrent.ConcurrentHashMap

/**
 * @author ItsFlicker
 * @date 2019/11/30 12:10
 */
@PlatformSide(Platform.BUKKIT)
object ListenerChat {

    @ConfigNode("Options.Always-Cancel-Chat-Event", "settings.yml")
    var cancelEvent = false
        private set

    @ConfigNode("Options.Disabled-Worlds", "settings.yml")
    var disabledWorlds = emptyList<String>()
        private set

    private val cachePrefix = ConcurrentHashMap<UUID, String>()

    private val worldPatterns by resettableLazy("settings") {
        disabledWorlds.map { Regex(it, RegexOption.IGNORE_CASE) }
    }

    private fun isDisabledWorld(player: Player): Boolean {
        val world = player.world?.name ?: return false
        return worldPatterns.any { it.matches(world) }
    }

    @Ghost
    @SubscribeEvent(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    fun onPaperChat(e: AsyncChatEvent) {
        if (!TrChatBukkit.isPaperEnv) return
        if (isDisabledWorld(e.player)) return
        if (cancelEvent) {
            e.isCancelled = true
        } else {
            e.viewers().clear()
        }

        val player = e.player
        val session = player.session

        cachePrefix.remove(player.uniqueId)?.let {
            val channel = Channel.channels[it] ?: return@let
            if (channel.settings.isPrivate) e.isCancelled = true
            channel.execute(player, e.message().toNative(), toConsole = true)
            return
        }

        session.getChannel()?.let {
            if (it.settings.isPrivate) e.isCancelled = true
            it.execute(player, e.message().toNative(), toConsole = true)
        }
    }

    @SubscribeEvent(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    fun onBukkitChat(e: AsyncPlayerChatEvent) {
        if (isDisabledWorld(e.player)) return
        // 提前判定前缀
        if (TrChatBukkit.isPaperEnv) {
            Channel.channels.values.forEach { channel ->
                channel.bindings.prefix?.forEach {
                    if (e.message.startsWith(it, ignoreCase = true)) {
                        cachePrefix[e.player.uniqueId] = channel.id
                        e.message = e.message.substring(it.length)
                        return
                    }
                }
            }
            return
        }
        if (e.isCancelled) return
        if (cancelEvent) {
            e.isCancelled = true
        } else {
            e.recipients.clear()
        }
        val player = e.player
        val session = player.session

        Channel.channels.values.forEach { channel ->
            channel.bindings.prefix?.forEach {
                if (e.message.startsWith(it, ignoreCase = true)) {
                    if (channel.settings.isPrivate) e.isCancelled = true
                    channel.execute(player, e.message.substring(it.length), cancelEvent)
                    return
                }
            }
        }
        session.getChannel()?.let {
            if (it.settings.isPrivate) e.isCancelled = true
            it.execute(player, e.message, cancelEvent)
        }
    }

}