package me.arasple.mc.trchat.module.internal.listener

import me.arasple.mc.trchat.api.impl.BungeeComponentManager
import me.arasple.mc.trchat.api.impl.BungeeProxyManager
import me.arasple.mc.trchat.module.internal.TrChatBungee
import me.arasple.mc.trchat.util.print
import me.arasple.mc.trchat.util.proxy.common.MessageReader
import net.md_5.bungee.api.ProxyServer
import net.md_5.bungee.api.connection.Connection
import net.md_5.bungee.api.event.PlayerDisconnectEvent
import net.md_5.bungee.api.event.PluginMessageEvent
import net.md_5.bungee.api.event.PostLoginEvent
import net.md_5.bungee.api.event.ServerSwitchEvent
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.platform.function.console
import taboolib.common.platform.function.server
import taboolib.common.platform.function.submit
import taboolib.module.chat.Components
import java.io.IOException

/**
 * ListenerBungeeTransfer
 * me.arasple.mc.trchat.util.proxy.bungee
 *
 * @author ItsFlicker
 * @since 2021/8/9 15:01
 */
@PlatformSide(Platform.BUNGEE)
object ListenerBungeeTransfer {

    @SubscribeEvent(level = 0)
    fun onTransfer(e: PluginMessageEvent) {
        if (e.isCancelled) {
            return
        }
        if (e.tag == TrChatBungee.TRCHAT_CHANNEL) {
            try {
                val message = MessageReader.read(e.data)
                if (message.isCompleted) {
                    val data = message.build()
                    execute(data, e.sender)
                }
            } catch (ex: IOException) {
                ex.print("Error occurred while reading plugin message.")
            }
        }
    }

    @SubscribeEvent
    fun onProxyJoin(e: PostLoginEvent) {
        updateAllNamesLater()
    }

    @SubscribeEvent
    fun onProxyQuit(e: PlayerDisconnectEvent) {
        updateAllNamesLater()
    }

    @SubscribeEvent
    fun onProxySwitch(e: ServerSwitchEvent) {
        updateAllNamesLater()
    }

    private fun updateAllNamesLater() {
        submit(delay = 30) {
            BungeeProxyManager.updateAllNames()
        }
    }

    @Suppress("Deprecation")
    private fun execute(data: Array<String>, connection: Connection) {
        when (data[0]) {
            "ForwardMessage" -> {
                BungeeProxyManager.sendMessageToAll(*data)
            }
            "BroadcastRaw" -> {
                val uuid = data[1]
                val raw = data[2]
                val perm = data[3]
                val doubleTransfer = data[4].toBoolean()
                val ports = data[5].takeIf { it != "" }?.split(";")?.map { it.toInt() }
                val fallback = data.getOrElse(6) { "" }
                val senderName = data.getOrElse(7) { "" }
                val mentioned = data.getOrElse(8) { "" }.takeIf { it.isNotEmpty() }?.split(",")?.toSet() ?: emptySet()
                val message = kotlin.runCatching { Components.parseRaw(raw) }.getOrElse { Components.text(fallback) }
                message.sendTo(console())

                if (doubleTransfer) {
                    BungeeProxyManager.sendMessageToAll(*data) {
                        ports == null || it.address.port in ports
                    }
                } else {
                    server<ProxyServer>().servers.forEach { (_, v) ->
                        if (ports == null || v.address.port in ports) {
                            val receivers = v.players.filter { perm == "" || it.hasPermission(perm) }
                            receivers.forEach {
                                BungeeComponentManager.sendComponent(it, message, uuid)
                            }
                            if (mentioned.isNotEmpty() && senderName.isNotEmpty()) {
                                receivers.filter { it.name in mentioned }.forEach {
                                    BungeeProxyManager.sendMessage(v, "SendLang", it.name, "Function-Mention-Notify", senderName)
                                }
                            }
                        }
                    }
                }
            }
            "UpdateNames" -> {
                val port = data[1].toIntOrNull() ?: connection.address.port
                val names = data[2].split(",")
                val displayNames = data[3].split(",")
                val uuids = data[4].split(",")
                val updated = names.mapIndexed { index, name ->
                    Triple(name, displayNames[index], uuids[index])
                }
                if (BungeeProxyManager.updateNames(port, updated)) {
                    BungeeProxyManager.updateAllNames()
                }
            }
        }
    }
}
