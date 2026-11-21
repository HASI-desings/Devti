package app.devti.android.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import app.devti.android.R

// Devti "Prism" tokens — the same values as desktop/src/styles/tokens.css (docs/DESIGN.md).

@Immutable
data class DevtiColors(
    val bg: Color,
    val bgDeep: Color,
    val surface: Color,
    val surface2: Color,
    val surface3: Color,
    val line: Color,
    val text: Color,
    val textDim: Color,
    val textFaint: Color,
    val hub: Color,
    val danger: Color,
    val ok: Color,
    val voices: Map<String, Color>,
)

val Night = DevtiColors(
    bg = Color(0xFF15122A),
    bgDeep = Color(0xFF0F0C21),
    surface = Color(0xFF1D1937),
    surface2 = Color(0xFF262145),
    surface3 = Color(0xFF312B55),
    line = Color(0x1AD6D0FF),
    text = Color(0xFFF2F0FB),
    textDim = Color(0xFFAAA4CA),
    textFaint = Color(0xFF736D95),
    hub = Color.White,
    danger = Color(0xFFFF6680),
    ok = Color(0xFF63E4B3),
    voices = mapOf(
        "iris" to Color(0xFFA596FF), "aqua" to Color(0xFF5AD6F3), "coral" to Color(0xFFFF8F7E),
        "mint" to Color(0xFF63E4B3), "marigold" to Color(0xFFFFCB62), "rose" to Color(0xFFFF82C4),
    ),
)

val Day = DevtiColors(
    bg = Color(0xFFEEEDF6),
    bgDeep = Color(0xFFE5E3F1),
    surface = Color(0xFFFBFBFE),
    surface2 = Color(0xFFF2F1F9),
    surface3 = Color(0xFFE8E6F4),
    line = Color(0x1A1E1846),
    text = Color(0xFF1A1733),
    textDim = Color(0xFF56517A),
    textFaint = Color(0xFF8B87A7),
    hub = Color(0xFF1A1733),
    danger = Color(0xFFCF1F48),
    ok = Color(0xFF0F8158),
    voices = mapOf(
        "iris" to Color(0xFF5643DC), "aqua" to Color(0xFF0B7496), "coral" to Color(0xFFC8432F),
        "mint" to Color(0xFF0F8158), "marigold" to Color(0xFF9C6A00), "rose" to Color(0xFFB52A76),
    ),
)

val VOICES = listOf("iris", "aqua", "coral", "mint", "marigold", "rose")

/** Same FNV-1a hash as the desktop, so a device keeps its color everywhere it can. */
fun voiceFor(id: String): String {
    var h = 0x811c9dc5.toInt()
    for (c in id) {
        h = h xor c.code
        h *= 0x01000193
    }
    return VOICES[((h.toLong() and 0xffffffffL) % VOICES.size).toInt()]
}

@OptIn(ExperimentalTextApi::class)
private fun display(weight: Int, width: Float) = Font(
    R.font.bricolage_grotesque,
    weight = FontWeight(weight),
    variationSettings = FontVariation.Settings(FontVariation.weight(weight), FontVariation.width(width)),
)

@OptIn(ExperimentalTextApi::class)
private fun ui(weight: Int) = Font(
    R.font.instrument_sans,
    weight = FontWeight(weight),
    variationSettings = FontVariation.Settings(FontVariation.weight(weight)),
)

val DisplayFamily = FontFamily(display(640, 80f), display(700, 76f))
val UiFamily = FontFamily(ui(400), ui(560), ui(620))

@Immutable
data class DevtiType(
    val hero: TextStyle = TextStyle(fontFamily = DisplayFamily, fontWeight = FontWeight(700), fontSize = 44.sp, lineHeight = 44.sp, letterSpacing = (-0.6).sp),
    val title: TextStyle = TextStyle(fontFamily = DisplayFamily, fontWeight = FontWeight(640), fontSize = 31.sp, lineHeight = 34.sp, letterSpacing = (-0.3).sp),
    val heading: TextStyle = TextStyle(fontFamily = DisplayFamily, fontWeight = FontWeight(640), fontSize = 22.sp, lineHeight = 26.sp),
    val body: TextStyle = TextStyle(fontFamily = UiFamily, fontWeight = FontWeight(400), fontSize = 16.sp, lineHeight = 24.sp),
    val label: TextStyle = TextStyle(fontFamily = UiFamily, fontWeight = FontWeight(560), fontSize = 15.sp, lineHeight = 20.sp),
    val small: TextStyle = TextStyle(fontFamily = UiFamily, fontWeight = FontWeight(400), fontSize = 13.sp, lineHeight = 18.sp),
    val digits: TextStyle = TextStyle(fontFamily = DisplayFamily, fontWeight = FontWeight(640), fontSize = 40.sp),
)

val LocalDevtiColors = staticCompositionLocalOf { Night }
val LocalDevtiType = staticCompositionLocalOf { DevtiType() }

object Devti {
    val colors: DevtiColors @Composable get() = LocalDevtiColors.current
    val type: DevtiType @Composable get() = LocalDevtiType.current
}

@Composable
fun DevtiTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val c = if (dark) Night else Day
    val scheme = if (dark) {
        darkColorScheme(primary = c.hub, onPrimary = c.bg, background = c.bg, surface = c.surface, onSurface = c.text, onBackground = c.text, error = c.danger)
    } else {
        lightColorScheme(primary = c.hub, onPrimary = c.bg, background = c.bg, surface = c.surface, onSurface = c.text, onBackground = c.text, error = c.danger)
    }
    CompositionLocalProvider(LocalDevtiColors provides c, LocalDevtiType provides DevtiType()) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
