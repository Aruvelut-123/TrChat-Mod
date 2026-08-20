package me.arasple.mc.trchat.module.internal.proxy

import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.display.channel.PrivateChannel
import me.arasple.mc.trchat.module.display.function.standard.EnderChestShow
import me.arasple.mc.trchat.module.display.function.standard.InventoryShow
import me.arasple.mc.trchat.module.display.function.standard.ItemShow
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.module.internal.command.main.CommandReply
import me.arasple.mc.trchat.module.internal.proxy.redis.RedisManager
import me.arasple.mc.trchat.module.internal.proxy.redis.TrRedisMessage
import me.arasple.mc.trchat.util.print
import me.arasple.mc.trchat.util.proxy.buildMessage
import me.arasple.mc.trchat.util.proxy.common.MessageReader
import me.arasple.mc.trchat.util.sendComponent
import me.arasple.mc.trchat.util.toUUID
import org.bukkit.Bukkit
import org.bukkit.entity.Player
import org.bukkit.inventory.Inventory
import org.bukkit.plugin.messaging.PluginMessageListener
import org.bukkit.plugin.messaging.PluginMessageRecipient
import taboolib.common.platform.function.console
import taboolib.common.platform.function.getProxyPlayer
import taboolib.common.util.subList
import taboolib.common5.util.decodeBase64
import taboolib.module.chat.Components
import taboolib.module.lang.asLangText
import taboolib.module.lang.sendLang
import taboolib.module.ui.MenuHolder
import taboolib.module.ui.type.impl.ChestImpl
import taboolib.platform.util.bukkitPlugin
import taboolib.platform.util.deserializeToInventory
import taboolib.platform.util.onlinePlayers
import java.io.IOException
import java.util.*
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Future

/**
 * @author ItsFlicker
 * @since 2022/6/18 16:17
 */
sealed interface BukkitProxyProcessor : PluginMessageListener {

    operator fun invoke(): BukkitProxyProcessor = this

    override fun onPluginMessageReceived(channel: String, player: Player, message: ByteArray) = Unit

    fun close() {
        Bukkit.getMessenger().unregisterIncomingPluginChannel(bukkitPlugin)
    }

    fun sendMessage(
        recipient: PluginMessageRecipient,
        executor: ExecutorService,
        data: Array<String>
    ): Future<*>

    fun execute(data: Array<String>) {
        when (data[0]) {
            "ForwardMessage" -> {
                execute(data.drop(1).toTypedArray())
            }
            "SendLang" -> {
                val to = data[1]
                val node = data[2]
                val args = subList(data.toList(), 3).toTypedArray()

                try {
                    getProxyPlayer(to)?.sendLang(node, *args)
                } catch (_: IllegalStateException) {
                }
            }
            "SendPrivateRaw" -> {
                val to = data[1]
                val from = data[2]
                val raw = data[3]
                val fallback = data.getOrElse(4) { "" }
                val message = kotlin.runCatching { Components.parseRaw(raw) }.getOrElse { Components.text(fallback) }

                if (from.isNotEmpty()) {
                    CommandReply.lastMessageFrom[to] = from
                }
                getProxyPlayer(to)?.sendComponent(null, message)

                val rawMsgComponent = data.getOrNull(5)?.takeIf { it.isNotEmpty() } ?: return
                val msgComponent = kotlin.runCatching { Components.parseRaw(rawMsgComponent) }.getOrNull() ?: return
                PrivateChannel.sendSpy(from, to, msgComponent)
            }
            "BroadcastRaw" -> {
                val uuid = data[1].toUUID()
                val raw = data[2]
                val perm = data[3]
                val ports = data[5].takeIf { it != "" }?.split(";")?.map { it.toInt() }
                val fallback = data.getOrElse(6) { "" }
                val senderName = data.getOrElse(7) { "" }
                val mentioned = data.getOrElse(8) { "" }.takeIf { it.isNotEmpty() }?.split(",")?.toSet() ?: emptySet()
                val message = kotlin.runCatching { Components.parseRaw(raw) }.getOrElse { Components.text(fallback) }

                if (ports == null || BukkitProxyManager.port in ports) {
                    val receivers = onlinePlayers.filter { perm == "" || it.hasPermission(perm) }
                    receivers.forEach { it.sendComponent(uuid, message) }
                    if (mentioned.isNotEmpty() && senderName.isNotEmpty()) {
                        receivers.filter { it.name in mentioned }.forEach {
                            getProxyPlayer(it.name)?.sendLang("Function-Mention-Notify", senderName)
                        }
                    }
                    if (this is RedisSide) {
                        console().sendComponent(uuid, message)
                    }
                }
            }
            "UpdateAllNames" -> {
                val names = data[1].takeIf { it != "" }?.split(",") ?: return
                val displayNames = data[2].split(",")
                val uuids = data[3].split(",")
                BukkitProxyManager.allPlayerNames = names.mapIndexed { index, name ->
                    Triple(name, displayNames[index].takeIf { it != "#" }, uuids[index].toUUID())
                }
            }
            "GlobalMute" -> {
                when (data[1]) {
                    "on" -> TrChatBukkit.isGlobalMuting = true
                    "off" -> TrChatBukkit.isGlobalMuting = false
                }
            }
            "ItemShow" -> {
//                if (data[1] > MinecraftVersion.minecraftVersion) return
                val name = data[2]
                val sha1 = data[3]
                if (ItemShow.cacheInventory.getIfPresent(sha1) == null) {
                    val inventory = try {
                        data[4].decodeBase64().deserializeToInventory(
                            createNoClickChest(3, console().asLangText("Function-Item-Show-Title", name))
                        )
                    } catch (_: Throwable) {
                        return
                    }
                    ItemShow.cacheInventory.put(sha1, inventory)
                }
            }
            "InventoryShow" -> {
//                if (data[1] > MinecraftVersion.minecraftVersion) return
                val name = data[2]
                val sha1 = data[3]
                if (InventoryShow.cache.getIfPresent(sha1) == null) {
                    val inventory = try {
                        data[4].decodeBase64().deserializeToInventory(
                            createNoClickChest(6, console().asLangText("Function-Inventory-Show-Title", name))
                        )
                    } catch (_: Throwable) {
                        return
                    }
                    InventoryShow.cache.put(sha1, inventory)
                }
            }
            "EnderChestShow" -> {
//                if (data[1] > MinecraftVersion.minecraftVersion) return
                val name = data[2]
                val sha1 = data[3]
                if (EnderChestShow.cache.getIfPresent(sha1) == null) {
                    val inventory = try {
                        data[4].decodeBase64().deserializeToInventory(
                            createNoClickChest(3, console().asLangText("Function-EnderChest-Show-Title", name))
                        )
                    } catch (_: Throwable) {
                        return
                    }
                    EnderChestShow.cache.put(sha1, inventory)
                }
            }
        }
    }

    object BungeeSide : BukkitProxyProcessor {

        private const val TRCHAT_CHANNEL = "trchat:main"

        override operator fun invoke(): BukkitProxyProcessor {
            TRCHAT_CHANNEL.registerOutgoing()
            TRCHAT_CHANNEL.registerIncoming(this)
            return super.invoke()
        }

        override fun sendMessage(
            recipient: PluginMessageRecipient,
            executor: ExecutorService,
            data: Array<String>
        ): Future<*> {
            return executor.submit {
                try {
                    for (bytes in buildMessage(*data)) {
                        recipient.sendPluginMessage(bukkitPlugin, TRCHAT_CHANNEL, bytes)
                    }
                } catch (e: IOException) {
                    e.print("Failed to send proxy trchat message!")
                }
            }
        }

        override fun onPluginMessageReceived(channel: String, player: Player, message: ByteArray) {
            if (channel == TRCHAT_CHANNEL) {
                try {
                    val data = MessageReader.read(message)
                    if (data.isCompleted) {
                        execute(data.build())
                    }
                } catch (e: IOException) {
                    e.print("Failed to read proxy trchat message!")
                }
            }
        }
    }

    object VelocitySide : BukkitProxyProcessor {

        private const val TRCHAT_INCOMING = "trchat:server"
        private const val TRCHAT_OUTGOING = "trchat:proxy"

        override operator fun invoke(): BukkitProxyProcessor {
            TRCHAT_OUTGOING.registerOutgoing()
            TRCHAT_INCOMING.registerIncoming(this)
            return super.invoke()
        }

        override fun sendMessage(
            recipient: PluginMessageRecipient,
            executor: ExecutorService,
            data: Array<String>
        ): Future<*> {
            return executor.submit {
                try {
                    for (bytes in buildMessage(*data)) {
                        recipient.sendPluginMessage(bukkitPlugin, TRCHAT_OUTGOING, bytes)
                    }
                } catch (e: IOException) {
                    e.print("Failed to send proxy trchat message!")
                }
            }
        }

        override fun onPluginMessageReceived(channel: String, player: Player, message: ByteArray) {
            if (channel == TRCHAT_INCOMING) {
                try {
                    val data = MessageReader.read(message)
                    if (data.isCompleted) {
                        execute(data.build())
                    }
                } catch (e: IOException) {
                    e.print("Failed to read proxy trchat message!")
                }
            }
        }
    }

    object RedisSide : BukkitProxyProcessor {

        val allNames = ConcurrentHashMap<String, List<Triple<String, String?, UUID>>>()

        override fun execute(data: Array<String>) {
            when (data[0]) {
                "UpdateNames" -> {
                    val port = data[1]
                    val names = data[2].split(",")
                    val displayNames = data[3].split(",")
                    val uuids = data[4].split(",")
                    allNames[port] = names.mapIndexed { index, name ->
                        Triple(name, displayNames[index].takeIf { it != "#" }, uuids[index].toUUID())
                    }
                    BukkitProxyManager.allPlayerNames = allNames.values.flatten()
                }
                else -> super.execute(data)
            }
        }

        override fun sendMessage(
            recipient: PluginMessageRecipient,
            executor: ExecutorService,
            data: Array<String>
        ): Future<*> {
            return executor.submit {
                RedisManager.sendMessage(TrRedisMessage(data))
            }
        }
    }

    companion object {

        protected fun String.registerOutgoing() {
            if (!Bukkit.getMessenger().isOutgoingChannelRegistered(bukkitPlugin, this)) {
                Bukkit.getMessenger().registerOutgoingPluginChannel(bukkitPlugin, this)
            }
        }

        protected fun String.registerIncoming(listener: PluginMessageListener) {
            if (!Bukkit.getMessenger().isIncomingChannelRegistered(bukkitPlugin, this)) {
                Bukkit.getMessenger().registerIncomingPluginChannel(bukkitPlugin, this, listener)
            }
        }

        fun createNoClickChest(rows: Int, title: String): Inventory {
            return MenuHolder(object : ChestImpl(title) {
                init {
                    rows(rows)
                    onClick(lock = true)
                }
            }).inventory
        }

    }
}
