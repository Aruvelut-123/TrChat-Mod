package me.arasple.mc.trchat.module.display.function

import me.arasple.mc.trchat.api.event.TrChatReloadEvent
import me.arasple.mc.trchat.module.internal.script.Reaction
import org.bukkit.entity.Player
import taboolib.common.io.runningClassesWithoutLibrary
import taboolib.common.platform.function.warning
import taboolib.module.chat.ComponentText
import java.util.concurrent.atomic.AtomicInteger

abstract class Function(val id: String) {

    open val alias = id

    open val reaction: Reaction? = null

    abstract fun createVariable(sender: Player, message: String): String

    abstract fun parseVariable(sender: Player, arg: String): ComponentText?

    abstract fun canUse(sender: Player): Boolean

    abstract fun checkCooldown(sender: Player, message: String): Boolean

    companion object {

        @JvmStatic
        val functions = mutableListOf<Function>()

        @JvmStatic
        private val args = arrayOfNulls<String>(1000)

        private val cur = AtomicInteger(0)

        fun push(arg: Any): Int {
            val i = cur.getAndIncrement() % 1000
            if (args[i] != null) {
                warning("Unexpected function 'args' status! Please contact the maintainer!")
            }
            args[i] = arg.toString()
            return i
        }

        fun pop(i: Int): String {
            return args[i]!!.also { args[i] = null }
        }

        fun reload(customFunctions: List<CustomFunction>) {
            functions.clear()
            functions.addAll(runningClassesWithoutLibrary
                .filter { it.hasAnnotation(StandardFunction::class.java) }
                .mapNotNull { it.getInstance() as? Function }
            )
            functions.addAll(customFunctions)
            TrChatReloadEvent.Function(functions).call()
        }
    }
}