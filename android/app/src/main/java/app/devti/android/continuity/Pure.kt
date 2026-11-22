package app.devti.android.continuity

/**
 * Small decisions with no Android dependencies, so they run in plain JVM unit tests.
 * (Flag values are copied from android.app.Notification, which is not available there.)
 */
object Pure {
    private const val FLAG_FOREGROUND_SERVICE = 0x00000040
    private const val FLAG_GROUP_SUMMARY = 0x00000200

    /** Whether a phone notification is worth showing on a computer. */
    fun shouldMirror(pkg: String, ownPkg: String, ongoing: Boolean, flags: Int): Boolean {
        if (pkg == ownPkg || pkg == "android" || pkg == "com.android.systemui") return false
        if (ongoing) return false
        if (flags and FLAG_GROUP_SUMMARY != 0) return false
        if (flags and FLAG_FOREGROUND_SERVICE != 0) return false
        return true
    }

    /** A file name that is safe inside one folder: no separators, no control characters. */
    fun safeName(raw: String?): String {
        val cleaned = raw.orEmpty()
            .substringAfterLast('/')
            .substringAfterLast('\\')
            .filter { it >= ' ' && it !in "<>:\"|?*" }
            .trim()
            .trimStart('.')
            .take(180)
        return cleaned.ifBlank { "file" }
    }

    /** "photo.JPG" → "jpg"; no extension → null. */
    fun extension(name: String): String? =
        name.substringAfterLast('.', "").lowercase().takeIf { it.isNotEmpty() && it.length <= 10 && it != name.lowercase() }

    /** "1.4 MB", "820 KB", "12 B". */
    fun bytes(n: Long): String = when {
        n >= 1L shl 30 -> String.format(java.util.Locale.ROOT, "%.1f GB", n / (1L shl 30).toDouble())
        n >= 1L shl 20 -> String.format(java.util.Locale.ROOT, "%.1f MB", n / (1L shl 20).toDouble())
        n >= 1L shl 10 -> "${n shr 10} KB"
        else -> "$n B"
    }

    /** One line about a transfer, as the phone shows it. */
    fun transferLine(state: String, done: Long, size: Long, rateBps: Long): String = when (state) {
        "done" -> bytes(size)
        "failed" -> "Didn't finish"
        "cancelled" -> "Cancelled"
        "paused" -> "Paused at ${bytes(done)} of ${bytes(size)} — resumes when connected"
        "queued" -> "Waiting to start"
        else -> buildString {
            append("${bytes(done)} of ${bytes(size)}")
            if (rateBps > 0) {
                append(" · ${bytes(rateBps)}/s")
                val left = (size - done).coerceAtLeast(0) / rateBps
                append(" · ").append(if (left < 60) "${left.coerceAtLeast(1)} s left" else "${left / 60} min left")
            }
        }
    }
}
