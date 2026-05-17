package me.arasple.mc.trchat.util

import me.arasple.mc.trchat.TrChat
import me.arasple.mc.trchat.api.nms.NMS
import me.arasple.mc.trchat.module.adventure.hoverItemAdventure
import me.arasple.mc.trchat.util.color.colorify
import net.md_5.bungee.api.chat.ComponentBuilder
import net.md_5.bungee.api.chat.HoverEvent
import org.bukkit.Material
import org.bukkit.block.ShulkerBox
import org.bukkit.entity.Player
import org.bukkit.inventory.ItemStack
import org.bukkit.inventory.meta.BlockStateMeta
import org.bukkit.inventory.meta.ItemMeta
import taboolib.common.platform.ProxyCommandSender
import taboolib.common.platform.ProxyPlayer
import taboolib.common.platform.function.adaptPlayer
import taboolib.common.platform.function.severe
import taboolib.common.util.VariableReader
import taboolib.common.util.replaceWithOrder
import taboolib.module.chat.ComponentText
import taboolib.module.chat.Components
import taboolib.module.chat.component
import taboolib.module.chat.impl.DefaultComponent
import taboolib.module.lang.*
import taboolib.module.nms.MinecraftVersion.versionId
import taboolib.module.nms.NMSItemTag
import taboolib.module.nms.getI18nName
import taboolib.platform.util.*

fun String.parseSimple() = component().build {
    transform { it.colorify() }
}

fun ComponentText.hoverItemFixed(item: ItemStack): ComponentText {
    var newItem = item.optimizeShulkerBox()
    if (Components.useAdventure) {
        return hoverItemAdventure(newItem)
    }
    newItem = NMS.instance.optimizeNBT(newItem)
    if (versionId >= 12005) {
        (this as DefaultComponent).latest.forEach {
            it.hoverEvent = HoverEvent(HoverEvent.Action.SHOW_ITEM, ComponentBuilder(NMSItemTag.instance.toMinecraftJson(newItem)).create())
        }
        return this
    }
    return try {
        // https://github.com/TrPlugins/TrChat/issues/363
        NMS.instance.hoverItem(this, newItem)
    } catch (_: Throwable) {
        try {
            hoverItem(newItem)
        } catch (_: Throwable) {
            hoverText("Unable to display this item! Click to view it.")
        }
    }
}

@Suppress("Deprecation")
fun ItemStack.optimizeShulkerBox(): ItemStack {
    if (!type.name.endsWith("SHULKER_BOX")) {
        return this
    }
    try {
        val itemClone = clone()
        val blockStateMeta = itemClone.itemMeta!! as BlockStateMeta
        val shulkerBox = blockStateMeta.blockState as ShulkerBox
        val contents = shulkerBox.inventory.contents
        val contentsClone = contents.mapNotNull {
            if (it.isNotAir()) {
                ItemStack(Material.STONE, it.amount, it.durability).modifyMeta<ItemMeta> {
                    if (it.itemMeta?.hasDisplayName() == true) {
                        setDisplayName(it.itemMeta!!.displayName)
                    } else {
                        setDisplayName(it.getI18nName())
                    }
                }
            } else {
                null
            }
        }.toTypedArray()
        shulkerBox.inventory.contents = contentsClone
        blockStateMeta.blockState = shulkerBox
        itemClone.itemMeta = blockStateMeta
        return itemClone
    } catch (_: Throwable) {
    }
    return this
}

@Suppress("Deprecation")
fun ItemStack.filter() {
    if (isAir()) return
    modifyMeta<ItemMeta> {
        if (hasDisplayName()) {
            setDisplayName(TrChat.api().getFilterManager().filter(displayName).filtered)
        }
        modifyLore {
            if (isNotEmpty()) {
                replaceAll { TrChat.api().getFilterManager().filter(it).filtered }
            }
        }
    }
}

private val parser = VariableReader("[", "]")

fun Player.getComponentFromLang(
    node: String,
    vararg args: Any,
    processor: (TypeJson, Int, VariableReader.Part, ProxyPlayer) -> ComponentText = { type, i, part, sender ->
        Components.text(part.text.translate(sender).replaceWithOrder(*args)).applyStyle(type, part, i, sender, *args)
    }
): ComponentText? {
    val sender = adaptPlayer(this)
    val file = sender.getLocaleFile() ?: return null
    return when (val type = file.nodes[node].let { if (it is TypeList) it.list[0] else it }) {
        is TypeJson -> {
            var i = 0
            val component = Components.empty()
            parser.readToFlatten(type.text!!.joinToString()).forEach { part ->
                component.append(processor(type, i, part, sender))
                if (part.isVariable) {
                    i++
                }
            }
            component
        }
        is TypeText -> {
            Components.text(type.asText(adaptPlayer(this), *args)!!)
        }
        else -> {
            severe("Error language type (Required TypeJson or TypeText)")
            null
        }
    }
}

fun ComponentText.applyStyle(type: TypeJson, part: VariableReader.Part, i: Int, sender: ProxyPlayer, vararg args: Any): ComponentText {
    val extra = type.jsonArgs.getOrNull(i)
    if (extra != null) {
        if (extra.containsKey("hover")) {
            hoverText(extra["hover"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("command")) {
            clickRunCommand(extra["command"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("suggest")) {
            clickSuggestCommand(extra["suggest"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("insertion")) {
            clickInsertText(extra["insertion"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("copy")) {
            clickCopyToClipboard(extra["copy"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("file")) {
            clickOpenFile(extra["file"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("url")) {
            clickOpenURL(extra["url"].toString().translate(sender).replaceWithOrder(*args))
        }
        if (extra.containsKey("font")) {
            font(extra["font"].toString().translate(sender).replaceWithOrder(*args))
        }
    }
    return this
}

internal fun String.translate(sender: ProxyCommandSender): String {
    var s = this
    Language.textTransfer.forEach { s = it.translate(sender, s) }
    return s
}