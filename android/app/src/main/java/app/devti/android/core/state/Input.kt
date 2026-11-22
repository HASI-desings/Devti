package app.devti.android.core.state

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * Builds the core's `InputEvent` / `SystemAction` JSON (see core/src/protocol/messages.rs).
 * Pure Kotlin, so the JVM tests send exactly what the app sends.
 */
object Input {
    fun move(dx: Float, dy: Float) = buildJsonObject { put("k", "move"); put("dx", dx); put("dy", dy) }
    fun click(button: String = "left", count: Int = 1) = buildJsonObject { put("k", "click"); put("button", button); put("count", count) }
    fun button(button: String, down: Boolean) = buildJsonObject { put("k", "button"); put("button", button); put("down", down) }
    fun scroll(dx: Float, dy: Float) = buildJsonObject { put("k", "scroll"); put("dx", dx); put("dy", dy) }
    fun text(text: String) = buildJsonObject { put("k", "text"); put("text", text) }
    fun key(key: String, vararg mods: String) = buildJsonObject {
        put("k", "key")
        put("key", key)
        putJsonArray("mods") { mods.forEach { add(kotlinx.serialization.json.JsonPrimitive(it)) } }
    }
    fun media(action: String) = buildJsonObject { put("k", "media"); put("action", action) }

    fun encode(events: List<JsonObject>): String = JsonArray(events).toString()

    fun system(action: String, percent: Int? = null): String = buildJsonObject {
        put("a", action)
        if (percent != null) put("percent", percent.coerceIn(0, 100))
    }.toString()

    /** Touchpad acceleration: slow moves stay precise, fast flicks cross the screen. */
    fun accelerate(d: Float, speedPxPerMs: Float): Float {
        val gain = when {
            speedPxPerMs < 0.3f -> 1.0f
            speedPxPerMs < 1.2f -> 1.6f
            else -> 2.6f
        }
        return d * gain
    }
}
