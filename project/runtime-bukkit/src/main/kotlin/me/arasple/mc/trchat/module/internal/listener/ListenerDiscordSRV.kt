package me.arasple.mc.trchat.module.internal.listener

import github.scarsz.discordsrv.api.Subscribe
import github.scarsz.discordsrv.api.events.DiscordGuildMessagePreBroadcastEvent
import github.scarsz.discordsrv.api.events.GameChatMessagePreProcessEvent
import github.scarsz.discordsrv.dependencies.kyori.adventure.text.serializer.gson.GsonComponentSerializer
import me.arasple.mc.trchat.api.impl.BukkitComponentManager
import me.arasple.mc.trchat.module.display.format.MsgComponent
import me.arasple.mc.trchat.module.display.function.Function
import me.arasple.mc.trchat.module.internal.hook.hookDiscordSRV
import me.arasple.mc.trchat.util.pass
import me.arasple.mc.trchat.util.session
import org.bukkit.entity.Player
import taboolib.common.LifeCycle
import taboolib.common.platform.Awake
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.module.chat.Components

class ListenerDiscordSRV {

    @Subscribe
    fun onChatPreProcess(e: GameChatMessagePreProcessEvent) {
        val player = e.player
        val channel = player.session.lastChannel ?: return
        if (channel.settings.sendToDiscord) {
            val origin = Components.parseRaw(GsonComponentSerializer.gson().serialize(e.messageComponent))
            val component = (channel.formats
                .firstOrNull { it.condition.pass(player) }?.msg
                ?.firstOrNull { it.condition.pass(player) }?.content as? MsgComponent)
                ?.createComponent(player, BukkitComponentManager.filterComponent(origin), channel.settings.disabledFunctions)
                ?: return
            // Discord 转发渲染不产生游戏内 @ 提示，清空避免污染后续聊天流程
            Function.clearMentioned()
            e.messageComponent = GsonComponentSerializer.gson().deserialize(component.toRawMessage())
            if (channel.settings.discordChannel.isNotEmpty()) {
                e.channel = channel.settings.discordChannel
            }
        } else {
            e.isCancelled = true
        }
    }

    @Subscribe
    fun onMessagePreBroadcast(e: DiscordGuildMessagePreBroadcastEvent) {
        e.recipients.removeIf {
            (it as? Player)?.session?.getChannel()?.settings?.receiveFromDiscord == false
        }
    }

    @PlatformSide(Platform.BUKKIT)
    companion object {

        @Awake(LifeCycle.ACTIVE)
        fun register() {
            if (hookDiscordSRV.isHooked) {
                hookDiscordSRV.registerListener(ListenerDiscordSRV())
            }
        }
    }
}