package me.arasple.mc.trchat.module.internal

import me.arasple.mc.trchat.TrChat
import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.api.nms.NMS
import me.arasple.mc.trchat.module.conf.file.Filters
import me.arasple.mc.trchat.module.conf.file.Functions
import me.arasple.mc.trchat.module.conf.file.Settings
import me.arasple.mc.trchat.module.conf.file.SpecialChars
import me.arasple.mc.trchat.module.display.ChatSession
import me.arasple.mc.trchat.module.display.channel.Channel
import me.arasple.mc.trchat.module.display.function.Function
import me.arasple.mc.trchat.module.internal.data.PlayerData
import me.arasple.mc.trchat.module.internal.hook.HookPlugin
import me.arasple.mc.trchat.util.YamlUpdater
import me.arasple.mc.trchat.util.hasClass
import org.bukkit.Bukkit
import taboolib.common.LifeCycle
import taboolib.common.platform.*
import taboolib.common.platform.function.console
import taboolib.common.platform.function.pluginVersion
import taboolib.module.chat.Components
import taboolib.module.lang.sendLang
import taboolib.module.nms.MinecraftVersion
import taboolib.module.nms.MinecraftVersion.versionId
import taboolib.platform.Folia

@PlatformSide(Platform.BUKKIT)
object TrChatBukkit : Plugin() {

    var isPaperEnv = false
        private set

    var isGlobalMuting = false

    var isActivated = false

    internal fun detectPaperEnv() {
        if ((hasClass("com.destroystokyo.paper.PaperConfig")
            || hasClass("io.papermc.paper.configuration.Configuration"))
            && versionId >= 11604) {
            isPaperEnv = true
        }
        if (Folia.isFolia || (isPaperEnv && MinecraftVersion.isHigherOrEqual(MinecraftVersion.V1_20))) {
            Components.useAdventure = true
        }
    }

    @Awake(LifeCycle.CONST)
    internal fun onConst() {
        detectPaperEnv()
    }

//    @Awake(LifeCycle.INIT)
    internal fun updateConfigs() {
        YamlUpdater.update("settings.yml", updateExists = false)
    }

    override fun onLoad() {
        console().sendLang("Plugin-Loading", Bukkit.getBukkitVersion())
    }

    override fun onEnable() {
//        if (!Settings.usePackets
//            || Folia.isFolia
//            || Bukkit.getPluginManager().isPluginEnabled("Geyser-Spigot")
//            || versionId >= 12005
//            ) disablePacketListener()
        NMS.instance
        BukkitProxyManager.processor
        HookPlugin.printInfo()
        reload(console())
        console().sendLang("Plugin-Enabled", pluginVersion)
    }

    override fun onActive() {
        isActivated = true
    }

    override fun onDisable() {
        BukkitProxyManager.close()

        ChatSession.sessions.clear()
        PlayerData.data.clear()
        Channel.channels.clear()
        Function.functions.clear()
    }

    fun reload(notify: ProxyCommandSender) {
        Settings.conf.reload()
        Functions.conf.reload()
        Filters.conf.reload()
        SpecialChars.conf.reload()
        TrChat.api().getChannelManager().loadChannels(notify)
    }

}
