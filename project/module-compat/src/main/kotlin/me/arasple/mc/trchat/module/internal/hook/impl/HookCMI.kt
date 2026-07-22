package me.arasple.mc.trchat.module.internal.hook.impl

import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.module.incision.annotation.Excise
import taboolib.module.incision.annotation.Surgeon
import taboolib.module.incision.api.Theatre

@Surgeon
@PlatformSide(Platform.BUKKIT)
object HookCMI {

    @Excise("com.Zrips.CMI.Modules.Chat.ChatManager#isModifyChatFormat()Z")
    fun disableModifyChatFormat(theatre: Theatre) {
        theatre.override(false)
    }

    @Excise("com.Zrips.CMI.Modules.Chat.ChatManager#isChatClickHoverMessages()Z")
    fun disableChatClickHoverMessages(theatre: Theatre) {
        theatre.override(false)
    }

}