package me.arasple.mc.trchat.module.display.function.standard

import com.google.common.cache.Cache
import com.google.common.cache.CacheBuilder
import me.arasple.mc.trchat.api.impl.BukkitProxyManager
import me.arasple.mc.trchat.module.conf.file.Functions
import me.arasple.mc.trchat.module.display.function.Function
import me.arasple.mc.trchat.module.display.function.StandardFunction
import me.arasple.mc.trchat.module.internal.script.Reaction
import me.arasple.mc.trchat.util.*
import org.bukkit.entity.Player
import org.bukkit.inventory.Inventory
import org.bukkit.inventory.ItemStack
import taboolib.common.io.digest
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.util.asList
import taboolib.common.util.resettableLazy
import taboolib.common5.util.encodeBase64
import taboolib.common5.util.parseMillis
import taboolib.library.xseries.XMaterial
import taboolib.module.chat.ComponentText
import taboolib.module.configuration.ConfigNode
import taboolib.module.configuration.ConfigNodeTransfer
import taboolib.module.nms.MinecraftVersion
import taboolib.module.ui.buildMenu
import taboolib.module.ui.type.PageableChest
import taboolib.platform.util.*

/**
 * @author ItsFlicker
 * @since 2022/3/18 19:14
 */
@StandardFunction
@PlatformSide(Platform.BUKKIT)
object InventoryShow : Function("INVENTORY") {

    override val alias = "Inventory-Show"

    override val reaction by resettableLazy("functions") {
        Functions.conf["General.Inventory-Show.Action"]?.let { Reaction(it.asList()) }
    }

    @ConfigNode("General.Inventory-Show.Enabled", "function.yml")
    var enabled = true

    @ConfigNode("General.Inventory-Show.Permission", "function.yml")
    var permission = "none"

    @ConfigNode("General.Inventory-Show.Cooldown", "function.yml")
    val cooldown = ConfigNodeTransfer<String, Long> { parseMillis() }

    @ConfigNode("General.Inventory-Show.Keys", "function.yml")
    var keys = listOf<String>()

    val keysRegex by resettableLazy("functions") {
        keys.map { Regex(Regex.escape(it), RegexOption.IGNORE_CASE) }
    }

    val cache: Cache<String, Inventory> = CacheBuilder.newBuilder()
        .maximumSize(10)
        .build()

    private val inventorySlots = IntRange(18, 53).toList()
    private val AIR_ITEM = buildItem(XMaterial.GRAY_STAINED_GLASS_PANE) { name = "§f" }
    private val PLACEHOLDER_ITEM = buildItem(XMaterial.WHITE_STAINED_GLASS_PANE) { name = "§f" }

    override fun createVariable(sender: Player, message: String): String {
        if (!enabled) {
            return message
        }
        var result = message
        keysRegex.forEach {
            result = result.replace(it) { "{{INVENTORY:${push(sender.name)}}}" }
        }
        return result
    }

    override fun parseVariable(sender: Player, arg: String): ComponentText? {
        return computeAndCache(sender).let {
            BukkitProxyManager.sendMessage(sender, arrayOf(
                "ForwardMessage",
                "InventoryShow",
                MinecraftVersion.minecraftVersion,
                sender.name,
                it.first,
                it.second)
            )
            sender.getComponentFromLang("Function-Inventory-Show-Format", sender.name, it.first)
        }
    }

    override fun canUse(sender: Player): Boolean {
        return sender.passPermission(permission)
    }

    override fun checkCooldown(sender: Player, message: String): Boolean {
        if (enabled && keys.any { message.contains(it, ignoreCase = true) } && !sender.hasPermission("trchat.bypass.inventorycd")) {
            val inventoryCooldown = sender.getCooldownLeft(CooldownType.INVENTORY_SHOW)
            if (inventoryCooldown > 0) {
                sender.sendLang("Cooldowns-Inventory-Show", inventoryCooldown / 1000)
                return false
            } else {
                sender.updateCooldown(CooldownType.INVENTORY_SHOW, cooldown.get())
            }
        }
        return true
    }

    @Suppress("Deprecation")
    fun computeAndCache(sender: Player): Pair<String, String> {
        val inventory = sender.inventory
        val sha1 = inventory.serializeToByteArray(zipped = false).encodeBase64().digest("sha-1")
        if (cache.getIfPresent(sha1) != null) {
            return sha1 to cache.getIfPresent(sha1)!!.serializeToByteArray().encodeBase64()
        }
        val menu = buildMenu<PageableChest<ItemStack>>(sender.asLangText("Function-Inventory-Show-Title", sender.name)) {
            rows(6)
            slots(inventorySlots)
            elements {
                (9..35).map { inventory.getItem(it).replaceAir() } +
                        (0..8).map { inventory.getItem(it).replaceAir() }
            }
            onGenerate { _, element, _, _ -> element }
            onInventoryCreate { inv ->
                inv.setItem(0, PLACEHOLDER_ITEM)
                inv.setItem(1, inventory.runCatching { itemInOffHand }.getOrDefault(AIR_ITEM).replaceAir())
                inv.setItem(2, buildItem(XMaterial.PLAYER_HEAD) {
                    name = "§e${sender.name}"
                    skullOwner = sender.name
                })
                inv.setItem(3, inventory.itemInHand.replaceAir())
                inv.setItem(4, PLACEHOLDER_ITEM)
                val armor = inventory.armorContents
                inv.setItem(5, armor[3].replaceAir()) // 头盔
                inv.setItem(6, armor[2].replaceAir()) // 胸甲
                inv.setItem(7, armor[1].replaceAir()) // 护腿
                inv.setItem(8, armor[0].replaceAir()) // 靴子
                (9..17).forEach { slot -> inv.setItem(slot, PLACEHOLDER_ITEM) }
            }
            onClick(lock = true)
        }
        cache.put(sha1, menu)
        return sha1 to menu.serializeToByteArray().encodeBase64()
    }

    private fun ItemStack?.replaceAir() = if (isAir()) AIR_ITEM else this

}