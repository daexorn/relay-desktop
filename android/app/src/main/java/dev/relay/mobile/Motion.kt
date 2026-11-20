package dev.relay.mobile

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext

/**
 * Continuous motion (spinners, live waveforms) is off in Robolectric, where an endless animation
 * would keep the test clock busy, and when the phone has animations switched off.
 */
@Composable
fun motionEnabled(): Boolean {
    if (android.os.Build.FINGERPRINT == "robolectric") return false
    val context = LocalContext.current
    return remember(context) {
        runCatching {
            android.provider.Settings.Global.getFloat(
                context.contentResolver,
                android.provider.Settings.Global.ANIMATOR_DURATION_SCALE,
                1f,
            ) > 0f
        }.getOrDefault(true)
    }
}

/** 0..1 repeating phase while [active]; a fixed value otherwise. */
@Composable
fun loopPhase(active: Boolean, periodMs: Int, idle: Float = 0.3f, label: String = "loop"): Float {
    if (!active || !motionEnabled()) return idle
    val transition = rememberInfiniteTransition(label = label)
    val value by transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(periodMs, easing = LinearEasing), RepeatMode.Restart),
        label = label,
    )
    return value
}

/** Smoothly breathes between 0 and 1 while [active]. */
@Composable
fun breathing(active: Boolean, periodMs: Int = 1400, label: String = "breath"): Float {
    if (!active || !motionEnabled()) return 0.5f
    val transition = rememberInfiniteTransition(label = label)
    val value by transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(periodMs / 2), RepeatMode.Reverse),
        label = label,
    )
    return value
}

/** Buttons sink a little under the finger and spring back. Works next to `clickable`. */
fun Modifier.pressBounce(scaleDown: Float = 0.9f): Modifier = composed {
    var pressed by remember { mutableStateOf(false) }
    val scale by animateFloatAsState(
        if (pressed) scaleDown else 1f,
        spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium),
        label = "press",
    )
    this
        .graphicsLayer {
            scaleX = scale
            scaleY = scale
        }
        .pointerInput(Unit) {
            awaitEachGesture {
                awaitFirstDown(requireUnconsumed = false)
                pressed = true
                waitForUpOrCancellation()
                pressed = false
            }
        }
}

// --- Agent marks drawn as vectors (Claude spark, OpenAI blossom for Codex, Antigravity arch) ---

private val CLAUDE_RAYS = floatArrayOf(1.0f, 0.80f, 0.94f, 0.78f, 0.98f, 0.84f, 0.90f, 0.76f, 0.97f, 0.82f, 0.92f, 0.79f)

fun androidx.compose.ui.graphics.drawscope.DrawScope.drawClaudeMark() {
    val color = androidx.compose.ui.graphics.Color(0xFFD97757)
    val c = center
    val r = size.minDimension / 2f
    CLAUDE_RAYS.forEachIndexed { i, l ->
        val a = (-Math.PI / 2 + i * 2 * Math.PI / CLAUDE_RAYS.size + if (i % 2 == 1) 0.05 else -0.03).toFloat()
        val ux = kotlin.math.cos(a); val uy = kotlin.math.sin(a)
        val nx = -uy; val ny = ux
        val base = r * 0.135f
        val tip = (r * 0.06f).coerceAtLeast(0.5f)
        val ex = c.x + ux * (r * l - tip); val ey = c.y + uy * (r * l - tip)
        val path = androidx.compose.ui.graphics.Path().apply {
            moveTo(c.x + nx * base, c.y + ny * base)
            lineTo(ex + nx * tip, ey + ny * tip)
            lineTo(ex - nx * tip, ey - ny * tip)
            lineTo(c.x - nx * base, c.y - ny * base)
            close()
        }
        drawPath(path, color)
        drawCircle(color, tip, androidx.compose.ui.geometry.Offset(ex, ey))
    }
    drawCircle(color, r * 0.2f, c)
}

fun androidx.compose.ui.graphics.drawscope.DrawScope.drawOpenAiMark(color: androidx.compose.ui.graphics.Color) {
    val c = center
    val r = size.minDimension / 2f
    val length = r * 0.95f; val radius = r * 0.24f; val offset = r * 0.36f
    val stroke = androidx.compose.ui.graphics.drawscope.Stroke(width = (r * 0.11f).coerceAtLeast(1.1f))
    for (k in 0 until 6) {
        val a = Math.toRadians(k * 60.0).toFloat()
        val ux = kotlin.math.cos(a); val uy = kotlin.math.sin(a)
        val nx = -uy; val ny = ux
        val cx = c.x + nx * offset; val cy = c.y + ny * offset
        val half = length / 2f - radius
        val path = androidx.compose.ui.graphics.Path()
        var first = true
        for ((end, from) in listOf(half to -90f, -half to 90f)) {
            for (step in 0..18) {
                val t = Math.toRadians((from + step * 10f).toDouble()).toFloat()
                val x = cx + ux * end + (ux * kotlin.math.cos(t) + nx * kotlin.math.sin(t)) * radius
                val y = cy + uy * end + (uy * kotlin.math.cos(t) + ny * kotlin.math.sin(t)) * radius
                if (first) { path.moveTo(x, y); first = false } else path.lineTo(x, y)
            }
        }
        path.close()
        drawPath(path, color, style = stroke)
    }
}

fun androidx.compose.ui.graphics.drawscope.DrawScope.drawAntigravityMark() {
    val stops = listOf(0xFF3186FF, 0xFF9062FF, 0xFFFF4641, 0xFFFFA31A, 0xFFFFD314, 0xFF00B95C)
        .map { androidx.compose.ui.graphics.Color(it) }
    val c = center
    val r = size.minDimension / 2f
    val span = r * 0.58f; val top = c.y - r * 0.12f; val bottom = c.y + r * 0.78f
    val points = mutableListOf<androidx.compose.ui.geometry.Offset>()
    for (i in 0 until 8) points += androidx.compose.ui.geometry.Offset(c.x - span, bottom - (bottom - top) * i / 7f)
    for (k in 1 until 24) {
        val t = (Math.PI * (1.0 + k / 24.0)).toFloat()
        points += androidx.compose.ui.geometry.Offset(c.x + span * kotlin.math.cos(t), top + span * kotlin.math.sin(t))
    }
    for (i in 0 until 8) points += androidx.compose.ui.geometry.Offset(c.x + span, top + (bottom - top) * i / 7f)
    val width = (r * 0.36f).coerceAtLeast(1.5f)
    val last = points.size - 1
    for (i in 0 until last) {
        val f = i.toFloat() / (last - 1).coerceAtLeast(1) * (stops.size - 1)
        val j = f.toInt().coerceAtMost(stops.size - 2)
        val color = androidx.compose.ui.graphics.lerp(stops[j], stops[j + 1], f - j)
        drawLine(color, points[i], points[i + 1], strokeWidth = width, cap = androidx.compose.ui.graphics.StrokeCap.Round)
    }
}
