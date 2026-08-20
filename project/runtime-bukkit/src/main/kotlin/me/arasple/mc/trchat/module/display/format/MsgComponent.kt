package me.arasple.mc.trchat.module.display.format

import me.arasple.mc.trchat.module.display.format.obj.Style
import me.arasple.mc.trchat.module.display.format.obj.Style.Companion.applyTo
import me.arasple.mc.trchat.module.display.function.Function
import me.arasple.mc.trchat.module.internal.script.Condition
import me.arasple.mc.trchat.module.conf.file.SpecialChars
import me.arasple.mc.trchat.util.color.wrapSpecialChars
import me.arasple.mc.trchat.util.color.hasSpecialChars
import me.arasple.mc.trchat.util.color.CustomColor
import me.arasple.mc.trchat.util.isDragonCoreHooked
import me.arasple.mc.trchat.util.pass
import me.arasple.mc.trchat.util.session
import net.kyori.adventure.text.Component
import net.kyori.adventure.text.TextComponent
import net.kyori.adventure.text.event.HoverEvent
import net.kyori.adventure.text.format.TextColor
import org.bukkit.command.CommandSender
import org.bukkit.entity.Player
import taboolib.common.util.VariableReader
import taboolib.module.chat.ComponentText
import taboolib.module.chat.Components
import taboolib.module.chat.impl.AdventureComponent

/**
 * @author ItsFlicker
 * @since 2021/12/12 13:46
 */
class MsgComponent(
    val defaultColor: List<Pair<CustomColor, Condition?>>,
    style: List<Style>,
    val specialCharsEnabled: Boolean = false,
    val specialCharsColor: String = "&f"
) : JsonComponent(style = style) {

    private fun processComponent(sender: CommandSender, msg: TextComponent, disabledFunctions: List<String>, depth: Int = 1): TextComponent {
        val children = msg.children().map { processComponent(sender, it as TextComponent, disabledFunctions, depth + 1) }
        var new = createComponent(sender, msg.content(), disabledFunctions).toAdventureObject() as TextComponent
        val style = new.style().merge(msg.style())
        new = new.children(new.children() + children).style(style)
        // https://github.com/TrPlugins/TrChat/issues/511 粗暴的解决方法 (主要是由于不同children的错位，merge无法解决)
        if (msg.content().isNotBlank() && msg.children().isEmpty()) {
            val color = msg.style().color()
            if (color != null) {
                new = changeColor(new, color) as TextComponent
            }
            val hover = msg.style().hoverEvent()
            if (hover != null) {
                new = changeHover(new, hover) as TextComponent
            }
        }
        return new
    }

    fun createComponent(sender: CommandSender, msg: ComponentText, disabledFunctions: List<String>): ComponentText {
        if (!Components.useAdventure) {
            return createComponent(sender, msg.toPlainText(), disabledFunctions)
        }
        val component = msg.toAdventureObject() as TextComponent
        return AdventureComponent(processComponent(sender, component, disabledFunctions))
    }

    fun createComponent(sender: CommandSender, msg: String, disabledFunctions: List<String>): ComponentText {
        if (msg.isBlank()) return Components.empty()
        val component = Components.empty()
//        var message = HookPlugin.getItemsAdder().replaceFontImages(msg, sender as? Player)
        var message = msg.replace("{{", "\\{{")

        // 非玩家 不处理functions
        if (sender !is Player) {
            val defaultColor = defaultColor[0].first
            return toTextComponent(sender, defaultColor.colored(sender, message))
        }

        // 创建{{xxx:xxx}}
        Function.functions.filter { it.alias !in disabledFunctions && it.canUse(sender) }.forEach {
            message = it.createVariable(sender, message)
        }

        val defaultColor = sender.session.getColor(defaultColor.firstOrNull { it.second.pass(sender) }?.first)

        val channelColor = defaultColor.color
        val specialCharsPrefix  = if (this.specialCharsEnabled)
            CustomColor.get(this.specialCharsColor).color else ""

        // 分割为多个ComponentText
        for (part in parser.readToFlatten(message)) {
            if (part.isVariable) {
                val args = part.text.split(":", limit = 2)
                val function = Function.functions.firstOrNull { it.id == args[0] }
                if (function != null) {
                    function.parseVariable(sender, Function.pop(args[1].toInt()))?.let { component.append(it) }
                    function.reaction?.eval(sender, "message" to message)
                }
                continue
            }
            val coloredText = defaultColor.colored(sender,  part.text)
            val text = if (specialCharsPrefix.isNotEmpty() && coloredText.hasSpecialChars(SpecialChars.specialChars))
                coloredText.wrapSpecialChars(specialCharsPrefix, channelColor, SpecialChars.specialChars)
            else coloredText
            component.append(toTextComponent(sender, text))
        }
        return component
    }

    override fun toTextComponent(sender: CommandSender, vararg vars: String): ComponentText {
        val message = vars[0]
        val component = if (isDragonCoreHooked) {
            Components.text(message, color = false)
        } else {
            Components.text(message)
        }
        style.forEach {
            it.applyTo(component, sender, *vars)
        }
        return component
    }

    companion object {

        private val parser = VariableReader()

        private fun changeColor(msg: Component, color: TextColor): Component {
            val new = if (msg.style().color() != null) {
                msg.style(msg.style().color(color))
            } else {
                msg
            }
            val children = new.children().map { changeColor(it, color) }
            return new.children(children)
        }

        private fun changeHover(msg: Component, hover: HoverEvent<*>): Component {
            val new = if (msg.style().hoverEvent() != null) {
                msg.style(msg.style().hoverEvent(hover))
            } else {
                msg
            }
            val children = new.children().map { changeHover(it, hover) }
            return new.children(children)
        }
    }
}