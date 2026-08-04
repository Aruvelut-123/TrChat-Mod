package me.arasple.mc.trchat.module.internal.command.main

import me.arasple.mc.trchat.module.conf.file.Settings
import me.arasple.mc.trchat.module.display.channel.Channel
import org.bukkit.Bukkit
import org.bukkit.command.CommandSender
import org.bukkit.entity.Player
import taboolib.common.LifeCycle
import taboolib.common.platform.Awake
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.command.command
import taboolib.common.platform.command.suggest
import taboolib.common.platform.command.suggestUncheck
import taboolib.expansion.createHelper
import taboolib.module.lang.sendLang
import taboolib.platform.util.onlinePlayers
import taboolib.platform.util.sendLang

/**
 * @author ItsFlicker
 * @since 2021/7/21 11:24
 */
@PlatformSide(Platform.BUKKIT)
object CommandChannel {

    @Awake(LifeCycle.ENABLE)
    fun register() {
        if (Settings.conf.getStringList("Options.Disabled-Commands").contains("channel")) return
        command("channel", listOf("chatchannel", "trchannel"), "TrChat Channel", permission = "trchat.command.channel") {
            literal("join") {
                dynamic("channel") {
                    suggest {
                        Channel.channels.keys.toList()
                    }
                    execute<Player> { sender, _, argument ->
                        Channel.join(sender, argument)
                    }
                    dynamic("player", optional = true) {
                        suggestUncheck {
                            onlinePlayers.map { it.name }
                        }
                        execute<CommandSender> { sender, ctx, _ ->
                            sender.changeChannel(ctx["channel"], "join", ctx["player"])
                        }
                    }
                }
            }
            literal("quit", "leave") {
                execute<Player> { sender, _, _ ->
                    Channel.quit(sender, true)
                }
                dynamic("player", optional = true) {
                    suggestUncheck {
                        onlinePlayers.map { it.name }
                    }
                    execute<CommandSender> { sender, ctx, _ ->
                        sender.changeChannel("", "quit", ctx["player"])
                    }
                }
            }
            execute<Player> { _, _, _ ->
                createHelper()
            }
            incorrectSender { sender, _ ->
                sender.sendLang("Command-Not-Player")
            }
            incorrectCommand { _, _, _, _ ->
                createHelper()
            }
        }
    }

    private fun CommandSender.changeChannel(channelId: String, action: String, target: String) {
        if (this is Player && !hasPermission("trchat.command.channel.other")) {
            return sendLang("General-No-Permission")
        }
        val player = Bukkit.getPlayerExact(target)
            ?: onlinePlayers.firstOrNull { it.name.equals(target, ignoreCase = true) }
            ?: return sendLang("Command-Player-Not-Exist")
        if (action == "join") {
            val id = Channel.channels.keys.firstOrNull { channelId.equals(it, ignoreCase = true) }
            if (id == null) {
                return sendLang("Channel-Not-Found", channelId)
            }
            if (Channel.join(player, Channel.channels[id]!!)) {
                sendLang("Channel-Join-Other", player.name, id)
            } else {
                sendLang("General-No-Permission")
            }
        } else {
            Channel.quit(player, setDefault = true)
            sendLang("Channel-Quit-Other", player.name)
        }
    }
}
