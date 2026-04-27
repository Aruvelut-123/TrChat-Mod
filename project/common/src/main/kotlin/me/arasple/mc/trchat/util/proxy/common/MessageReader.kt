package me.arasple.mc.trchat.util.proxy.common

import com.google.common.cache.CacheBuilder
import com.google.gson.JsonParser
import me.arasple.mc.trchat.util.toUUID
import java.nio.charset.StandardCharsets
import java.util.concurrent.TimeUnit

/**
 * 通讯信息数据包读取工具
 *
 * @author 坏黑
 * @since 2020-10-15
 */
object MessageReader {

    private val queueMessages = CacheBuilder.newBuilder()
        .expireAfterWrite(10, TimeUnit.SECONDS)
        .build<String, Message>()

    /**
     * 将通讯数据读取为数据包
     *
     * @param packet 通讯数据（未经过处理的原始内容）
     */
    @JvmStatic
    fun read(packet: ByteArray): Message {
        return read(String(packet, StandardCharsets.UTF_8))
    }

    /**
     * 通过通讯数据读取为数据包
     *
     * @param packet 通讯数据（未经过处理的原始内容）
     */
    @JvmStatic
    fun read(packet: String): Message {
        val json = JsonParser().parse(packet).asJsonObject
        val uid = json.get("uid").asString
        val message = queueMessages.getIfPresent(uid) ?: Message().also {
            queueMessages.put(uid, it)
        }
        message.messages += MessagePacket(
            uid.toUUID(),
            json.get("data").asString,
            json.get("index").asInt,
            json.get("total").asInt
        )
        return message
    }
}
