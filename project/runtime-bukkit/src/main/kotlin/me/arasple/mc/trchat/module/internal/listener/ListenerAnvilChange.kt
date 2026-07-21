package me.arasple.mc.trchat.module.internal.listener

import me.arasple.mc.trchat.TrChat
import me.arasple.mc.trchat.module.adventure.toAdventure
import me.arasple.mc.trchat.module.internal.TrChatBukkit
import me.arasple.mc.trchat.util.color.MessageColors
import me.arasple.mc.trchat.util.data
import me.arasple.mc.trchat.util.parseSimple
import me.arasple.mc.trchat.util.session
import org.bukkit.entity.HumanEntity
import org.bukkit.entity.Player
import org.bukkit.event.inventory.InventoryType
import org.bukkit.event.inventory.PrepareAnvilEvent
import org.bukkit.inventory.meta.ItemMeta
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.event.EventPriority
import taboolib.common.platform.event.SubscribeEvent
import taboolib.common.platform.function.adaptPlayer
import taboolib.library.reflex.Reflex.Companion.invokeMethod
import taboolib.module.configuration.ConfigNode
import taboolib.platform.util.isAir
import taboolib.platform.util.modifyMeta
import taboolib.platform.util.sendLang

/**
 * @author ItsFlicker
 * @date 2019/8/15 21:18
 */
@PlatformSide(Platform.BUKKIT)
object ListenerAnvilChange {

    @ConfigNode("Enable.Anvil", "filter.yml")
    var filter = true
        private set

    @ConfigNode("Color.Anvil", "settings.yml")
    var color = true
        private set

    @ConfigNode("Simple-Component.Anvil", "settings.yml")
    var simple = false
        private set

    @ConfigNode("Chat.Permission-Check.Anvil", "settings.yml")
    var anvilPermissionCheck = false

    @Suppress("Deprecation")
    @SubscribeEvent(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    fun onAnvilCraft(e: PrepareAnvilEvent) {
        // e.view -> AnvilView
        // java.lang.IncompatibleClassChangeError: Found class org.bukkit.inventory.InventoryView, but interface was expected
        val p = e.invokeMethod<Any>("getView")!!.invokeMethod<HumanEntity>("getPlayer")!! as? Player ?: return
        val result = e.result

        if (e.inventory.type != InventoryType.ANVIL || result.isAir()) {
            return
        }

        val left = e.inventory.getItem(0)      // 左侧输入槽物品
        val right = e.inventory.getItem(1)     // 右侧输入槽物品
        val resultName = result.itemMeta?.displayName
        val leftName = left?.itemMeta?.displayName
        val rightName = right?.itemMeta?.displayName
        val isRenaming = resultName != null && resultName != leftName && resultName != rightName

        if (anvilPermissionCheck && isRenaming && !canSpeak(p)) {
            e.result = null
            p.sendLang("Anvil-Edit-No-Permission")
            return
        }

        result.modifyMeta<ItemMeta> {
            if (!hasDisplayName()) {
                return@modifyMeta
            }
            if (filter) {
                val filtered = TrChat.api().getFilterManager().filter(displayName, adaptPlayer(p)).filtered
                if (filtered != displayName) {
                    setDisplayName(filtered)
                }
            }
            if (simple && TrChatBukkit.isPaperEnv && p.hasPermission("trchat.simple.anvil")) {
                displayName(displayName.parseSimple().toAdventure())
            } else if (color) {
                val colored = MessageColors.replaceWithPermission(p, displayName, MessageColors.Type.ANVIL)
                if (colored != displayName) {
                    setDisplayName(colored)
                }
            }
        }
        e.result = result
    }

    private fun canSpeak(player: Player): Boolean {
        if (TrChatBukkit.isGlobalMuting && !player.hasPermission("trchat.bypass.globalmute")) return false
        if (player.data.isMuted) return false
        val channel = player.session.getChannel()
        return channel == null || channel.canSpeak(player)
    }
}