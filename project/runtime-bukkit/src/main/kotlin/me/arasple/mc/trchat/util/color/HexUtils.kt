package me.arasple.mc.trchat.util.color

import me.arasple.mc.trchat.util.isDragonCoreHooked
import net.md_5.bungee.api.ChatColor
import java.awt.Color
import java.util.regex.Matcher
import java.util.regex.Pattern
import java.util.stream.Collectors
import kotlin.math.abs
import kotlin.math.asin
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.round
import kotlin.math.sin

/**
 * @author Esophose
 * <a href="https://github.com/Rosewood-Development/RoseGarden/blob/master/src/main/java/dev/rosewood/rosegarden/utils/HexUtils.java">...</a>
 */
object HexUtils {

    private const val CHARS_UNTIL_LOOP = 30

    @JvmField
    val RAINBOW_PATTERN: Pattern = Pattern.compile("<(?<type>rainbow|r)(#(?<speed>\\d+))?(:(?<saturation>\\d*\\.?\\d+))?(:(?<brightness>\\d*\\.?\\d+))?(:(?<loop>l|L|loop))?>")

    @JvmField
    val GRADIENT_PATTERN: Pattern = Pattern.compile("<(?<type>gradient|g)(#(?<speed>\\d+))?(?<hex>(:#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})){2,})(:(?<loop>l|L|loop))?>")

    @JvmField
    val HEX_PATTERNS: List<Pattern> = listOf(
        Pattern.compile("&\\{#([A-Fa-f0-9]){6}}"),
        Pattern.compile("&#([A-Fa-f0-9]){6}")
    )

    private val STOP: Pattern = Pattern.compile(
        "<(rainbow|r)(#(\\d+))?(:(\\d*\\.?\\d+))?(:(\\d*\\.?\\d+))?(:(l|L|loop))?>|" +
            "<(gradient|g)(#(\\d+))?((:#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})){2,})(:(l|L|loop))?>|" +
            "(&[a-f0-9r])|" +
            "<#([A-Fa-f0-9]){6}>|" +
            "\\{#([A-Fa-f0-9]){6}}|" +
            "&#([A-Fa-f0-9]){6}|" +
            "#([A-Fa-f0-9]){6}|" +
            '§'
    )

    private fun getCaptureGroup(matcher: Matcher, group: String): String? {
        return try {
            matcher.group(group)
        } catch (_: IllegalStateException) {
            null
        } catch (_: IllegalArgumentException) {
            null
        }
    }

    @JvmStatic
    fun colorify(message: String): String {
        var parsed = message
        parsed = parseRainbow(parsed)
        parsed = parseGradients(parsed)
        parsed = parseHex(parsed)
        parsed = parseLegacy(parsed)
        return parsed
    }

    @JvmStatic
    fun parseRainbow(message: String): String {
        var parsed = message
        var matcher = RAINBOW_PATTERN.matcher(parsed)
        while (matcher.find()) {
            val parsedRainbow = StringBuilder()

            var speed = -1
            var saturation = 1.0F
            var brightness = 1.0F
            val looping = getCaptureGroup(matcher, "looping") != null

            val speedGroup = getCaptureGroup(matcher, "speed")
            if (speedGroup != null) {
                try {
                    speed = speedGroup.toInt()
                } catch (_: NumberFormatException) {
                }
            }

            val saturationGroup = getCaptureGroup(matcher, "saturation")
            if (saturationGroup != null) {
                try {
                    saturation = saturationGroup.toFloat()
                } catch (_: NumberFormatException) {
                }
            }

            val brightnessGroup = getCaptureGroup(matcher, "brightness")
            if (brightnessGroup != null) {
                try {
                    brightness = brightnessGroup.toFloat()
                } catch (_: NumberFormatException) {
                }
            }

            val stop = findStop(parsed, matcher.end())
            val content = parsed.substring(matcher.end(), stop)

            var length = content.codePointCount(0, content.length)
            if (looping) {
                length = min(length, CHARS_UNTIL_LOOP)
            }

            val rainbow: ColorGenerator = if (speed == -1) {
                Rainbow(length, saturation, brightness)
            } else {
                AnimatedRainbow(length, saturation, brightness, speed)
            }

            var compoundedFormat = ""
            var i = 0
            while (i < content.length) {
                val codePoint = content.codePointAt(i)
                val charCount = Character.charCount(codePoint)

                if (codePoint == '&'.code && i + 1 < content.length) {
                    val next = content[i + 1]
                    if (isFormat(next)) {
                        compoundedFormat += "${ChatColor.COLOR_CHAR}$next"
                        i += 2
                        continue
                    }
                }

                parsedRainbow.append(rainbow.nextChatColor()).append(compoundedFormat).appendCodePoint(codePoint)
                i += charCount
            }

            val before = parsed.substring(0, matcher.start())
            val after = parsed.substring(stop)
            parsed = before + parsedRainbow + after
            matcher = RAINBOW_PATTERN.matcher(parsed)
        }
        return parsed
    }

    @JvmStatic
    fun parseGradients(message: String): String {
        var parsed = message
        var matcher = GRADIENT_PATTERN.matcher(parsed)
        while (matcher.find()) {
            val parsedGradient = StringBuilder()

            var speed = -1
            val looping = getCaptureGroup(matcher, "loop") != null

            val hexSteps = getCaptureGroup(matcher, "hex")!!
                .substring(1)
                .split(":")
                .stream()
                .map { if (it.length != 4) it else "#${it[1]}${it[1]}${it[2]}${it[2]}${it[3]}${it[3]}" }
                .map(Color::decode)
                .collect(Collectors.toList())

            val speedGroup = getCaptureGroup(matcher, "speed")
            if (speedGroup != null) {
                try {
                    speed = speedGroup.toInt()
                } catch (_: NumberFormatException) {
                }
            }

            val stop = findStop(parsed, matcher.end())
            val content = parsed.substring(matcher.end(), stop)

            var length = content.codePointCount(0, content.length)
            if (looping) {
                length = min(length, CHARS_UNTIL_LOOP)
            }

            val gradient: ColorGenerator = if (speed == -1) {
                Gradient(hexSteps, length)
            } else {
                AnimatedGradient(hexSteps, length, speed)
            }

            var compoundedFormat = ""
            var i = 0
            while (i < content.length) {
                val codePoint = content.codePointAt(i)
                val charCount = Character.charCount(codePoint)

                if (codePoint == '&'.code && i + 1 < content.length) {
                    val next = content[i + 1]
                    if (isFormat(next)) {
                        compoundedFormat += "${ChatColor.COLOR_CHAR}$next"
                        i += 2
                        continue
                    }
                }

                parsedGradient.append(gradient.nextChatColor()).append(compoundedFormat).appendCodePoint(codePoint)
                i += charCount
            }

            val before = parsed.substring(0, matcher.start())
            val after = parsed.substring(stop)
            parsed = before + parsedGradient + after
            matcher = GRADIENT_PATTERN.matcher(parsed)
        }
        return parsed
    }

    @JvmStatic
    fun parseHex(message: String): String {
        var parsed = message
        for (pattern in HEX_PATTERNS) {
            var matcher = pattern.matcher(parsed)
            while (matcher.find()) {
                val color = if (isDragonCoreHooked) {
                    "§" + cleanHex(matcher.group())
                } else {
                    translateHex(cleanHex(matcher.group())).toString()
                }
                val before = parsed.substring(0, matcher.start())
                val after = parsed.substring(matcher.end())
                parsed = before + color + after
                matcher = pattern.matcher(parsed)
            }
        }
        return parsed
    }

    @JvmStatic
    fun parseLegacy(message: String): String {
        return ChatColor.translateAlternateColorCodes('&', message.replace("&r", "&f").replace("§r", "§f"))
    }

    private fun findStop(content: String, searchAfter: Int): Int {
        val matcher = STOP.matcher(content)
        while (matcher.find()) {
            if (matcher.start() > searchAfter) {
                return matcher.start()
            }
        }
        return content.length
    }

    private fun cleanHex(hex: String): String {
        return when {
            hex.startsWith("&{") -> hex.substring(2, hex.length - 1)
            hex.startsWith("&#") -> hex.substring(1)
            else -> hex
        }
    }

    @JvmStatic
    fun translateHex(hex: String): ChatColor {
        if (isHigherOrEqual11600()) {
            return ChatColor.of(hex)
        }
        return translateHex(Color.decode(hex))
    }

    @JvmStatic
    fun translateHex(color: Color): ChatColor {
        if (isHigherOrEqual11600()) {
            return ChatColor.of(color)
        }

        var minDist = Int.MAX_VALUE
        var legacy = ChatColor.WHITE
        for (mapping in ChatColorHexMapping.entries) {
            val r = mapping.red - color.red
            val g = mapping.green - color.green
            val b = mapping.blue - color.blue
            val dist = r * r + g * g + b * b
            if (dist < minDist) {
                minDist = dist
                legacy = mapping.chatColor
            }
        }
        return legacy
    }

    enum class ChatColorHexMapping(val hex: Int, val chatColor: ChatColor) {
        BLACK(0x000000, ChatColor.BLACK),
        DARK_BLUE(0x0000AA, ChatColor.DARK_BLUE),
        DARK_GREEN(0x00AA00, ChatColor.DARK_GREEN),
        DARK_AQUA(0x00AAAA, ChatColor.DARK_AQUA),
        DARK_RED(0xAA0000, ChatColor.DARK_RED),
        DARK_PURPLE(0xAA00AA, ChatColor.DARK_PURPLE),
        GOLD(0xFFAA00, ChatColor.GOLD),
        GRAY(0xAAAAAA, ChatColor.GRAY),
        DARK_GRAY(0x555555, ChatColor.DARK_GRAY),
        BLUE(0x5555FF, ChatColor.BLUE),
        GREEN(0x55FF55, ChatColor.GREEN),
        AQUA(0x55FFFF, ChatColor.AQUA),
        RED(0xFF5555, ChatColor.RED),
        LIGHT_PURPLE(0xFF55FF, ChatColor.LIGHT_PURPLE),
        YELLOW(0xFFFF55, ChatColor.YELLOW),
        WHITE(0xFFFFFF, ChatColor.WHITE);

        val red: Int = (hex shr 16) and 0xFF
        val green: Int = (hex shr 8) and 0xFF
        val blue: Int = hex and 0xFF
    }

    interface ColorGenerator {
        fun nextChatColor(): ChatColor

        fun nextColor(): Color
    }

    open class Gradient(colors: List<Color>, private val steps: Int) : ColorGenerator {

        private val gradients = mutableListOf<TwoStopGradient>()
        protected var step = 0L

        init {
            require(colors.size >= 2) { "Must provide at least 2 colors" }
            val increment = (steps - 1).toFloat() / (colors.size - 1)
            for (i in 0 until colors.size - 1) {
                gradients += TwoStopGradient(colors[i], colors[i + 1], increment * i, increment * (i + 1))
            }
        }

        override fun nextChatColor(): ChatColor {
            if (!isHigherOrEqual11600() || steps <= 1) {
                return translateHex(gradients[0].colorAt(0))
            }
            return translateHex(nextColor())
        }

        override fun nextColor(): Color {
            val adjustedStep = round(abs(((2 * asin(sin(step * (Math.PI / (2 * steps))))) / Math.PI) * steps)).toInt()
            val color = if (gradients.size < 2) {
                gradients[0].colorAt(adjustedStep)
            } else {
                val segment = steps.toFloat() / gradients.size
                val index = min(floor(adjustedStep / segment).toInt(), gradients.size - 1)
                gradients[index].colorAt(adjustedStep)
            }
            step++
            return color
        }

        class TwoStopGradient(
            private val startColor: Color,
            private val endColor: Color,
            private val lowerRange: Float,
            private val upperRange: Float
        ) {

            fun colorAt(step: Int): Color {
                return Color(
                    calculateHexPiece(step, startColor.red, endColor.red),
                    calculateHexPiece(step, startColor.green, endColor.green),
                    calculateHexPiece(step, startColor.blue, endColor.blue)
                )
            }

            private fun calculateHexPiece(step: Int, channelStart: Int, channelEnd: Int): Int {
                val range = upperRange - lowerRange
                if (range == 0f) {
                    return channelStart
                }
                val interval = (channelEnd - channelStart) / range
                val value = round(interval * (step - lowerRange) + channelStart).toInt()
                return min(max(value, 0), 255)
            }
        }
    }

    class AnimatedGradient(colors: List<Color>, steps: Int, speed: Int) : Gradient(colors, steps) {
        init {
            step = System.currentTimeMillis() / speed
        }
    }

    open class Rainbow(totalColors: Int, saturation: Float, brightness: Float) : ColorGenerator {

        protected val hueStep: Float
        protected val saturation: Float
        protected val brightness: Float
        protected var hue: Float

        init {
            val fixedTotalColors = if (totalColors < 1) 1 else totalColors
            hueStep = 1.0F / fixedTotalColors
            this.saturation = max(0f, min(1f, saturation))
            this.brightness = max(0f, min(1f, brightness))
            hue = 0f
        }

        override fun nextChatColor(): ChatColor {
            return translateHex(nextColor())
        }

        override fun nextColor(): Color {
            val color = Color.getHSBColor(hue, saturation, brightness)
            hue += hueStep
            return color
        }
    }

    class AnimatedRainbow(totalColors: Int, saturation: Float, brightness: Float, speed: Int) : Rainbow(totalColors, saturation, brightness) {
        init {
            hue = (((floor(System.currentTimeMillis() / 50.0) / 360) * speed) % 1).toFloat()
        }
    }
}
