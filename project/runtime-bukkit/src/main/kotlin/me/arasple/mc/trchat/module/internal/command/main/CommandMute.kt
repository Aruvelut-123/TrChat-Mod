package me.arasple.mc.trchat.module.internal.command.main

import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.conf.file.Settings
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.module.internal.data.PlayerData
import me.arasple.mc.trchat.util.data
import org.bukkit.Bukkit
import org.bukkit.OfflinePlayer
import org.bukkit.command.CommandSender
import taboolib.common.LifeCycle
import taboolib.common.platform.Awake
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.command.command
import taboolib.common.platform.command.suggestUncheck
import taboolib.common5.util.parseMillis
import taboolib.expansion.createHelper
import taboolib.platform.util.sendLang
import java.text.SimpleDateFormat

/**
 * CommandPrivateMessage
 * me.arasple.mc.trchat.module.internal.command
 *
 * @author ItsFlicker
 * @since 2021/7/21 10:40
 */
@PlatformSide(Platform.BUKKIT)
object CommandMute {

    val muteDateFormat = SimpleDateFormat()

    @Awake(LifeCycle.ENABLE)
    fun mute() {
        if (Settings.conf.getStringList("Options.Disabled-Commands").contains("mute")) return
        command("mute", listOf("trmute"), description = "Mute a player", permission = "trchat.command.mute") {
            dynamic("player") {
                suggestUncheck {
                    BukkitProxyManager.getPlayerNames(true).keys.toList()
                }
                execute<CommandSender> { sender, ctx, _ ->
                    mute(sender, ctx["player"], "999d", "null")
                }
                dynamic("time") {
                    suggestUncheck {
                        listOf("1d", "3h", "15m", "30s")
                    }
                    execute<CommandSender> { sender, ctx, _ ->
                        mute(sender, ctx["player"], ctx["time"], "null")
                    }
                    dynamic("reason") {
                        execute<CommandSender> { sender, ctx, _ ->
                            mute(sender, ctx["player"], ctx["time"], ctx["reason"])
                        }
                    }
                }
            }
            incorrectCommand { _, _, _, _ ->
                createHelper()
            }
        }
        command("shadowmute", listOf("muteshadow"), description = "Shadow mute a player", permission = "trchat.command.shadowmute") {
            dynamic("player") {
                suggestUncheck {
                    BukkitProxyManager.getPlayerNames(true).keys.toList()
                }
                execute<CommandSender> { sender, ctx, _ ->
                    muteShadow(sender, ctx["player"], "999d")
                }
                dynamic("time") {
                    suggestUncheck {
                        listOf("1d", "3h", "15m", "30s")
                    }
                    execute<CommandSender> { sender, ctx, _ ->
                        muteShadow(sender, ctx["player"], ctx["time"])
                    }
                }
            }
            incorrectCommand { _, _, _, _ ->
                createHelper()
            }
        }
        command("unmute", listOf("trunmute"), description = "Unmute a player", permission = "trchat.command.unmute") {
            dynamic("player") {
                suggestUncheck {
                    BukkitProxyManager.getPlayerNames(true).keys.toList()
                }
                execute<CommandSender> { sender, ctx, _ ->
                    val name = ctx["player"]
                    val player = Bukkit.getOfflinePlayer(name)
                    if (!player.hasPlayedBefore()) {
                        return@execute sender.sendLang("Command-Player-Not-Exist")
                    }
                    unmute(sender, player)
                }
            }
        }
    }

    fun mute(sender: CommandSender, name: String, time: String, reason: String) {
        val player = Bukkit.getOfflinePlayer(name)
        if (!player.hasPlayedBefore()) {
            return sender.sendLang("Command-Player-Not-Exist")
        }
        mute(sender, player, time, reason)
    }

    fun mute(sender: CommandSender?, player: OfflinePlayer, time: String, reason: String) {
        val data = player.data
        val millis = try {
            time.parseMillis()
        } catch (_: Throwable) {
            sender?.sendLang("Mute-Wrong-Format", time)
            return
        }
        data.updateMuteTime(millis)
        data.setMuteReason(reason)
        sender?.sendLang("Mute-Muted-Player", player.name ?: "unknown", time, reason)
        player.player?.sendLang("General-Muted", muteDateFormat.format(data.muteTime), data.muteReason)
        if (!player.isOnline) {
            PlayerData.removeData(player)
        }
    }

    fun muteShadow(sender: CommandSender, name: String, time: String) {
        val player = Bukkit.getOfflinePlayer(name)
        if (!player.hasPlayedBefore()) {
            return sender.sendLang("Command-Player-Not-Exist")
        }
        muteShadow(sender, player, time)
    }

    fun muteShadow(sender: CommandSender?, player: OfflinePlayer, time: String) {
        val data = player.data
        val millis = try {
            time.parseMillis()
        } catch (_: Throwable) {
            sender?.sendLang("Mute-Wrong-Format", time)
            return
        }
        data.updateShadowMuteTime(millis)
        sender?.sendLang("Mute-Muted-Player", player.name ?: "unknown", time, "shadow")
        if (!player.isOnline) {
            PlayerData.removeData(player)
        }
    }

    fun unmute(sender: CommandSender?, player: OfflinePlayer) {
        val data = player.data
        if (data.isMuted) {
            data.updateMuteTime(0)
            sender?.sendLang("Mute-Cancel-Muted-Player", player.name ?: "unknown")
            player.player?.sendLang("General-Cancel-Muted")
        }
        if (data.isShadowMuted) {
            data.updateShadowMuteTime(0)
            sender?.sendLang("Mute-Cancel-Muted-Player", player.name ?: "unknown")
        }
        if (!player.isOnline) {
            PlayerData.removeData(player)
        }
    }

    @Awake(LifeCycle.ENABLE)
    fun muteall() {
        if (Settings.conf.getStringList("Options.Disabled-Commands").contains("muteall")) return
        command("muteall", listOf("globalmute"), "Mute all players", permission = "trchat.command.muteall") {
            execute<CommandSender> { sender, _, _ ->
                TrChatBukkit.isGlobalMuting = !TrChatBukkit.isGlobalMuting
                if (TrChatBukkit.isGlobalMuting) {
                    sender.sendLang("Mute-Muted-All")
                } else {
                    sender.sendLang("Mute-Cancel-Muted-All")
                }
            }
        }
    }
}