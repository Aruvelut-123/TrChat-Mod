package me.arasple.mc.trchat.api.impl

import com.google.common.util.concurrent.ThreadFactoryBuilder
import me.arasple.mc.trchat.api.ClientMessageManager
import me.arasple.mc.trchat.api.ProxyMode
import me.arasple.mc.trchat.module.conf.file.Settings
import me.arasple.mc.trchat.module.internal.hook.isVanished
import me.arasple.mc.trchat.module.internal.proxy.BukkitProxyProcessor
import me.arasple.mc.trchat.module.internal.proxy.redis.RedisManager
import me.arasple.mc.trchat.util.parseString
import net.md_5.bungee.api.ChatColor
import org.bukkit.Bukkit
import org.bukkit.plugin.messaging.PluginMessageRecipient
import org.spigotmc.SpigotConfig
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformFactory
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.function.console
import taboolib.common.platform.function.getProxyPlayer
import taboolib.common.platform.function.submitAsync
import taboolib.common.util.unsafeLazy
import taboolib.common5.cint
import taboolib.module.chat.ComponentText
import taboolib.module.lang.sendLang
import taboolib.platform.util.onlinePlayers
import java.io.FileInputStream
import java.util.*
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.Future

/**
 * @author ItsFlicker
 * @since 2022/6/18 15:43
 */
@Suppress("Deprecation")
@PlatformSide(Platform.BUKKIT)
object BukkitProxyManager : ClientMessageManager {

    override var port = 25565

    // 这里不需要用 Set, 后面会转换为 Map
    var allPlayerNames = listOf<Triple<String, String?, UUID>>()
        get() = when (mode) {
            ProxyMode.NONE -> {
                onlinePlayers.map { Triple(it.name, ChatColor.stripColor(it.displayName), it.uniqueId) }
            }
            else -> {
                field + onlinePlayers.map { Triple(it.name, ChatColor.stripColor(it.displayName), it.uniqueId) }
            }
        }

    init {
        PlatformFactory.registerAPI<ClientMessageManager>(this)
        FileInputStream("server.properties").use {
            val props = Properties()
            props.load(it)
            port = props.getProperty("server-port")?.cint ?: 25565
        }
    }

    override val executor: ExecutorService by unsafeLazy {
        val factory = ThreadFactoryBuilder().setNameFormat("TrChat PluginMessage Processing Thread #%d").build()
        Executors.newFixedThreadPool(8, factory)
    }

    override val mode: ProxyMode by unsafeLazy {
        val force = Settings.conf.getString("Options.Proxy")?.uppercase() ?: "AUTO"
        if (force != "AUTO") {
            try {
                return@unsafeLazy ProxyMode.valueOf(force)
            } catch (_: IllegalArgumentException) {
            }
        }
        if (RedisManager.enabled) {
            console().sendLang("Plugin-Proxy-Supported", "Redis")
            ProxyMode.REDIS
        } else if (SpigotConfig.bungee) {
            console().sendLang("Plugin-Proxy-Supported", "Bungee")
            ProxyMode.BUNGEE
        } else if (kotlin.runCatching {
                Bukkit.spigot().paperConfig.getBoolean("proxies.velocity.enabled", false) ||
                        Bukkit.spigot().paperConfig.getBoolean("settings.velocity-support.enabled", false)
            }.getOrDefault(false)) {
            console().sendLang("Plugin-Proxy-Supported", "Velocity")
            ProxyMode.VELOCITY
        } else {
            console().sendLang("Plugin-Proxy-None")
            ProxyMode.NONE
        }
    }

    val processor by unsafeLazy {
        executor
        when (mode) {
            ProxyMode.BUNGEE -> {
                BukkitProxyProcessor.BungeeSide()
            }
            ProxyMode.VELOCITY -> {
                BukkitProxyProcessor.VelocitySide()
            }
            ProxyMode.REDIS -> {
                RedisManager()
                submitAsync(period = 200L) {
                    updateNames()
                }
                BukkitProxyProcessor.RedisSide()
            }
            else -> null
        }
    }

    override fun close() {
        processor?.close()
        executor.shutdownNow()
    }

    override fun getPlayerNames(includeVanish: Boolean): Map<String, String?> {
        return if (includeVanish) {
            allPlayerNames.associate { it.first to it.second }
        } else {
            allPlayerNames.filterNot { it.third.isVanished() }.associate { it.first to it.second }
        }
    }

    fun getPlayerNamesMerged(includeVanish: Boolean = false): Set<String> {
        return getPlayerNames(includeVanish).let { it.keys + it.values.filterNotNull() }
    }

    override fun getExactName(name: String): String? {
        var player = Bukkit.getPlayerExact(name)
        if (player == null) {
            player = Bukkit.getOnlinePlayers().firstOrNull { ChatColor.stripColor(it.displayName) == name }
        }
        return if (player != null && player.isOnline) {
            player.name
        } else {
            getPlayerNames().entries.firstOrNull {
                it.key.equals(name, ignoreCase = true) || it.value?.equals(name, ignoreCase = true) == true
            }?.key
        }
    }

    override fun isPlayerOnline(name: String): Boolean {
        return getExactName(name) != null
    }

    override fun sendMessage(recipient: Any?, data: Array<String>): Future<*> {
        if (processor == null || recipient !is PluginMessageRecipient) return CompletableFuture.completedFuture(false)
        return processor!!.sendMessage(recipient, executor, data)
    }



    fun sendProxyLang(recipient: Any?, target: String, node: String, vararg args: String) {
        if (processor == null || Bukkit.getPlayerExact(target) != null) {
            getProxyPlayer(target)?.sendLang(node, *args)
        } else {
            sendMessage(recipient, arrayOf("ForwardMessage", "SendLang", target, node, *args))
        }
    }

    fun sendBroadcastRaw(
        recipient: Any?,
        uuid: UUID,
        component: ComponentText,
        listenPerm: String = "",
        doubleTransfer: Boolean = true,
        ports: List<Int> = emptyList(),
        fallback: String = component.toLegacyText(),
        senderName: String = "",
        mentioned: String = ""
    ) {
        sendMessage(recipient, arrayOf(
            "BroadcastRaw",
            uuid.parseString(),
            component.toRawMessage(),
            listenPerm,
            doubleTransfer.toString(),
            ports.joinToString(";"),
            fallback,
            senderName,
            mentioned)
        )
    }

    fun sendPrivateRaw(recipient: Any?, to: String, from: String, component: ComponentText, msgComponent: ComponentText? = null, fallback: String = component.toLegacyText()) {
        sendMessage(recipient, arrayOf("ForwardMessage", "SendPrivateRaw", to, from, component.toRawMessage(), fallback, msgComponent?.toRawMessage() ?: ""))
    }

    fun updateNames() {
        sendMessage(onlinePlayers.lastOrNull(), arrayOf(
            "UpdateNames",
            port.toString(),
            onlinePlayers.joinToString(",") { it.name },
            onlinePlayers.joinToString(",") { ChatColor.stripColor(it.displayName)?.ifEmpty { "#" } ?: "#" },
            onlinePlayers.joinToString(",") { it.uniqueId.parseString() }
        ))
    }

}