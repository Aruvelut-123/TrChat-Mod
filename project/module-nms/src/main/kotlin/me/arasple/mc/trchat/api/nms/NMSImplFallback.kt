package me.arasple.mc.trchat.api.nms

import org.bukkit.entity.Player
import org.bukkit.inventory.ItemStack
import taboolib.common.platform.function.adaptPlayer
import taboolib.module.chat.ComponentText
import taboolib.module.nms.MinecraftLanguage
import taboolib.module.nms.getLanguageKey
import taboolib.platform.util.hoverItem
import java.util.*

class NMSImplFallback : NMS() {

    override fun craftChatMessageFromComponent(component: ComponentText): Any {
        return Any()
    }

    override fun rawMessageFromCraftChatMessage(component: Any): String {
        return "{}"
    }

    override fun sendMessage(receiver: Player, component: ComponentText, sender: UUID?, usePacket: Boolean) {
        component.sendTo(adaptPlayer(receiver))
    }

    override fun hoverItem(component: ComponentText, itemStack: ItemStack): ComponentText {
        return component.hoverItem(itemStack)
    }

    override fun optimizeNBT(itemStack: ItemStack, nbtWhitelist: Array<String>): ItemStack {
        return itemStack
    }

    override fun getLocaleKey(itemStack: ItemStack): MinecraftLanguage.LanguageKey {
        return itemStack.getLanguageKey()
    }
}