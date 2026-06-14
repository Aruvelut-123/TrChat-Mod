package me.arasple.mc.trchat.api.impl

import com.google.common.util.concurrent.ThreadFactoryBuilder
import com.velocitypowered.api.proxy.ServerConnection
import com.velocitypowered.api.proxy.messages.ChannelMessageSink
import com.velocitypowered.api.proxy.messages.MinecraftChannelIdentifier
import com.velocitypowered.api.proxy.server.RegisteredServer
import me.arasple.mc.trchat.api.ProxyMessageManager
import me.arasple.mc.trchat.module.internal.TrChatVelocity.plugin
import me.arasple.mc.trchat.util.print
import me.arasple.mc.trchat.util.proxy.buildMessage
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformFactory
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.Schedule
import taboolib.common.platform.function.warning
import taboolib.common.util.unsafeLazy
import java.io.IOException
import java.util.concurrent.*

/**
 * @author ItsFlicker
 * @since 2022/6/18 19:21
 */
@PlatformSide(Platform.VELOCITY)
object VelocityProxyManager : ProxyMessageManager {

    val incoming: MinecraftChannelIdentifier
    val outgoing: MinecraftChannelIdentifier

    init {
        PlatformFactory.registerAPI<ProxyMessageManager>(this)
        incoming = MinecraftChannelIdentifier.from("trchat:proxy")
        outgoing = MinecraftChannelIdentifier.from("trchat:server")
    }

    override val executor: ExecutorService by unsafeLazy {
        val factory = ThreadFactoryBuilder().setNameFormat("TrChat PluginMessage Processing Thread #%d").build()
        Executors.newFixedThreadPool(8, factory)
    }

    override val allNames = ConcurrentHashMap<Int, List<Triple<String, String, String>>>()

    override fun sendMessage(recipient: Any, vararg args: String): Future<*> {
        if (recipient !is ChannelMessageSink) {
            return CompletableFuture.completedFuture(false)
        }
        return executor.submit {
            try {
                for (bytes in buildMessage(*args)) {
                    if (!recipient.sendPluginMessage(outgoing, bytes)) {
                        warning("Failed to send proxy trchat message!")
                        warning(args)
                        val info = when (recipient) {
                            is RegisteredServer -> recipient.serverInfo
                            is ServerConnection -> recipient.serverInfo
                            else -> continue
                        }
                        warning(info)
                    }
                }
            } catch (e: IOException) {
                e.print("Failed to send proxy trchat message!")
            }
        }
    }

    fun sendMessageToAll(vararg args: String, predicate: (RegisteredServer) -> Boolean = { true }): Future<*> {
        val recipients = plugin.server.allServers.filter { v -> v.playersConnected.isNotEmpty() && predicate(v) }
        return executor.submit {
            try {
                for (bytes in buildMessage(*args)) {
                    recipients.forEach { v ->
                        if (!v.sendPluginMessage(outgoing, bytes)) {
                            warning("Failed to send proxy trchat message!")
                            warning(args)
                            warning(v.serverInfo)
                        }
                    }
                }
            } catch (e: IOException) {
                e.print("Failed to send proxy trchat message!")
            }
        }
    }

    @Schedule(period = 1200L)
    @Synchronized
    override fun updateAllNames() {
        refreshNamesFromProxy()
        val flat = allNames.values.toList().flatten()
        sendMessageToAll(
            "UpdateAllNames",
            flat.joinToString(",") { it.first },
            flat.joinToString(",") { it.second },
            flat.joinToString(",") { it.third },
        )
    }

    @Synchronized
    fun updateNames(port: Int, names: List<Triple<String, String, String>>): Boolean {
        if (allNames[port]?.toSet() == names.toSet()) {
            return false
        }
        allNames[port] = names
        return true
    }

    private fun refreshNamesFromProxy() {
        val displayNames = allNames.values.toList().flatten().associate { it.third to it.second }
        allNames.clear()
        plugin.server.allPlayers
            .mapNotNull { player ->
                val info = player.currentServer.map { it.serverInfo }.orElse(null) ?: return@mapNotNull null
                info.address.port to Triple(player.username, displayNames[player.uniqueId.toString()] ?: "#", player.uniqueId.toString())
            }
            .groupBy({ it.first }, { it.second })
            .forEach { (port, names) -> allNames[port] = names }
    }

}
