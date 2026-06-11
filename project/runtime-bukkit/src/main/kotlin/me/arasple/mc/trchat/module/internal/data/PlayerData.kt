package me.arasple.mc.trchat.module.internal.data

import me.arasple.mc.trchat.module.display.channel.Channel
import me.arasple.mc.trchat.util.parseString
import org.bukkit.OfflinePlayer
import taboolib.common5.cbool
import taboolib.common5.clong
import taboolib.expansion.getAutoDataContainer
import taboolib.expansion.releaseAutoDataContainer
import java.util.*
import java.util.concurrent.ConcurrentHashMap

/**
 * @author ItsFlicker
 * @since 2022/6/25 18:17
 */
class PlayerData(val uuid: UUID) {

    init {
        if (isSpying) {
            spying += uuid
        }
    }

    val color get() = uuid.getAutoDataContainer()["color"].takeIf { it != "null" }

    val channel get() = uuid.getAutoDataContainer()["channel"]

    val isSpying get() = uuid.getAutoDataContainer()["spying"].cbool

    val isFilterEnabled get() = uuid.getAutoDataContainer()["filter"]?.cbool ?: true

    val muteTime get() = uuid.getAutoDataContainer()["mute_time"].clong

    val isMuted get() = muteTime > System.currentTimeMillis()

    val muteReason get() = uuid.getAutoDataContainer()["mute_reason"] ?: "null"

    val shadowMuteTime get() = uuid.getAutoDataContainer()["shadow_mute_time"].clong

    val isShadowMuted get() = shadowMuteTime > System.currentTimeMillis()

    var ignored get() = uuid.getAutoDataContainer()["ignored"]
        ?.takeIf { it.isNotBlank() }
        ?.split(",")
        ?.toSet()
        ?: emptySet()
        set(value) { uuid.getAutoDataContainer()["ignored"] = value.joinToString(",") }

    fun setChannel(channel: Channel) {
        uuid.getAutoDataContainer()["channel"] = channel.id
    }

    fun selectColor(color: String) {
        uuid.getAutoDataContainer()["color"] = color
    }

    fun setFilter(value: Boolean) {
        uuid.getAutoDataContainer()["filter"] = value
    }

    fun updateMuteTime(time: Long) {
        uuid.getAutoDataContainer()["mute_time"] = System.currentTimeMillis() + time
    }

    fun setMuteReason(reason: String) {
        uuid.getAutoDataContainer()["mute_reason"] = reason
    }

    fun updateShadowMuteTime(time: Long) {
        uuid.getAutoDataContainer()["shadow_mute_time"] = System.currentTimeMillis() + time
    }

    fun switchSpy(): Boolean {
        uuid.getAutoDataContainer()["spying"] = !isSpying
        return isSpying.also {
            if (it) spying += uuid else spying -= uuid
        }
    }

    fun addIgnored(uuid: UUID) {
        ignored = ignored + uuid.parseString()
    }

    fun removeIgnored(uuid: UUID) {
        ignored = ignored - uuid.parseString()
    }

    fun hasIgnored(uuid: UUID?): Boolean {
        if (uuid == null) return false
        return uuid.parseString() in ignored
    }

    fun switchIgnored(uuid: UUID): Boolean {
        return if (hasIgnored(uuid)) {
            removeIgnored(uuid)
            false
        } else {
            addIgnored(uuid)
            true
        }
    }

    companion object {

        @JvmField
        val data = ConcurrentHashMap<UUID, PlayerData>()
        val spying = mutableSetOf<UUID>()

        fun getData(player: OfflinePlayer): PlayerData {
            return data.computeIfAbsent(player.uniqueId) {
                PlayerData(player.uniqueId)
            }
        }

        fun removeData(player: OfflinePlayer) {
            data -= player.uniqueId
            player.uniqueId.releaseAutoDataContainer()
        }

    }
}