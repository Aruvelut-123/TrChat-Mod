package me.arasple.mc.trchat.util.color

/**
 * @author XyLuoDYS
 * @since 2026/8/18 19:50
 */
private const val COLOR_CHAR = '§'

fun String.wrapSpecialChars(prefix: String, suffix: String, specialCharsSet: Set<String>): String {
    if (prefix.isBlank() || specialCharsSet.isEmpty()) return this

    val sb = StringBuilder(length)
    var inSpecialChars = false
    var manualColor = false
    var hasManualColor = false
    var i = 0
    while (i < length) {
        val cp = codePointAt(i)
        val ch = String(Character.toChars(cp))
        val isSpecialChars = specialCharsSet.contains(ch)
        val isExt = cp == 0x200D || cp in 0x1F3FB..0x1F3FF || cp == 0xFE0F

        if (!isSpecialChars && !isExt && cp == COLOR_CHAR.code) {
            val next = if (i + 1 < length) this[i + 1] else ' '
            val isDefaultPrefix = (i == 0 && substring(i).startsWith(suffix))
            if (!isDefaultPrefix) {
                hasManualColor = !(next == 'r' || next == 'R')
            }
            sb.append(ch)
            if (i + 1 < length) sb.append(this[i + 1])
            i += if (i + 1 < length) 2 else 1
            continue
        }

        when {
            isSpecialChars -> {
                if (!inSpecialChars) {
                    manualColor = hasManualColor
                    inSpecialChars = true
                    if (!manualColor) sb.append(prefix)
                }
                sb.append(ch)
            }
            isExt && inSpecialChars -> sb.append(ch)
            inSpecialChars -> {
                if (!manualColor) sb.append(suffix)
                inSpecialChars = false
                sb.append(ch)
            }
            else -> sb.append(ch)
        }
        i += Character.charCount(cp)
    }
    if (inSpecialChars && !manualColor) sb.append(suffix)
    return sb.toString()
}

fun String.hasSpecialChars(specialCharsSet: Set<String>): Boolean {
    if (specialCharsSet.isEmpty()) return false
    var i = 0
    while (i < length) {
        val cp = codePointAt(i)
        if (specialCharsSet.contains(String(Character.toChars(cp)))) return true
        i += Character.charCount(cp)
    }
    return false
}