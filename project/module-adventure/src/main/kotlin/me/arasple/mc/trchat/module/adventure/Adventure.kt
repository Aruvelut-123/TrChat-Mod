package me.arasple.mc.trchat.module.adventure

import net.kyori.adventure.text.Component
import net.kyori.adventure.text.minimessage.MiniMessage
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer
import org.bukkit.inventory.ItemStack
import taboolib.module.chat.ComponentText
import taboolib.module.chat.Components
import taboolib.module.chat.impl.AdventureComponent

private val legacySerializer: Any? = try {
    LegacyComponentSerializer.legacySection()
} catch (_: Throwable) {
    null
}

private val gsonSerializer: Any? = try {
    GsonComponentSerializer.gson()
} catch (_: Throwable) {
    null
}

private val plainSerializer: Any? = try {
    PlainTextComponentSerializer.plainText()
} catch (_: Throwable) {
    null
}

val miniMessage: Any? = try {
    MiniMessage.miniMessage()
} catch (_: Throwable) {
    null
}

fun gson(component: Component) = (gsonSerializer as GsonComponentSerializer).serialize(component)

fun gson(string: String) = (gsonSerializer as GsonComponentSerializer).deserialize(string)

fun Component.toPlain() = (plainSerializer as PlainTextComponentSerializer).serialize(this)

fun ComponentText.toAdventure(): Component {
    return if (this is AdventureComponent) this.component
    else gson(this.toRawMessage())
}

fun Component.toNative(): ComponentText {
    return if (Components.useAdventure) AdventureComponent(this)
    else Components.parseRaw(gson(this))
}

fun ComponentText.hoverItemAdventure(item: ItemStack): ComponentText {
    this as? AdventureComponent ?: error("Unsupported component type.")
    this.latest.hoverEvent(item.asHoverEvent())
    return this
}

fun String.parseMiniMessage(): ComponentText {
    val mm = miniMessage as? MiniMessage ?: return Components.text("No MiniMessage support in your environment!")
    val component = mm.deserialize(this)
    return if (Components.useAdventure) {
        AdventureComponent(component)
    } else {
        Components.parseRaw(gson(component))
    }
}