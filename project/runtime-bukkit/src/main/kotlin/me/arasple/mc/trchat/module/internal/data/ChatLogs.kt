package me.arasple.mc.trchat.module.internal.data

import me.arasple.mc.trchat.module.conf.file.Settings
import me.arasple.mc.trchat.util.print
import taboolib.common.LifeCycle
import taboolib.common.io.newFile
import taboolib.common.platform.Awake
import taboolib.common.platform.Platform
import taboolib.common.platform.PlatformSide
import taboolib.common.platform.Schedule
import taboolib.common.platform.function.getDataFolder
import taboolib.common.util.replaceWithOrder
import taboolib.module.configuration.ConfigNode
import java.io.File
import java.text.SimpleDateFormat
import java.util.concurrent.TimeUnit

/**
 * @author Arasple
 * @date 2019/11/30 16:08
 */
@PlatformSide(Platform.BUKKIT)
object ChatLogs {

    private val waveList = mutableListOf<String>()
    private val dateFormat0 = SimpleDateFormat("yyyy-MM-dd")
    private val dateFormat1 = SimpleDateFormat("HH:mm:ss")

    @ConfigNode("Options.Log-Normal", "settings.yml")
    var logNormal = "[{0}] {1}: {2}"

    @ConfigNode("Options.Log-Private", "settings.yml")
    var logPrivate = "[{0}] {1} -> {2}: {3}"

    @Schedule(delay = (20 * 15).toLong(), period = (20 * 60 * 5).toLong(), async = true)
    @Awake(LifeCycle.DISABLE)
    fun writeToFile() {
        if (waveList.isEmpty()) {
            return
        }
        val logFile = newFile(File(getDataFolder(), "logs"), "${dateFormat0.format(System.currentTimeMillis())}.txt", create = true)
        try {
            waveList.forEach { line ->
                logFile.appendText(line + "\n")
            }
        } catch (t: Throwable) {
            t.print("Failed to save chat log!")
            return
        }
        waveList.clear()
    }

    @Schedule(period = (20 * 60 * 60).toLong(), async = true)
    fun autoDelete() {
        val days = Settings.conf.getLong("General.Log-Delete-Time", 0L)
        if (days > 0) {
            val millis = TimeUnit.DAYS.toMillis(days)
            kotlin.runCatching {
                File(getDataFolder(), "logs").walk().forEach {
                    if (it.lastModified() < (System.currentTimeMillis() - millis)) {
                        it.delete()
                    }
                }
            }
        }
    }

    fun logNormal(from: String, message: String) {
        waveList.add(
            logNormal.replaceWithOrder(dateFormat1.format(System.currentTimeMillis()), from, message)
        )
    }

    fun logPrivate(from: String, to: String, message: String) {
        waveList.add(
            logPrivate.replaceWithOrder(dateFormat1.format(System.currentTimeMillis()), from, to, message)
        )
    }
}