package app.devti.android.continuity

import app.devti.android.core.state.CallInfo

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

    // ------------------------------------------------------------------ Phase 6

    private val URL = Regex("""https?://[^\s<>"]+""")

    /**
     * The link in text shared to Devti, when the share is "a link" (just the URL, or a
     * short title around one URL, as browsers and video apps share). Null otherwise.
     */
    fun sharedLink(text: String): String? {
        val urls = URL.findAll(text).map { it.value.trimEnd('.', ',', ')', '!', '?') }.toList()
        if (urls.size != 1) return null
        val rest = text.replace(URL, "").trim()
        return urls[0].takeIf { rest.length <= 120 }
    }

    /** YouTube links resume at the second they were at; others open as they are. */
    fun mediaUrl(url: String, positionMs: Long?): String {
        val secs = (positionMs ?: 0) / 1000
        if (secs <= 0 || !(url.contains("youtube.com/") || url.contains("youtu.be/"))) return url
        val base = url.substringBefore('#').split('&').filterNot { it.startsWith("t=") }.joinToString("&")
        val clean = if (base.contains("?t=")) base.replace(Regex("""\?t=[^&]*&?"""), "?").trimEnd('?') else base
        return clean + (if ('?' in clean) "&" else "?") + "t=${secs}s"
    }

    /**
     * Folds Android's call states ("RINGING", "OFFHOOK", "IDLE") into the call computers see.
     * Android 9+ sends each change twice (without, then with the number), so a repeat
     * keeps the same call.
     */
    fun nextCall(previous: CallInfo?, state: String, number: String?, name: String, now: Long): CallInfo? = when (state) {
        "RINGING" -> if (previous?.state == "ringing") {
            previous.copy(number = number ?: previous.number, name = name.ifEmpty { previous.name })
        } else {
            CallInfo(id = "call-$now", state = "ringing", number = number.orEmpty(), name = name, incoming = true, ts = now)
        }
        "OFFHOOK" -> previous?.copy(state = "active", number = number ?: previous.number, name = name.ifEmpty { previous.name })
            ?: CallInfo(id = "call-$now", state = "active", number = number.orEmpty(), name = name, incoming = false, ts = now)
        "IDLE" -> previous?.let { it.copy(state = if (it.state == "ringing") "missed" else "ended") }
        else -> null
    }

    /**
     * Rotates an NV21 frame (full-size luma, then interleaved V,U at half size) clockwise
     * by 90, 180 or 270 degrees. The result is h×w for 90/270 and w×h for 180.
     */
    fun rotateNv21(data: ByteArray, w: Int, h: Int, degrees: Int): ByteArray {
        val d = ((degrees % 360) + 360) % 360
        if (d == 0) return data
        val nw = if (d == 180) w else h
        val out = ByteArray(data.size)
        for (y in 0 until h) {
            for (x in 0 until w) {
                val (nx, ny) = when (d) {
                    90 -> (h - 1 - y) to x
                    180 -> (w - 1 - x) to (h - 1 - y)
                    else -> y to (w - 1 - x)
                }
                out[ny * nw + nx] = data[y * w + x]
            }
        }
        val (cw, ch) = w / 2 to h / 2
        val ncw = nw / 2
        val base = w * h
        for (y in 0 until ch) {
            for (x in 0 until cw) {
                val (nx, ny) = when (d) {
                    90 -> (ch - 1 - y) to x
                    180 -> (cw - 1 - x) to (ch - 1 - y)
                    else -> y to (cw - 1 - x)
                }
                val src = base + (y * cw + x) * 2
                val dst = base + (ny * ncw + nx) * 2
                out[dst] = data[src]
                out[dst + 1] = data[src + 1]
            }
        }
        return out
    }
}
