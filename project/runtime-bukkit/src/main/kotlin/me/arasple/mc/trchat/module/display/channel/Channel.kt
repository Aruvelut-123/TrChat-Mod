package me.arasple.mc.trchat.module.display.channel

import me.arasple.mc.trchat.TrChat
import me.arasple.mc.trchat.api.event.TrChatEvent
import me.arasple.mc.trchat.api.event.TrChatSendEvent
import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.conf.file.Settings
import me.arasple.mc.trchat.module.display.channel.obj.*
import me.arasple.mc.trchat.module.display.format.Format
import me.arasple.mc.trchat.module.display.format.MsgComponent
import me.arasple.mc.trchat.module.display.function.Function
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.module.internal.data.ChatLogs
import me.arasple.mc.trchat.module.internal.service.Metrics
import me.arasple.mc.trchat.util.*
import org.bukkit.command.CommandSender
import org.bukkit.entity.Player
import taboolib.common.platform.command.PermissionDefault
import taboolib.common.platform.command.command
import taboolib.common.platform.function.adaptPlayer
import taboolib.common.platform.function.console
import taboolib.common.platform.function.getProxyPlayer
import taboolib.common.util.Strings
import taboolib.common.util.subList
import taboolib.module.chat.ComponentText
import taboolib.module.chat.Components
import taboolib.module.lang.sendLang
import taboolib.platform.util.onlinePlayers
import taboolib.platform.util.sendLang

/**
 * @author ItsFlicker
 * @since 2021/12/11 22:27
 */
open class Channel(
    val id: String,
    val settings: ChannelSettings,
    val bindings: ChannelBindings,
    val events: ChannelEvents,
    val formats: List<Format>,
    val consoleFormat: List<Format>
) {

    var isUnregistered = false

    val listeners: MutableSet<String> = mutableSetOf()

    open fun init() {
        registerCommand()
        onlinePlayers.filter { it.session.channel == id }.forEach {
            join(it, this, hint = false)
        }
        if (settings.alwaysListen) {
            onlinePlayers.forEach {
                if (canListen(it)) {
                    listeners.add(it.name)
                }
            }
        }
    }

    open fun registerCommand() {
        if (bindings.command.isNullOrEmpty() || TrChatBukkit.isActivated) return
        command(
            name = bindings.command[0],
            aliases = subList(bindings.command, 1),
            description = "TrChat channel $id",
            permission = "trchat.command.channel.${id.lowercase()}",
            permissionDefault = PermissionDefault.TRUE
        ) {
            execute<Player> { sender, _, _ ->
                if (sender.session.channel == id) {
                    quit(sender, setDefault = true)
                } else {
                    join(sender, id)
                }
            }
            dynamic("message", optional = true) {
                execute<CommandSender> { sender, _, argument ->
                    val channel = channels[id] ?: return@execute
                    if (sender is Player) {
                        channel.execute(sender, argument)
                    } else {
                        channel.execute(sender, argument)
                    }
                }
            }
            incorrectSender { sender, _ ->
                sender.sendLang("Command-Not-Player")
            }
        }
    }

    open fun canListen(player: Player): Boolean {
        return player.passPermission(settings.listenPermission)
    }

    open fun canSpeak(player: Player): Boolean {
        return if (settings.speakCondition.isEmpty()) {
            player.passPermission(settings.joinPermission)
        } else {
            settings.speakCondition.pass(player)
        }
    }

    open fun execute(sender: CommandSender, message: String): ChannelExecuteResult {
        Function.clearMentioned()
        if (sender is Player) {
            return execute(sender, message)
        }
        val component = Components.empty()
        consoleFormat.firstOrNull()?.let { format ->
            format.prefix.forEach { prefix ->
                component.append(prefix.value[0].content.toTextComponent(sender)) }
            component.append((format.msg[0].content as MsgComponent).createComponent(sender, message, settings.disabledFunctions))
            format.suffix.forEach { suffix ->
                component.append(suffix.value[0].content.toTextComponent(sender)) }
        } ?: return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.NO_FORMAT)

        if (settings.proxy && BukkitProxyManager.processor != null) {
            BukkitProxyManager.sendBroadcastRaw(
                onlinePlayers.firstOrNull(),
                nilUUID,
                component,
                settings.listenPermission,
                settings.doubleTransfer,
                settings.ports
            )
        } else {
            listeners.forEach { getProxyPlayer(it)?.sendComponent(null, component) }
            sender.sendComponent(null, component)
        }
        return ChannelExecuteResult.success(component)
    }

    open fun execute(player: Player, message: String, toConsole: Boolean = true): ChannelExecuteResult {
        return execute(player, Components.text(message), toConsole)
    }

    open fun execute(player: Player, message: ComponentText, toConsole: Boolean = true): ChannelExecuteResult {
        Function.clearMentioned()
        var plain = message.toPlainText()
        if (!checkLimits(player, plain)) {
            return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.LIMITED)
        }
        val session = player.session
        session.lastChannel = this
        session.lastPublicMessage = plain
        val event = TrChatEvent(this, session, message)
        if (!event.call()) {
            return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.EVENT)
        }
        val msg = events.process(player, event.component)
            ?: return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.EVENT)
        plain = msg.toPlainText()
        ChatLogs.logNormal(player.name, plain)
        Metrics.increase(0)

        var component = Components.empty()
        var mentioned = emptySet<String>()
        formats.firstOrNull { it.condition.pass(player) }?.let { format ->
            format.prefix
                .mapNotNull { prefix -> prefix.value.firstOrNull { it.condition.pass(player) }?.content?.toTextComponent(player) }
                .forEach { prefix -> component.append(prefix) }
            format.msg.firstOrNull { it.condition.pass(player) }
                ?.let { component.append((it.content as MsgComponent).createComponent(player, msg, settings.disabledFunctions)) }
                ?: return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.NO_FORMAT)
            mentioned = Function.takeMentioned()
            format.suffix
                .mapNotNull { suffix -> suffix.value.firstOrNull { it.condition.pass(player) }?.content?.toTextComponent(player) }
                .forEach { suffix -> component.append(suffix) }
        } ?: return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.NO_FORMAT)
        if (session.cancelChat) {
            session.cancelChat = false
            return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.EVENT)
        }
        val sendEvent = TrChatSendEvent(this, session, component)
        if (!sendEvent.call()) {
            return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.EVENT)
        }
        component = sendEvent.component

        if (player.data.isShadowMuted) {
            if (events.send(player, player.name, plain)) {
                player.sendComponent(player, component)
                if (player.name in mentioned) {
                    BukkitProxyManager.sendProxyLang(player, player.name, "Function-Mention-Notify", player.name)
                }
            }
            if (toConsole) {
                console().sendComponent(player, component)
            }
            return ChannelExecuteResult(failedReason = ChannelExecuteResult.FailReason.LIMITED)
        }

        // Proxy
        if (settings.proxy) {
            if (BukkitProxyManager.processor != null || settings.forceProxy) {
                BukkitProxyManager.sendBroadcastRaw(
                    player,
                    player.uniqueId,
                    component,
                    settings.listenPermission,
                    settings.doubleTransfer,
                    settings.ports,
                    senderName = player.name,
                    mentioned = mentioned.joinToString(",")
                )
                return ChannelExecuteResult.success(component)
            }
        }
        // Local
        when (settings.range.type) {
            ChannelRange.Type.ALL -> {
                val receivers = listeners.filter { events.send(player, it, plain) }
                receivers.forEach {
                    getProxyPlayer(it)?.sendComponent(player, component)
                }
                notifyMentioned(player, receivers, mentioned)
            }
            ChannelRange.Type.SINGLE_WORLD -> {
                val receivers = onlinePlayers.filter { it.name in listeners
                        && it.world == player.world
                        && events.send(player, it.name, plain) }
                receivers.forEach {
                    it.sendComponent(player, component)
                }
                notifyMentioned(player, receivers.map { it.name }, mentioned)
            }
            ChannelRange.Type.DISTANCE -> {
                val receivers = onlinePlayers.filter { it.name in listeners
                        && it.world == player.world
                        && it.location.distance(player.location) <= settings.range.distance
                        && events.send(player, it.name, plain) }
                receivers.forEach {
                    it.sendComponent(player, component)
                }
                notifyMentioned(player, receivers.map { it.name }, mentioned)
            }
            ChannelRange.Type.SELF -> {
                if (events.send(player, player.name, plain)) {
                    player.sendComponent(player, component)
                    notifyMentioned(player, listOf(player.name), mentioned)
                }
            }
        }
        if (toConsole) {
            console().sendComponent(player, component)
        }
        return ChannelExecuteResult.success(component)
    }

    /** 只有真正收到（能看到）该消息且被 @ 的玩家才会收到提示 */
    protected fun notifyMentioned(sender: Player, receivers: Collection<String>, mentioned: Set<String>) {
        if (mentioned.isEmpty()) return
        receivers.filter { it in mentioned }.forEach {
            BukkitProxyManager.sendProxyLang(sender, it, "Function-Mention-Notify", sender.name)
        }
    }

    open fun checkLimits(player: Player, message: String): Boolean {
        if (player.hasPermission("trchat.bypass.*")) {
            return true
        }
        if (!player.checkMute()) {
            return false
        }
        if (!canSpeak(player)) {
            player.sendLang("Channel-No-Speak-Permission")
            return false
        }
        if (settings.filterBeforeSending && TrChat.api().getFilterManager().filter(message, adaptPlayer(player)).sensitiveWords > 0) {
            player.sendLang("Channel-Bad-Language")
            return false
        }
        if (!player.hasPermission("trchat.bypass.chatlength")) {
            if (message.length > Settings.chatLengthLimit) {
                player.sendLang("General-Too-Long", message.length, Settings.chatLengthLimit)
                return false
            }
        }
        if (!player.hasPermission("trchat.bypass.repeat")) {
            val lastMessage = player.session.lastPublicMessage
            if (Settings.chatSimilarity > 0 && Settings.chatSimilarity <= 1 && Strings.similarDegree(lastMessage, message) >= Settings.chatSimilarity) {
                player.sendLang("General-Too-Similar")
                return false
            }
        }
        if (!player.hasPermission("trchat.bypass.chatcd")) {
            val chatCooldown = player.getCooldownLeft(CooldownType.CHAT)
            if (chatCooldown > 0) {
                player.sendLang("Cooldowns-Chat", chatCooldown / 1000)
                return false
            }
        }
        if (Function.functions.any { !it.checkCooldown(player, message) }) {
            return false
        }
        player.updateCooldown(CooldownType.CHAT, Settings.chatCooldown.get())
        return true
    }

    open fun unregister() {
        isUnregistered = true
        listeners.clear()
    }

    companion object {

        val channels = mutableMapOf<String, Channel>()

        fun join(player: Player, channel: String, hint: Boolean = true): Boolean {
            val id = channels.keys.firstOrNull { channel.equals(it, ignoreCase = true) } ?: return false
            return join(player, channels[id]!!, hint)
        }

        fun join(player: Player, channel: Channel, hint: Boolean = true): Boolean {
            if (!player.passPermission(channel.settings.joinPermission)) {
                if (hint) {
                    player.sendLang("General-No-Permission")
                }
                return false
            }
            quit(player, hint = false)
            player.session.setChannel(channel)
            channel.listeners.add(player.name)
            channel.events.join(player)

            if (hint) {
                player.sendLang("Channel-Join", channel.id)
            }
            return true
        }

        fun quit(player: Player, setDefault: Boolean = false, hint: Boolean = true) {
            player.session.getChannel()?.let {
                if (!it.settings.alwaysListen) {
                    it.listeners -= player.name
                }
                it.events.quit(player)
                if (hint) {
                    player.sendLang("Channel-Quit", it.id)
                }
            }
            if (!setDefault || !join(player, Settings.defaultChannel)) {
                player.session.setChannel(null)
            }
        }
    }
}