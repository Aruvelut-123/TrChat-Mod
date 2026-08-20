package me.arasple.mc.trchat.module.internal.listener

import com.velocitypowered.api.event.connection.DisconnectEvent
import com.velocitypowered.api.event.connection.PluginMessageEvent
import com.velocitypowered.api.event.connection.PostLoginEvent
import com.velocitypowered.api.event.player.ServerPostConnectEvent
import com.velocitypowered.api.proxy.ServerConnection
import me.arasple.mc.trchat.api.impl.VelocityProxyManager
import me.arasple.mc.trchat.module.internal.TrChatVelocity.plugin
import me.arasple.mc.trchat.util.print
import me.arasple.mc.trchat.util.proxy.common.MessageReader
import me.arasple.mc.trchat.util.toUUID
import net.kyori.adventure.audience.Audience
import net.kyori.adventure.identity.Identity
import net.kyori.adventure.text.Component
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.platform.function.submit
import taboolib.common.platform.function.warning
import java.io.IOException

/**
 * ListenerVelocityTransfer
 * me.arasple.mc.trchat.util.proxy.velocity
 *
 * @author ItsFlicker
 * @since 2021/8/21 13:29
 */
@PlatformSide(Platform.VELOCITY)
object ListenerVelocityTransfer {

    @SubscribeEvent
    fun onTransfer(e: PluginMessageEvent) {
        if (e.identifier != VelocityProxyManager.incoming) return
        val source = e.source
        if (source !is ServerConnection) {
            warning("Received trchat:proxy message, but source is not ServerConnection")
            return
        }
        try {
            val message = MessageReader.read(e.data)
            if (message.isCompleted) {
                val data = message.build()
                execute(data, source)
            }
        } catch (ex: IOException) {
            ex.print("Error occurred while reading plugin message.")
        }
    }

    @SubscribeEvent
    fun onProxyJoin(e: PostLoginEvent) {
        updateAllNamesLater()
    }

    @SubscribeEvent
    fun onProxyQuit(e: DisconnectEvent) {
        updateAllNamesLater()
    }

    @SubscribeEvent
    fun onProxySwitch(e: ServerPostConnectEvent) {
        updateAllNamesLater()
    }

    private fun updateAllNamesLater() {
        submit(delay = 30) {
            VelocityProxyManager.updateAllNames()
        }
    }

    private fun execute(data: Array<String>, connection: ServerConnection) {
        when (data[0]) {
            "ForwardMessage" -> {
                VelocityProxyManager.sendMessageToAll(*data)
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
                val message = kotlin.runCatching { GsonComponentSerializer.gson().deserialize(raw) }
                    .getOrElse { LegacyComponentSerializer.legacySection().deserialize(fallback) }
                plugin.server.consoleCommandSource.sendMessage(message)

                if (doubleTransfer) {
                    VelocityProxyManager.sendMessageToAll(*data) {
                        ports == null || it.serverInfo.address.port in ports
                    }
                } else {
                    plugin.server.allServers.forEach { server ->
                        if (ports == null || server.serverInfo.address.port in ports) {
                            val receivers = server.playersConnected.filter { perm == "" || it.hasPermission(perm) }
                            receivers.forEach {
                                it.sendIdentifiedMessage(Identity.identity(uuid.toUUID()), message)
                            }
                            if (mentioned.isNotEmpty() && senderName.isNotEmpty()) {
                                receivers.filter { it.username in mentioned }.forEach {
                                    VelocityProxyManager.sendMessage(server, "SendLang", it.username, "Function-Mention-Notify", senderName)
                                }
                            }
                        }
                    }
                }
            }
            "UpdateNames" -> {
                val port = data[1].toIntOrNull() ?: connection.serverInfo.address.port
                val names = data[2].split(",")
                val displayNames = data[3].split(",")
                val uuids = data[4].split(",")
                val updated = names.mapIndexed { index, name ->
                    Triple(name, displayNames[index], uuids[index])
                }
                if (VelocityProxyManager.updateNames(port, updated)) {
                    VelocityProxyManager.updateAllNames()
                }
            }
        }
    }

    private val sendMessageWithIdentity by lazy {
        runCatching { Audience::class.java.getMethod("sendMessage", Identity::class.java, Component::class.java) }.getOrNull()
    }

    @Suppress("Deprecation")
    private fun Audience.sendIdentifiedMessage(identity: Identity, message: Component) {
        if (sendMessageWithIdentity != null) {
            sendMessage(identity, message)
        } else {
            sendMessage(message)
        }
    }

}
