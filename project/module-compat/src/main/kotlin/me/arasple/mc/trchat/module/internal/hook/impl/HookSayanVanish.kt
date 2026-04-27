package me.arasple.mc.trchat.module.internal.hook.impl

import me.arasple.mc.trchat.module.internal.hook.Hook
import me.arasple.mc.trchat.module.internal.hook.type.HookVanish
import org.bukkit.OfflinePlayer
import taboolib.common.platform.Platform
import taboolib.common.util.unsafeLazy
import java.lang.reflect.Method

@Hook([Platform.BUKKIT])
class HookSayanVanish : HookVanish() {

    private val sayanVanishApiCompanion by unsafeLazy {
        runCatching {
            val apiClass = Class.forName("org.sayandev.sayanvanish.bukkit.api.SayanVanishBukkitAPI")
            apiClass.getDeclaredField("Companion").get(null)
        }.getOrNull()
    }

    private val userMethod: Method? by unsafeLazy {
        sayanVanishApiCompanion?.javaClass?.methods?.firstOrNull {
            it.name == "user" && it.parameterCount == 1 && OfflinePlayer::class.java.isAssignableFrom(it.parameterTypes[0])
        }
    }

    private val isVanishedMethod: Method? by unsafeLazy {
        runCatching {
            val user = userMethod?.invoke(sayanVanishApiCompanion, org.bukkit.Bukkit.getOfflinePlayer(java.util.UUID(0, 0)))
            user?.javaClass?.methods?.firstOrNull { it.name == "isVanished" && it.parameterCount == 0 }
        }.getOrNull()
    }

    override fun isVanished(player: OfflinePlayer): Boolean {
        if (!isHooked) {
            return false
        }
        val user = runCatching {
            userMethod?.invoke(sayanVanishApiCompanion, player)
        }.getOrNull() ?: return false
        return runCatching {
            isVanishedMethod?.invoke(user) as? Boolean ?: false
        }.getOrDefault(false)
    }

}
