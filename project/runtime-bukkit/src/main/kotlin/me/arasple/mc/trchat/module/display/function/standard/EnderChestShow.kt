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
object EnderChestShow : Function("ENDERCHEST") {

    override val alias = "EnderChest-Show"

    override val reaction by resettableLazy("functions") {
        Functions.conf["General.EnderChest-Show.Action"]?.let { Reaction(it.asList()) }
    }

    @ConfigNode("General.EnderChest-Show.Enabled", "function.yml")
    var enabled = true

    @ConfigNode("General.EnderChest-Show.Permission", "function.yml")
    var permission = "none"

    @ConfigNode("General.EnderChest-Show.Cooldown", "function.yml")
    val cooldown = ConfigNodeTransfer<String, Long> { parseMillis() }

    @ConfigNode("General.EnderChest-Show.Keys", "function.yml")
    var keys = listOf<String>()

    val keysRegex by resettableLazy("functions") {
        keys.map { Regex(Regex.escape(it), RegexOption.IGNORE_CASE) }
    }

    val cache: Cache<String, Inventory> = CacheBuilder.newBuilder()
        .maximumSize(10)
        .build()

    private val AIR_ITEM = buildItem(XMaterial.GRAY_STAINED_GLASS_PANE) { name = "§f" }

    override fun createVariable(sender: Player, message: String): String {
        if (!enabled) {
            return message
        }
        var result = message
        keysRegex.forEach {
            result = result.replace(it) { "{{ENDERCHEST:${push(sender.name)}}}" }
        }
        return result
    }

    override fun parseVariable(sender: Player, arg: String): ComponentText? {
        return computeAndCache(sender).let {
            BukkitProxyManager.sendMessage(sender, arrayOf(
                "ForwardMessage",
                "EnderChestShow",
                MinecraftVersion.minecraftVersion,
                sender.name,
                it.first,
                it.second)
            )
            sender.getComponentFromLang("Function-EnderChest-Show-Format", sender.name, it.first)
        }
    }

    override fun canUse(sender: Player): Boolean {
        return sender.passPermission(permission)
    }

    override fun checkCooldown(sender: Player, message: String): Boolean {
        if (enabled && keys.any { message.contains(it, ignoreCase = true) } && !sender.hasPermission("trchat.bypass.enderchestcd")) {
            val enderChestCooldown = sender.getCooldownLeft(CooldownType.ENDERCHEST_SHOW)
            if (enderChestCooldown > 0) {
                sender.sendLang("Cooldowns-EnderChest-Show", enderChestCooldown / 1000)
                return false
            } else {
                sender.updateCooldown(CooldownType.ENDERCHEST_SHOW, cooldown.get())
            }
        }
        return true
    }

    fun computeAndCache(sender: Player): Pair<String, String> {
        val inventory = sender.enderChest
        val sha1 = inventory.serializeToByteArray(zipped = false).encodeBase64().digest("sha-1")
        if (cache.getIfPresent(sha1) != null) {
            return sha1 to cache.getIfPresent(sha1)!!.serializeToByteArray().encodeBase64()
        }
        val menu = buildMenu<PageableChest<ItemStack>>(sender.asLangText("Function-EnderChest-Show-Title", sender.name)) {
            rows(3)
            slots((0..26).toList())
            elements { (0..26).map { inventory.getItem(it).replaceAir() } }
            onGenerate { _, element, _, _ -> element }
            onClick(lock = true)
        }
        cache.put(sha1, menu)
        return sha1 to menu.serializeToByteArray().encodeBase64()
    }

    private fun ItemStack?.replaceAir() = if (isAir()) AIR_ITEM else this

}