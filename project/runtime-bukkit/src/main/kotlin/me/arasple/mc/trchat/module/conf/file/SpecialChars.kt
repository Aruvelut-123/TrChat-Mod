package me.arasple.mc.trchat.module.conf.file

import taboolib.common.LifeCycle
import taboolib.common.platform.Awake
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.module.configuration.Config
import taboolib.module.configuration.Configuration

/**
 * @author XyLuoDYS
 * @since 2026/8/18 19:14
 */
@PlatformSide(Platform.BUKKIT)
object SpecialChars {

    @Config("special-chars.yml")
    lateinit var conf: Configuration
        private set

    var specialChars: Set<String> = emptySet()
        private set

    @Awake(LifeCycle.ENABLE)
    fun init() {
        conf.onReload { reload() }
    }

    fun reload() {
        specialChars = conf.getStringList("SpecialChars").toSet()
    }
}