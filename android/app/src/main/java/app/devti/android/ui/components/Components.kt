package app.devti.android.ui.components

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.devti.android.ui.theme.Devti

/**
 * The Hub glyph: a faceted prism whose facets take the colors of connected devices.
 * Same geometry as desktop/src/design/Prism.tsx.
 */
@Composable
fun Prism(size: Dp, voices: List<Color>, modifier: Modifier = Modifier, breathe: Boolean = true, label: String? = null) {
    val c = Devti.colors
    val pulse by rememberInfiniteTransition(label = "prism").animateFloat(
        initialValue = 0.85f,
        targetValue = 1.15f,
        animationSpec = infiniteRepeatable(tween(2800), RepeatMode.Reverse),
        label = "core",
    )
    Canvas(
        modifier
            .size(size)
            .semantics { if (label != null) contentDescription = label },
    ) {
        val sx = this.size.width / 120f
        val sy = this.size.height / 112f
        fun p(x: Float, y: Float) = Offset(x * sx, y * sy)
        val a = p(60f, 10f)
        val b = p(10f, 98f)
        val cc = p(110f, 98f)
        val o = p(62f, 66f)
        val alpha = if (voices.isEmpty()) 0.18f else 0.55f
        val facets = listOf(listOf(a, b, o), listOf(b, cc, o), listOf(cc, a, o))
        facets.forEachIndexed { i, pts ->
            val color = voices.getOrNull(i % maxOf(voices.size, 1)) ?: c.textFaint
            val path = Path().apply {
                moveTo(pts[0].x, pts[0].y)
                lineTo(pts[1].x, pts[1].y)
                lineTo(pts[2].x, pts[2].y)
                close()
            }
            drawPath(path, Brush.linearGradient(listOf(color.copy(alpha = alpha), color.copy(alpha = alpha * 0.25f)), pts[0], pts[2]))
        }
        val outline = Path().apply {
            moveTo(a.x, a.y)
            lineTo(b.x, b.y)
            lineTo(cc.x, cc.y)
            close()
        }
        drawPath(outline, c.hub.copy(alpha = 0.9f), style = Stroke(width = 1.6f * sx, join = StrokeJoin.Round))
        listOf(a, b, cc).forEach { drawLine(c.hub.copy(alpha = 0.4f), o, it, strokeWidth = 1.2f * sx) }
        val coreR = 18f * sx * (if (breathe) pulse else 1f)
        drawCircle(Brush.radialGradient(listOf(Color.White.copy(alpha = 0.9f), Color.White.copy(alpha = 0f)), o, coreR), coreR, o)
        drawCircle(c.hub, 3.2f * sx, o)
    }
}

@Composable
fun PrimaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true) {
    val c = Devti.colors
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.height(52.dp),
        shape = RoundedCornerShape(12.dp),
        colors = ButtonDefaults.buttonColors(containerColor = c.hub, contentColor = c.bg),
        contentPadding = PaddingValues(horizontal = 24.dp),
    ) { Text(text, style = Devti.type.label) }
}

@Composable
fun QuietButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, danger: Boolean = false) {
    val c = Devti.colors
    OutlinedButton(
        onClick = onClick,
        modifier = modifier.height(52.dp),
        shape = RoundedCornerShape(12.dp),
        border = BorderStroke(1.dp, if (danger) c.danger else c.line),
        colors = ButtonDefaults.outlinedButtonColors(containerColor = c.surface2, contentColor = if (danger) c.danger else c.text),
    ) { Text(text, style = Devti.type.label) }
}

@Composable
fun GhostButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    TextButton(onClick = onClick, modifier = modifier) { Text(text, style = Devti.type.label, color = Devti.colors.textDim) }
}

/** Six digits in two groups, as on the Hub — for comparing verification codes. */
@Composable
fun CodeDigits(code: String, modifier: Modifier = Modifier) {
    val c = Devti.colors
    val digits = code.filter { it.isDigit() }
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        digits.forEachIndexed { i, d ->
            if (i == 3) Box(Modifier.width(12.dp))
            Box(
                Modifier
                    .size(width = 44.dp, height = 60.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(c.surface2)
                    .border(1.dp, c.line, RoundedCornerShape(10.dp)),
                contentAlignment = Alignment.Center,
            ) { Text(d.toString(), style = Devti.type.digits, color = c.text) }
        }
    }
}

/** Thin battery bar in a device's voice. */
@Composable
fun BatteryBar(percent: Int, color: Color, modifier: Modifier = Modifier) {
    val c = Devti.colors
    Box(
        modifier
            .width(40.dp)
            .height(6.dp)
            .clip(RoundedCornerShape(3.dp))
            .background(c.surface3),
    ) {
        Box(
            Modifier
                .fillMaxWidth(percent.coerceIn(0, 100) / 100f)
                .height(6.dp)
                .background(if (percent <= 15) c.danger else color),
        )
    }
}
