package dev.relay.mobile

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import kotlin.math.cos
import kotlin.math.sin

val LocalPalette = staticCompositionLocalOf { palette("graphite") }

@Composable
fun RelayApp(vm: RelayViewModel) {
    val target = palette(vm.theme)
    val p = target.copy(
        base = animateColorAsState(target.base, tween(280), label = "base").value,
        side = animateColorAsState(target.side, tween(280), label = "side").value,
        rail = animateColorAsState(target.rail, tween(280), label = "rail").value,
        surface = animateColorAsState(target.surface, tween(280), label = "surface").value,
        line = animateColorAsState(target.line, tween(280), label = "line").value,
        text = animateColorAsState(target.text, tween(280), label = "text").value,
        muted = animateColorAsState(target.muted, tween(280), label = "muted").value,
        accent = animateColorAsState(target.accent, tween(280), label = "accent").value,
    )
    CompositionLocalProvider(LocalPalette provides p) {
        Box(Modifier.fillMaxSize().background(p.base).statusBarsPadding().navigationBarsPadding().imePadding()) {
            AnimatedContent(
                targetState = vm.screen,
                transitionSpec = {
                    val forward = targetState.ordinal > initialState.ordinal
                    (slideInHorizontally(tween(240)) { if (forward) it / 4 else -it / 4 } + fadeIn(tween(240))) togetherWith
                        (slideOutHorizontally(tween(200)) { if (forward) -it / 6 else it / 6 } + fadeOut(tween(160)))
                },
                label = "screen",
            ) { screen ->
                when (screen) {
                    Screen.Servers -> ServersScreen(vm)
                    Screen.Sessions -> SessionsScreen(vm)
                    Screen.Chat -> ChatScreen(vm)
                    Screen.Settings -> SettingsScreen(vm)
                }
            }
            vm.trust?.let { TrustDialog(it, vm) }
            vm.authFlow?.let { AuthSheet(vm, it) }
        }
    }
}

// --- Custom Crisp Vector Glyphs (replaces OEM emojis everywhere) -----------------------

enum class Glyph {
    RelayMark, Lock, Unlock, ShieldLock, Pin, Folder, FolderOpen,
    Paperclip, Image, Terminal, Spark, Download, Server, Key, Check,
    Orbit, Pulse, Bolt, Layers, Radar
}

@Composable
fun VectorGlyph(glyph: Glyph, tint: Color = LocalPalette.current.muted, size: Dp = 16.dp, modifier: Modifier = Modifier) {
    Canvas(modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val sw = (w * 0.095f).coerceAtLeast(1.4f)
        val stroke = Stroke(width = sw, cap = StrokeCap.Round, join = StrokeJoin.Round)
        when (glyph) {
            Glyph.RelayMark -> {
                val r = w * 0.12f
                val top = Offset(w * 0.5f, h * 0.18f)
                val left = Offset(w * 0.2f, h * 0.72f)
                val right = Offset(w * 0.8f, h * 0.72f)
                drawLine(tint.copy(alpha = 0.75f), top, left, sw)
                drawLine(tint.copy(alpha = 0.75f), top, right, sw)
                drawLine(tint.copy(alpha = 0.75f), left, right, sw)
                drawCircle(tint, r, top)
                drawCircle(tint, r, left)
                drawCircle(tint, r, right)
            }
            Glyph.Orbit -> {
                val c = Offset(w * 0.5f, h * 0.5f)
                drawCircle(tint.copy(alpha = 0.18f), w * 0.38f, c)
                drawArc(
                    color = tint,
                    startAngle = -35f,
                    sweepAngle = 215f,
                    useCenter = false,
                    topLeft = Offset(w * 0.12f, h * 0.12f),
                    size = Size(w * 0.76f, h * 0.76f),
                    style = stroke,
                )
                drawArc(
                    color = tint.copy(alpha = 0.65f),
                    startAngle = 155f,
                    sweepAngle = 115f,
                    useCenter = false,
                    topLeft = Offset(w * 0.24f, h * 0.24f),
                    size = Size(w * 0.52f, h * 0.52f),
                    style = stroke,
                )
                drawCircle(tint, w * 0.13f, c)
                drawCircle(tint, w * 0.09f, Offset(w * 0.80f, h * 0.26f))
            }
            Glyph.Pulse -> {
                val bars = floatArrayOf(0.32f, 0.72f, 1.0f, 0.58f, 0.36f)
                val step = w * 0.16f
                val startX = w * 0.18f
                for (i in bars.indices) {
                    val x = startX + i * step
                    val half = h * 0.38f * bars[i]
                    drawLine(
                        color = if (i == 2) tint else tint.copy(alpha = 0.82f),
                        start = Offset(x, h * 0.5f - half),
                        end = Offset(x, h * 0.5f + half),
                        strokeWidth = sw * 1.05f,
                        cap = StrokeCap.Round,
                    )
                }
            }
            Glyph.Bolt -> {
                val bolt = Path().apply {
                    moveTo(w * 0.56f, h * 0.10f)
                    lineTo(w * 0.22f, h * 0.54f)
                    lineTo(w * 0.48f, h * 0.54f)
                    lineTo(w * 0.42f, h * 0.90f)
                    lineTo(w * 0.78f, h * 0.44f)
                    lineTo(w * 0.52f, h * 0.44f)
                    close()
                }
                drawPath(bolt, tint.copy(alpha = 0.18f))
                drawPath(bolt, tint, style = stroke)
            }
            Glyph.Layers -> {
                val topPlane = Path().apply {
                    moveTo(w * 0.50f, h * 0.14f)
                    lineTo(w * 0.86f, h * 0.33f)
                    lineTo(w * 0.50f, h * 0.52f)
                    lineTo(w * 0.14f, h * 0.33f)
                    close()
                }
                val midPlane = Path().apply {
                    moveTo(w * 0.16f, h * 0.51f)
                    lineTo(w * 0.50f, h * 0.69f)
                    lineTo(w * 0.84f, h * 0.51f)
                }
                val botPlane = Path().apply {
                    moveTo(w * 0.16f, h * 0.68f)
                    lineTo(w * 0.50f, h * 0.86f)
                    lineTo(w * 0.84f, h * 0.68f)
                }
                drawPath(topPlane, tint.copy(alpha = 0.16f))
                drawPath(topPlane, tint, style = stroke)
                drawPath(midPlane, tint.copy(alpha = 0.85f), style = stroke)
                drawPath(botPlane, tint.copy(alpha = 0.65f), style = stroke)
            }
            Glyph.Radar -> {
                val c = Offset(w * 0.5f, h * 0.5f)
                drawArc(
                    color = tint.copy(alpha = 0.20f),
                    startAngle = -65f,
                    sweepAngle = 65f,
                    useCenter = true,
                    topLeft = Offset(w * 0.12f, h * 0.12f),
                    size = Size(w * 0.76f, h * 0.76f),
                )
                drawCircle(tint, w * 0.38f, c, style = stroke)
                drawCircle(tint.copy(alpha = 0.52f), w * 0.21f, c, style = Stroke(width = sw * 0.8f))
                drawLine(tint, c, Offset(w * 0.77f, h * 0.23f), sw, StrokeCap.Round)
                drawCircle(tint, w * 0.085f, c)
                drawCircle(tint, w * 0.09f, Offset(w * 0.69f, h * 0.31f))
            }
            Glyph.Lock -> {
                val archW = w * 0.44f
                val archLeft = (w - archW) / 2f
                drawArc(
                    color = tint,
                    startAngle = 180f,
                    sweepAngle = 180f,
                    useCenter = false,
                    topLeft = Offset(archLeft, h * 0.14f),
                    size = Size(archW, h * 0.42f),
                    style = stroke,
                )
                drawRoundRect(
                    color = tint,
                    topLeft = Offset(w * 0.20f, h * 0.44f),
                    size = Size(w * 0.60f, h * 0.44f),
                    cornerRadius = CornerRadius(w * 0.12f),
                    style = stroke,
                )
                drawCircle(tint, sw * 0.9f, Offset(w * 0.5f, h * 0.65f))
            }
            Glyph.Unlock -> {
                val archW = w * 0.44f
                drawArc(
                    color = tint,
                    startAngle = 180f,
                    sweepAngle = 145f,
                    useCenter = false,
                    topLeft = Offset(w * 0.34f, h * 0.12f),
                    size = Size(archW, h * 0.42f),
                    style = stroke,
                )
                drawRoundRect(
                    color = tint,
                    topLeft = Offset(w * 0.20f, h * 0.45f),
                    size = Size(w * 0.60f, h * 0.43f),
                    cornerRadius = CornerRadius(w * 0.12f),
                    style = stroke,
                )
            }
            Glyph.ShieldLock -> {
                val shield = Path().apply {
                    moveTo(w * 0.5f, h * 0.10f)
                    lineTo(w * 0.84f, h * 0.24f)
                    lineTo(w * 0.84f, h * 0.52f)
                    cubicTo(w * 0.84f, h * 0.74f, w * 0.66f, h * 0.86f, w * 0.5f, h * 0.92f)
                    cubicTo(w * 0.34f, h * 0.86f, w * 0.16f, h * 0.74f, w * 0.16f, h * 0.52f)
                    lineTo(w * 0.16f, h * 0.24f)
                    close()
                }
                drawPath(shield, tint.copy(alpha = 0.14f))
                drawPath(shield, tint, style = stroke)
                drawRoundRect(
                    color = tint,
                    topLeft = Offset(w * 0.36f, h * 0.47f),
                    size = Size(w * 0.28f, h * 0.23f),
                    cornerRadius = CornerRadius(w * 0.06f),
                    style = stroke,
                )
                drawArc(
                    color = tint,
                    startAngle = 180f,
                    sweepAngle = 180f,
                    useCenter = false,
                    topLeft = Offset(w * 0.41f, h * 0.35f),
                    size = Size(w * 0.18f, h * 0.20f),
                    style = stroke,
                )
            }
            Glyph.Pin -> {
                val path = Path().apply {
                    moveTo(w * 0.35f, h * 0.18f)
                    lineTo(w * 0.65f, h * 0.18f)
                    lineTo(w * 0.62f, h * 0.46f)
                    lineTo(w * 0.74f, h * 0.58f)
                    lineTo(w * 0.26f, h * 0.58f)
                    lineTo(w * 0.38f, h * 0.46f)
                    close()
                }
                drawPath(path, tint, style = stroke)
                drawLine(tint, Offset(w * 0.5f, h * 0.58f), Offset(w * 0.5f, h * 0.86f), sw, StrokeCap.Round)
            }
            Glyph.Folder -> {
                val path = Path().apply {
                    moveTo(w * 0.12f, h * 0.26f)
                    lineTo(w * 0.40f, h * 0.26f)
                    lineTo(w * 0.48f, h * 0.36f)
                    lineTo(w * 0.88f, h * 0.36f)
                    lineTo(w * 0.88f, h * 0.78f)
                    lineTo(w * 0.12f, h * 0.78f)
                    close()
                }
                drawPath(path, tint.copy(alpha = 0.14f))
                drawPath(path, tint, style = stroke)
            }
            Glyph.FolderOpen -> {
                val back = Path().apply {
                    moveTo(w * 0.12f, h * 0.24f)
                    lineTo(w * 0.38f, h * 0.24f)
                    lineTo(w * 0.46f, h * 0.34f)
                    lineTo(w * 0.84f, h * 0.34f)
                    lineTo(w * 0.84f, h * 0.48f)
                    lineTo(w * 0.12f, h * 0.76f)
                    close()
                }
                val flap = Path().apply {
                    moveTo(w * 0.12f, h * 0.78f)
                    lineTo(w * 0.24f, h * 0.48f)
                    lineTo(w * 0.92f, h * 0.48f)
                    lineTo(w * 0.80f, h * 0.78f)
                    close()
                }
                drawPath(back, tint, style = stroke)
                drawPath(flap, tint.copy(alpha = 0.18f))
                drawPath(flap, tint, style = stroke)
            }
            Glyph.Paperclip -> {
                val p = Path().apply {
                    moveTo(w * 0.68f, h * 0.34f)
                    lineTo(w * 0.38f, h * 0.64f)
                    cubicTo(w * 0.28f, h * 0.74f, w * 0.44f, h * 0.88f, w * 0.56f, h * 0.76f)
                    lineTo(w * 0.82f, h * 0.50f)
                    cubicTo(w * 0.94f, h * 0.36f, w * 0.70f, h * 0.14f, w * 0.54f, h * 0.28f)
                    lineTo(w * 0.22f, h * 0.60f)
                }
                drawPath(p, tint, style = stroke)
            }
            Glyph.Image -> {
                drawRoundRect(tint, Offset(w * 0.14f, h * 0.18f), Size(w * 0.72f, h * 0.64f), CornerRadius(w * 0.12f), style = stroke)
                drawCircle(tint, w * 0.07f, Offset(w * 0.36f, h * 0.38f))
                val mtn = Path().apply {
                    moveTo(w * 0.20f, h * 0.74f)
                    lineTo(w * 0.44f, h * 0.52f)
                    lineTo(w * 0.58f, h * 0.65f)
                    lineTo(w * 0.70f, h * 0.54f)
                    lineTo(w * 0.82f, h * 0.74f)
                }
                drawPath(mtn, tint, style = stroke)
            }
            Glyph.Terminal -> {
                drawRoundRect(tint, Offset(w * 0.12f, h * 0.18f), Size(w * 0.76f, h * 0.64f), CornerRadius(w * 0.14f), style = stroke)
                val chevron = Path().apply {
                    moveTo(w * 0.28f, h * 0.38f)
                    lineTo(w * 0.42f, h * 0.50f)
                    lineTo(w * 0.28f, h * 0.62f)
                }
                drawPath(chevron, tint, style = stroke)
                drawLine(tint, Offset(w * 0.50f, h * 0.62f), Offset(w * 0.70f, h * 0.62f), sw, StrokeCap.Round)
            }
            Glyph.Spark -> {
                val star = Path().apply {
                    moveTo(w * 0.5f, h * 0.10f)
                    quadraticTo(w * 0.54f, h * 0.46f, w * 0.90f, h * 0.5f)
                    quadraticTo(w * 0.54f, h * 0.54f, w * 0.5f, h * 0.90f)
                    quadraticTo(w * 0.46f, h * 0.54f, w * 0.10f, h * 0.5f)
                    quadraticTo(w * 0.46f, h * 0.46f, w * 0.5f, h * 0.10f)
                    close()
                }
                drawPath(star, tint.copy(alpha = 0.2f))
                drawPath(star, tint, style = stroke)
            }
            Glyph.Download -> {
                drawLine(tint, Offset(w * 0.5f, h * 0.16f), Offset(w * 0.5f, h * 0.64f), sw, StrokeCap.Round)
                val arrow = Path().apply {
                    moveTo(w * 0.30f, h * 0.46f)
                    lineTo(w * 0.5f, h * 0.66f)
                    lineTo(w * 0.70f, h * 0.46f)
                }
                drawPath(arrow, tint, style = stroke)
                val tray = Path().apply {
                    moveTo(w * 0.18f, h * 0.66f)
                    lineTo(w * 0.18f, h * 0.82f)
                    lineTo(w * 0.82f, h * 0.82f)
                    lineTo(w * 0.82f, h * 0.66f)
                }
                drawPath(tray, tint, style = stroke)
            }
            Glyph.Server -> {
                drawRoundRect(tint, Offset(w * 0.14f, h * 0.18f), Size(w * 0.72f, h * 0.27f), CornerRadius(w * 0.08f), style = stroke)
                drawRoundRect(tint, Offset(w * 0.14f, h * 0.55f), Size(w * 0.72f, h * 0.27f), CornerRadius(w * 0.08f), style = stroke)
                drawCircle(tint, sw * 0.9f, Offset(w * 0.28f, h * 0.315f))
                drawCircle(tint, sw * 0.9f, Offset(w * 0.28f, h * 0.685f))
            }
            Glyph.Key -> {
                drawCircle(tint, w * 0.18f, Offset(w * 0.34f, h * 0.5f), style = stroke)
                drawLine(tint, Offset(w * 0.52f, h * 0.5f), Offset(w * 0.86f, h * 0.5f), sw, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.72f, h * 0.5f), Offset(w * 0.72f, h * 0.68f), sw, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.84f, h * 0.5f), Offset(w * 0.84f, h * 0.64f), sw, StrokeCap.Round)
            }
            Glyph.Check -> {
                val p = Path().apply {
                    moveTo(w * 0.20f, h * 0.52f)
                    lineTo(w * 0.42f, h * 0.74f)
                    lineTo(w * 0.82f, h * 0.28f)
                }
                drawPath(p, tint, style = stroke)
            }
        }
    }
}

/** Atmospheric top horizon bloom + 1dp specular border catchlight helpers. */
@Composable
fun AtmosphericHorizon(glowColor: Color, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    val animatedGlow by animateColorAsState(glowColor, tween(220), label = "horizon")
    Canvas(modifier.fillMaxWidth().height(210.dp)) {
        val w = size.width
        val h = size.height
        drawRect(
            brush = Brush.verticalGradient(
                colors = listOf(
                    animatedGlow.copy(alpha = if (p.light) 0.09f else 0.13f),
                    animatedGlow.copy(alpha = if (p.light) 0.03f else 0.045f),
                    Color.Transparent,
                ),
                startY = 0f,
                endY = h,
            )
        )
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(
                    animatedGlow.copy(alpha = if (p.light) 0.13f else 0.20f),
                    Color.Transparent,
                ),
                center = Offset(w * 0.5f, -h * 0.12f),
                radius = w * 0.75f,
            ),
            radius = w * 0.75f,
            center = Offset(w * 0.5f, -h * 0.12f),
        )
    }
}

fun Modifier.specularBorder(
    shape: RoundedCornerShape,
    topColor: Color,
    bottomColor: Color,
    width: Dp = 1.dp,
): Modifier = this.border(width, Brush.verticalGradient(listOf(topColor, bottomColor)), shape)

/** Multi-layered concentric orbital core emblem — zero infinite transitions so Robolectric stays instant. */
@Composable
fun OrbitalEmblem(color: Color, active: Boolean = true, size: Dp = 30.dp, modifier: Modifier = Modifier) {
    val tone by animateColorAsState(color, tween(220), label = "orbTone")
    val boost by animateFloatAsState(if (active) 1f else 0.65f, tween(220), label = "orbBoost")
    Canvas(modifier.size(size)) {
        val w = this.size.width
        val r = this.size.minDimension * 0.44f
        val c = Offset(w * 0.5f, this.size.height * 0.5f)
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(tone.copy(alpha = 0.28f * boost), Color.Transparent),
                center = c,
                radius = r * 1.15f,
            ),
            radius = r * 1.15f,
            center = c,
        )
        val outerStroke = Stroke(width = (w * 0.068f).coerceAtLeast(1.4f), cap = StrokeCap.Round)
        val innerStroke = Stroke(width = (w * 0.055f).coerceAtLeast(1.2f), cap = StrokeCap.Round)
        drawCircle(tone.copy(alpha = 0.16f), r, c, style = outerStroke)
        drawArc(
            color = tone.copy(alpha = 0.92f * boost),
            startAngle = -55f,
            sweepAngle = 215f,
            useCenter = false,
            topLeft = Offset(c.x - r, c.y - r),
            size = Size(r * 2f, r * 2f),
            style = outerStroke,
        )
        val rInner = r * 0.64f
        drawArc(
            color = tone.copy(alpha = 0.65f * boost),
            startAngle = 125f,
            sweepAngle = 175f,
            useCenter = false,
            topLeft = Offset(c.x - rInner, c.y - rInner),
            size = Size(rInner * 2f, rInner * 2f),
            style = innerStroke,
        )
        drawCircle(tone, r * 0.24f, c)
        if (active) {
            val ang = Math.toRadians(-35.0)
            val sat = Offset(c.x + (r * cos(ang)).toFloat(), c.y + (r * sin(ang)).toFloat())
            drawCircle(tone, (w * 0.075f).coerceAtLeast(1.8f), sat)
        }
    }
}

/** 5-bar geometric telemetry waveform — crisp on screen, instant idle in Robolectric. */
@Composable
fun NeuralWaveform(color: Color, active: Boolean = true, width: Dp = 22.dp, height: Dp = 13.dp) {
    val tone by animateColorAsState(color, tween(220), label = "waveColor")
    val gain by animateFloatAsState(if (active) 1f else 0.45f, tween(220), label = "waveGain")
    Canvas(Modifier.width(width).height(height)) {
        val levels = floatArrayOf(0.42f, 0.82f, 1.0f, 0.68f, 0.46f)
        val sw = (size.width * 0.11f).coerceAtLeast(1.6f)
        val gap = (size.width - sw) / (levels.size - 1).coerceAtLeast(1)
        for (i in levels.indices) {
            val x = sw * 0.5f + i * gap
            val hBar = (size.height * 0.45f * levels[i] * gain).coerceAtLeast(sw * 0.6f)
            drawLine(
                color = if (i == 2) tone else tone.copy(alpha = 0.78f),
                start = Offset(x, size.height * 0.5f - hBar),
                end = Offset(x, size.height * 0.5f + hBar),
                strokeWidth = sw,
                cap = StrokeCap.Round,
            )
        }
    }
}

@Composable
fun BackendBadge(backend: String, compact: Boolean = false, showDot: Boolean = false, onClick: (() -> Unit)? = null) {
    val p = LocalPalette.current
    val tone = backendColor(p, backend)
    val label = backendShortLabel(backend)
    val shape = RoundedCornerShape(if (compact) 6.dp else 8.dp)
    Row(
        Modifier
            .clip(shape)
            .background(tone.copy(alpha = if (p.light) 0.12f else 0.16f))
            .specularBorder(shape, tone.copy(alpha = 0.48f), tone.copy(alpha = 0.24f))
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = if (compact) 6.dp else 8.dp, vertical = if (compact) 2.dp else 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (showDot) {
            Box(Modifier.size(if (compact) 5.dp else 6.dp).clip(CircleShape).background(tone))
        }
        Label(label, if (compact) 10.5.sp else 11.5.sp, tone, FontWeight.SemiBold, maxLines = 1)
    }
}

// --- Core Building Blocks --------------------------------------------------------------

@Composable
fun Label(
    text: String,
    size: TextUnit = 14.sp,
    color: Color = LocalPalette.current.text,
    weight: FontWeight = FontWeight.Normal,
    maxLines: Int = Int.MAX_VALUE,
    modifier: Modifier = Modifier,
) = Text(
    text = text,
    modifier = modifier,
    color = color,
    fontSize = size,
    fontWeight = weight,
    maxLines = maxLines,
    overflow = TextOverflow.Ellipsis,
)

@Composable
fun IconButton(icon: ImageVector, hint: String, tint: Color = LocalPalette.current.muted, onClick: () -> Unit) {
    Box(
        Modifier.size(36.dp).clip(RoundedCornerShape(11.dp)).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = hint, tint = tint, modifier = Modifier.size(19.dp))
    }
}

@Composable
fun GlyphButton(glyph: Glyph, hint: String, tint: Color = LocalPalette.current.muted, highlighted: Boolean = false, onClick: () -> Unit) {
    val p = LocalPalette.current
    Box(
        Modifier
            .size(36.dp)
            .clip(RoundedCornerShape(11.dp))
            .background(if (highlighted) tint.copy(alpha = 0.14f) else Color.Transparent)
            .then(if (highlighted) Modifier.border(1.dp, tint.copy(alpha = 0.38f), RoundedCornerShape(11.dp)) else Modifier)
            .semantics { contentDescription = hint }
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        VectorGlyph(glyph, tint = if (highlighted) tint else p.muted, size = 18.dp)
    }
}

@Composable
fun PrimaryButton(text: String, enabled: Boolean = true, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val p = LocalPalette.current
    val shape = RoundedCornerShape(12.dp)
    Box(
        modifier
            .clip(shape)
            .background(if (enabled) p.accent else p.line)
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 11.dp),
        contentAlignment = Alignment.Center,
    ) {
        Label(
            text = text,
            size = 14.sp,
            color = if (!enabled) p.muted else if (p.light) Color.White else Color(0xFF0E1318),
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

@Composable
fun SecondaryPillButton(text: String, glyph: Glyph? = null, tint: Color = LocalPalette.current.text, enabled: Boolean = true, onClick: () -> Unit) {
    val p = LocalPalette.current
    val shape = RoundedCornerShape(10.dp)
    Row(
        Modifier
            .clip(shape)
            .background(p.elevated)
            .specularBorder(shape, p.specularTop, p.borderSubtle)
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 6.5.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        if (glyph != null) VectorGlyph(glyph, if (enabled) tint else p.muted, size = 13.5.dp)
        Label(text, 12.sp, if (enabled) tint else p.muted, FontWeight.Medium, maxLines = 1)
    }
}

@Composable
fun GhostButton(text: String, color: Color = LocalPalette.current.text, onClick: () -> Unit) {
    Box(
        Modifier
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 10.dp),
    ) {
        Label(text, 13.5.sp, color, FontWeight.Medium, maxLines = 1)
    }
}

@Composable
fun Field(
    value: String,
    onChange: (String) -> Unit,
    hint: String,
    password: Boolean = false,
    number: Boolean = false,
    singleLine: Boolean = true,
    modifier: Modifier = Modifier,
) {
    val p = LocalPalette.current
    val shape = RoundedCornerShape(12.dp)
    BasicTextField(
        value = value,
        onValueChange = onChange,
        singleLine = singleLine,
        textStyle = TextStyle(color = p.text, fontSize = 15.sp),
        cursorBrush = SolidColor(p.accent),
        visualTransformation = if (password) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(
            keyboardType = when {
                password -> KeyboardType.Password
                number -> KeyboardType.Number
                else -> KeyboardType.Text
            }
        ),
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(p.base)
            .border(1.dp, if (value.isNotEmpty()) p.mix(p.line, p.accent, 0.25f) else p.line, shape)
            .padding(horizontal = 13.dp, vertical = 11.dp),
        decorationBox = { inner ->
            Box {
                if (value.isEmpty()) Label(hint, 14.5.sp, p.muted, maxLines = 1)
                inner()
            }
        },
    )
}

/** Static dot when idle so Compose `waitForIdle()` never blocks. */
@Composable
fun StatusDot(link: Link) {
    val p = LocalPalette.current
    val color = when (link) {
        Link.Online -> p.green
        Link.Connecting -> p.amber
        Link.Off -> p.muted.copy(alpha = 0.6f)
    }
    Box(Modifier.size(12.dp), contentAlignment = Alignment.Center) {
        if (link == Link.Online) {
            Box(Modifier.size(12.dp).clip(CircleShape).background(color.copy(alpha = 0.22f)))
        }
        Box(Modifier.size(6.5.dp).clip(CircleShape).background(color))
    }
}

@Composable
fun TopBar(
    title: String,
    subtitle: String? = null,
    link: Link? = null,
    onBack: (() -> Unit)? = null,
    actions: @Composable () -> Unit = {},
) {
    val p = LocalPalette.current
    Column(Modifier.fillMaxWidth().background(p.mix(p.rail, p.side, 0.55f).copy(alpha = 0.92f))) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (onBack != null) {
                IconButton(Icons.AutoMirrored.Filled.ArrowBack, "Назад", p.text, onBack)
            } else {
                Spacer(Modifier.width(10.dp))
            }
            Column(Modifier.weight(1f).padding(horizontal = 6.dp)) {
                Label(title, 16.5.sp, p.text, FontWeight.SemiBold, maxLines = 1)
                if (subtitle != null) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                        if (link != null) StatusDot(link)
                        Label(subtitle, 12.sp, p.muted, maxLines = 1)
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                actions()
            }
        }
        HorizontalDivider(thickness = 1.dp, color = p.borderSubtle)
    }
}

/** Finite telemetry indicator — settles in <250ms in Robolectric with zero infinite transitions! */
@Composable
fun Dots(color: Color) {
    NeuralWaveform(color = color, active = true, width = 18.dp, height = 12.dp)
}

/** Bottom-anchored modal sheet built on standard `Dialog` so Robolectric tests stay fast and reliable. */
@Composable
fun Sheet(onDismiss: () -> Unit, content: @Composable () -> Unit) {
    val p = LocalPalette.current
    val sheetShape = RoundedCornerShape(24.dp)
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = onDismiss,
                )
                .padding(horizontal = 10.dp, vertical = 10.dp),
            contentAlignment = Alignment.BottomCenter,
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .widthIn(max = 520.dp)
                    .clip(sheetShape)
                    .background(p.elevated)
                    .specularBorder(sheetShape, p.specularTop, p.line)
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                        onClick = {},
                    )
                    .padding(horizontal = 20.dp, vertical = 16.dp),
            ) {
                Box(
                    Modifier
                        .align(Alignment.CenterHorizontally)
                        .padding(bottom = 14.dp)
                        .width(36.dp)
                        .height(4.dp)
                        .clip(CircleShape)
                        .background(p.muted.copy(alpha = 0.35f)),
                )
                content()
            }
        }
    }
}

// --- Servers Screen --------------------------------------------------------------------

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ServersScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    var editing by remember { mutableStateOf<ServerProfile?>(null) }
    var asking by remember { mutableStateOf<ServerProfile?>(null) }
    var apiEditing by remember { mutableStateOf<ServerProfile?>(null) }
    var agentsSheet by remember { mutableStateOf(false) }

    Box(Modifier.fillMaxSize()) {
        AtmosphericHorizon(p.accent)
        Column(Modifier.fillMaxSize()) {
            TopBar("Relay", "Claude Code · Codex · Antigravity · API") {
                IconButton(Icons.Default.Settings, "Настройки") { vm.screen = Screen.Settings }
            }
            if (vm.notice.isNotEmpty()) Notice(vm.notice) { vm.notice = "" }
            if (vm.crash.isNotEmpty()) CrashCard(vm.crash) { vm.crash = "" }

            LazyColumn(
                Modifier.weight(1f),
                contentPadding = PaddingValues(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                if (vm.servers.isEmpty()) {
                    item {
                        val heroShape = RoundedCornerShape(24.dp)
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .padding(top = 14.dp)
                                .clip(heroShape)
                                .background(
                                    Brush.verticalGradient(
                                        listOf(p.mix(p.surface, p.accent, 0.08f), p.surface)
                                    )
                                )
                                .specularBorder(heroShape, p.accent.copy(alpha = 0.42f), p.borderSubtle)
                                .padding(24.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Box(
                                Modifier
                                    .size(66.dp)
                                    .clip(RoundedCornerShape(20.dp))
                                    .background(p.subtleAccent)
                                    .specularBorder(RoundedCornerShape(20.dp), p.accent.copy(alpha = 0.5f), p.accent.copy(alpha = 0.2f)),
                                contentAlignment = Alignment.Center,
                            ) {
                                OrbitalEmblem(p.accent, active = true, size = 40.dp)
                            }
                            Spacer(Modifier.height(18.dp))
                            Label("Все проекты. Одно пространство.", 20.sp, p.text, FontWeight.SemiBold)
                            Spacer(Modifier.height(8.dp))
                            Label(
                                "Подключи свой сервер по SSH и продолжай общие сессии с ПК — процессы работают на сервере даже без приложения.",
                                13.5.sp,
                                p.muted,
                            )
                            Spacer(Modifier.height(16.dp))
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                for (b in BACKENDS) BackendBadge(b, showDot = true)
                            }
                            Spacer(Modifier.height(18.dp))
                            SecondaryPillButton("Посмотреть без сервера — демо", Glyph.Spark, p.accent) { vm.startDemo() }
                        }
                    }
                } else {
                    items(vm.servers, key = { it.id }) { s ->
                        val online = vm.current?.id == s.id && vm.link == Link.Online
                        val tone = backendColor(p, s.effectiveBackend())
                        val cardShape = RoundedCornerShape(18.dp)
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .clip(cardShape)
                                .background(p.surface)
                                .specularBorder(
                                    cardShape,
                                    if (online) p.accent.copy(alpha = 0.58f) else p.specularTop,
                                    if (online) p.accent.copy(alpha = 0.28f) else p.line,
                                )
                                .clickable {
                                    if (online) vm.screen = Screen.Sessions
                                    else vm.savedPassword(s)?.let { vm.connect(s, it) } ?: run { asking = s }
                                }
                                .padding(15.dp),
                        ) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Box(
                                    Modifier
                                        .size(44.dp)
                                        .clip(RoundedCornerShape(13.dp))
                                        .background(tone.copy(alpha = 0.14f))
                                        .border(1.dp, tone.copy(alpha = 0.32f), RoundedCornerShape(13.dp)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    VectorGlyph(Glyph.Server, tone, size = 21.dp)
                                }
                                Column(Modifier.weight(1f).padding(horizontal = 13.dp)) {
                                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                                        Label(s.name.ifEmpty { s.host }, 16.sp, p.text, FontWeight.SemiBold, maxLines = 1, modifier = Modifier.weight(1f, fill = false))
                                        BackendBadge(s.effectiveBackend(), compact = true, showDot = true)
                                    }
                                    Spacer(Modifier.height(2.dp))
                                    Text(
                                        "${s.user}@${s.host}" + (if (s.port != 22) ":${s.port}" else "") + " · ${s.cwd}",
                                        color = p.muted,
                                        fontSize = 12.sp,
                                        fontFamily = FontFamily.Monospace,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                                if (online) StatusDot(Link.Online)
                                IconButton(Icons.Default.MoreVert, "Изменить") { editing = s }
                            }
                            Spacer(Modifier.height(10.dp))
                            Row(
                                Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                SecondaryPillButton("API", Glyph.Key, p.accent) { apiEditing = s }
                                if (online) {
                                    SecondaryPillButton("Агенты", Glyph.Terminal, p.text) { agentsSheet = true }
                                }
                                Spacer(Modifier.weight(1f))
                                Label(if (online) "Открыть сессии →" else "Подключиться →", 12.5.sp, if (online) p.green else p.accent, FontWeight.Medium)
                            }
                        }
                    }
                    item {
                        Box(Modifier.fillMaxWidth().padding(top = 4.dp), contentAlignment = Alignment.Center) {
                            GhostButton("Посмотреть без сервера — демо", p.accent) { vm.startDemo() }
                        }
                    }
                }
            }
            Column(Modifier.fillMaxWidth().background(p.mix(p.rail, p.base, 0.5f))) {
                HorizontalDivider(thickness = 1.dp, color = p.borderSubtle)
                Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)) {
                    PrimaryButton("Добавить сервер", modifier = Modifier.fillMaxWidth()) { editing = ServerProfile() }
                }
            }
        }
    }

    editing?.let { s -> ServerDialog(vm, s, vm.servers.any { it.id == s.id }) { editing = null } }
    apiEditing?.let { s -> ApiDialog(vm, s) { apiEditing = null } }
    if (agentsSheet) AgentsSheet(vm) { agentsSheet = false }
    asking?.let { s ->
        var password by remember { mutableStateOf("") }
        var rememberPw by remember { mutableStateOf(true) }
        Sheet({ asking = null }) {
            Label("Пароль для ${s.user}@${s.host}", 17.sp, p.text, FontWeight.SemiBold)
            Spacer(Modifier.height(12.dp))
            Field(password, { password = it }, "Пароль сервера", password = true)
            Check("Запомнить пароль", rememberPw) { rememberPw = it }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Отмена") { asking = null }
                Spacer(Modifier.width(6.dp))
                PrimaryButton("Подключиться", password.isNotEmpty()) {
                    if (rememberPw) vm.saveServer(s, password, true)
                    vm.connect(s, password)
                    asking = null
                }
            }
        }
    }
}

@Composable
fun ApiDialog(vm: RelayViewModel, initial: ServerProfile, close: () -> Unit) {
    val p = LocalPalette.current
    var base by remember { mutableStateOf(initial.api.baseUrl.ifBlank { "https://tooken.club/v1" }) }
    var model by remember { mutableStateOf(initial.api.model.ifBlank { "claude-sonnet-5-5" }) }
    var key by remember { mutableStateOf(vm.savedApiKey(initial)) }
    var enabled by remember { mutableStateOf(initial.api.enabled || initial.effectiveBackend() == "api") }
    var tokens by remember { mutableStateOf(initial.api.maxTokens.toString()) }
    var steps by remember { mutableStateOf(initial.api.maxSteps.toString()) }
    val catalog = if (vm.current?.id == initial.id) vm.apiCatalog() else initial.api.models.map { ModelInfo(it, it, "") }
    fun profile() = ApiProfile(enabled, base.trim().trimEnd('/'), model.trim(), (tokens.toIntOrNull() ?: 8192).coerceIn(256, 65536), (steps.toIntOrNull() ?: 80).coerceIn(1, 256), catalog.map { it.value })
    Sheet(close) {
        Label("API · ${initial.name.ifBlank { initial.host }}", 18.sp, p.text, FontWeight.SemiBold)
        Spacer(Modifier.height(12.dp))
        Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(9.dp)) {
            Check("API по умолчанию для новых сессий", enabled) { enabled = it }
            Field(base, { base = it }, "Адрес API (https://…/v1)")
            Field(key, { key = it }, "API-ключ", password = true)
            Field(model, { model = it }, "Идентификатор модели")
            val models = catalog.filterNot { it.value.contains("image") }
            if (models.isNotEmpty()) Selector(modelLabel(models, model), models.map { it.value to modelLabel(models, it.value) }) { model = it }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(tokens, { tokens = it.filter(Char::isDigit).take(5) }, "Токенов на ответ", number = true, modifier = Modifier.weight(1f))
                Field(steps, { steps = it.filter(Char::isDigit).take(3) }, "Шагов", number = true, modifier = Modifier.weight(1f))
            }
            SecondaryPillButton(if (vm.apiChecking) "Проверяю…" else "Проверить и загрузить модели", Glyph.Spark, p.accent) {
                vm.checkApi(initial, profile(), key)
            }
            if (vm.apiError.isNotEmpty()) Label(vm.apiError, 12.sp, p.red)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Отмена", onClick = close)
                Spacer(Modifier.width(6.dp))
                PrimaryButton("Сохранить", base.isNotBlank() && model.isNotBlank()) { if (vm.saveApi(initial, profile(), key)) close() }
            }
        }
    }
}

@Composable
fun Check(text: String, value: Boolean, onChange: (Boolean) -> Unit) {
    val p = LocalPalette.current
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { onChange(!value) }.padding(vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val knob by animateFloatAsState(if (value) 1f else 0f, tween(180), label = "toggle")
        Box(
            Modifier.width(38.dp).height(22.dp).clip(CircleShape).background(p.mix(p.line, p.accent, knob)),
            contentAlignment = Alignment.CenterStart,
        ) {
            Box(Modifier.padding(start = (3 + 16 * knob).dp).size(16.dp).clip(CircleShape).background(if (value) Color.White else p.muted))
        }
        Spacer(Modifier.width(10.dp))
        Label(text, 14.sp, p.text)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ServerDialog(vm: RelayViewModel, initial: ServerProfile, existing: Boolean, close: () -> Unit) {
    val p = LocalPalette.current
    var name by remember { mutableStateOf(initial.name) }
    var host by remember { mutableStateOf(initial.host) }
    var port by remember { mutableStateOf(initial.port.toString()) }
    var user by remember { mutableStateOf(initial.user) }
    var cwd by remember { mutableStateOf(initial.cwd) }
    var defaultBackend by remember { mutableStateOf(initial.effectiveBackend()) }
    var password by remember { mutableStateOf("") }
    var rememberPw by remember { mutableStateOf(true) }
    Sheet(close) {
        Label(if (existing) "Настройки сервера" else "Новое подключение", 18.sp, p.text, FontWeight.SemiBold)
        Spacer(Modifier.height(12.dp))
        Column(Modifier.heightIn(max = 540.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(name, { name = it }, "Название")
            Field(host, { host = it.trim() }, "Адрес сервера")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(user, { user = it.trim() }, "Пользователь", modifier = Modifier.weight(2f))
                Field(port, { port = it.filter(Char::isDigit).take(5) }, "Порт", number = true, modifier = Modifier.weight(1f))
            }
            Field(password, { password = it }, if (existing) "Пароль (пусто — не менять)" else "Пароль", password = true)
            Field(cwd, { cwd = it }, "Рабочая папка (~ — домашняя)")
            Spacer(Modifier.height(2.dp))
            Label("Агент по умолчанию", 12.5.sp, p.muted, FontWeight.Medium)
            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (b in BACKENDS) {
                    val active = defaultBackend == b
                    val tone = backendColor(p, b)
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(9.dp))
                            .background(if (active) tone.copy(alpha = 0.18f) else p.base)
                            .border(1.dp, if (active) tone else p.line, RoundedCornerShape(9.dp))
                            .clickable { defaultBackend = b }
                            .padding(horizontal = 10.dp, vertical = 6.dp),
                    ) {
                        Label(backendShortLabel(b), 12.5.sp, if (active) tone else p.muted, FontWeight.Medium)
                    }
                }
            }
            Check("Запомнить пароль", rememberPw) { rememberPw = it }
            if (existing && initial.fingerprint != null) {
                Label("Ключ сервера: ${initial.fingerprint}", 11.sp, p.muted)
                GhostButton("Забыть ключ (после переустановки сервера)", p.accent) {
                    vm.saveServer(initial.copy(fingerprint = null), null, false)
                    close()
                }
            }
            Spacer(Modifier.height(4.dp))
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                if (existing) GhostButton("Удалить", p.red) { vm.deleteServer(initial); close() }
                Spacer(Modifier.weight(1f))
                GhostButton("Отмена", onClick = close)
                Spacer(Modifier.width(6.dp))
                val profile = initial.copy(
                    name = name.trim(),
                    host = host,
                    port = port.toIntOrNull() ?: 22,
                    user = user.ifEmpty { "root" },
                    cwd = cwd.ifBlank { "~" },
                    defaultBackend = defaultBackend,
                    api = initial.api.copy(enabled = defaultBackend == "api"),
                )
                PrimaryButton(if (existing) "Сохранить" else "Подключиться", host.isNotEmpty() && (existing || password.isNotEmpty())) {
                    vm.saveServer(profile, password.takeIf { it.isNotEmpty() }, rememberPw)
                    if (!existing) vm.connect(profile, password)
                    close()
                }
            }
        }
    }
}

@Composable
fun TrustDialog(request: TrustRequest, vm: RelayViewModel) {
    val p = LocalPalette.current
    Sheet({ vm.answerTrust(false) }) {
        Label("Первое подключение", 18.sp, p.text, FontWeight.SemiBold)
        Spacer(Modifier.height(8.dp))
        Label(
            "Сверь отпечаток ключа ${request.server} с тем, что показывает сервер (ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub). Изменившийся ключ потом не примется молча.",
            13.sp,
            p.muted,
        )
        Spacer(Modifier.height(12.dp))
        Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(p.base).border(1.dp, p.line, RoundedCornerShape(12.dp)).padding(12.dp)) {
            Text(request.fingerprint, color = p.text, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
        }
        Spacer(Modifier.height(14.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            GhostButton("Отмена") { vm.answerTrust(false) }
            Spacer(Modifier.width(6.dp))
            PrimaryButton("Доверять") { vm.answerTrust(true) }
        }
    }
}

@Composable
fun Notice(text: String, close: () -> Unit) {
    val p = LocalPalette.current
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(p.subtleAccent)
            .border(1.dp, p.accent.copy(alpha = 0.3f), RoundedCornerShape(12.dp))
            .padding(horizontal = 13.dp, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(text, 13.sp, p.text, modifier = Modifier.weight(1f))
        GhostButton("OK", p.accent, close)
    }
}

@Composable
fun CrashCard(text: String, close: () -> Unit) {
    val p = LocalPalette.current
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Column(
        Modifier
            .fillMaxWidth()
            .padding(12.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(p.red.copy(alpha = 0.12f))
            .border(1.dp, p.red.copy(alpha = 0.4f), RoundedCornerShape(14.dp))
            .padding(14.dp),
    ) {
        Label("Прошлый запуск закончился ошибкой", 14.sp, p.text, FontWeight.SemiBold)
        Spacer(Modifier.height(4.dp))
        Label("Скопируй отчёт и пришли его в Relay — так ошибку можно будет исправить.", 13.sp, p.muted)
        Spacer(Modifier.height(8.dp))
        Label(text.lineSequence().take(4).joinToString("\n"), 11.sp, p.muted, maxLines = 4)
        Spacer(Modifier.height(10.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PrimaryButton(if (copied) "Скопировано" else "Скопировать отчёт") {
                clipboard.setText(androidx.compose.ui.text.AnnotatedString(text))
                copied = true
            }
            GhostButton("Закрыть", p.muted, close)
        }
    }
}

// --- Sessions Screen -------------------------------------------------------------------

fun dayGroup(time: Double): String {
    val midnight = Calendar.getInstance().apply { set(Calendar.HOUR_OF_DAY, 0); set(Calendar.MINUTE, 0); set(Calendar.SECOND, 0) }.timeInMillis / 1000.0
    return when {
        time >= midnight -> "Сегодня"
        time >= midnight - 86400 -> "Вчера"
        time >= midnight - 6 * 86400 -> "Эта неделя"
        time >= midnight - 29 * 86400 -> "Этот месяц"
        else -> "Ранее"
    }
}

fun relative(time: Double): String {
    val s = (System.currentTimeMillis() / 1000.0 - time).toLong().coerceAtLeast(0)
    return when {
        s < 60 -> "сейчас"
        s < 3600 -> "${s / 60} мин"
        s < 86400 -> "${s / 3600} ч"
        s < 7 * 86400 -> "${s / 86400} дн"
        else -> SimpleDateFormat("d MMM", Locale("ru")).format(Date((time * 1000).toLong()))
    }
}

fun folderName(path: String) = path.trimEnd('/').substringAfterLast('/').ifEmpty { "/" }

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SessionsScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    val server = vm.current
    var query by rememberSaveable { mutableStateOf("") }
    var searching by rememberSaveable { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<Session?>(null) }
    var moving by remember { mutableStateOf<Session?>(null) }
    var naming by remember { mutableStateOf<Pair<Folder?, Session?>?>(null) }
    var locking by remember { mutableStateOf<Session?>(null) }
    var agentsSheet by remember { mutableStateOf(false) }
    var radarSheet by remember { mutableStateOf(false) }

    BackHandler { if (vm.demo) vm.leaveDemo() else vm.screen = Screen.Servers }

    val activeSessions = vm.sessions.filter { s ->
        s.running || s.id in vm.running || vm.chats[s.id]?.let { it.version >= 0 && it.transcript.busy } == true
    }
    val horizonTone = activeSessions.firstOrNull()?.let { backendColor(p, it.backend.ifBlank { server?.effectiveBackend() ?: "claude" }) } ?: p.accent

    Box(Modifier.fillMaxSize().background(p.side)) {
        AtmosphericHorizon(horizonTone)
        Column(Modifier.fillMaxSize()) {
            val status = when (vm.link) {
                Link.Online -> "на связи"
                Link.Connecting -> "подключаемся…"
                Link.Off -> "нет связи"
            }
            TopBar(
                title = server?.name?.ifEmpty { server.host } ?: "Сессии",
                subtitle = status,
                link = vm.link,
                onBack = { if (vm.demo) vm.leaveDemo() else vm.screen = Screen.Servers },
            ) {
                GlyphButton(
                    glyph = Glyph.Radar,
                    hint = "Радар агентов",
                    tint = if (activeSessions.isNotEmpty()) p.green else p.accent,
                    highlighted = activeSessions.isNotEmpty(),
                ) { radarSheet = true }
                SecondaryPillButton("Агенты", Glyph.Terminal, p.accent, enabled = vm.link == Link.Online) { agentsSheet = true }
                IconButton(Icons.Default.Search, "Поиск") { searching = !searching; if (!searching) query = "" }
                IconButton(Icons.Default.Refresh, "Обновить") { vm.refresh() }
                IconButton(Icons.Default.Settings, "Настройки") { vm.screen = Screen.Settings }
            }
            if (vm.notice.isNotEmpty()) Notice(vm.notice) { vm.notice = "" }
            AnimatedVisibility(searching) {
                Box(Modifier.padding(horizontal = 12.dp, vertical = 6.dp)) {
                    Field(query, { query = it }, "Поиск по названию и папке")
                }
            }
            if (vm.link == Link.Off && server != null) {
                Box(Modifier.padding(14.dp)) {
                    PrimaryButton("Подключиться снова", modifier = Modifier.fillMaxWidth()) {
                        vm.savedPassword(server)?.let { vm.connect(server, it) } ?: run { vm.screen = Screen.Servers }
                    }
                }
            }
            if (activeSessions.isNotEmpty() && vm.link == Link.Online) {
                ActiveAgentsBanner(activeSessions, onClick = { radarSheet = true })
            }
            val q = query.lowercase()
            val matches = { s: Session -> q.isEmpty() || s.title.lowercase().contains(q) || s.autoTitle.lowercase().contains(q) || s.cwd.lowercase().contains(q) }
            val known = vm.folders.map { it.id }.toSet()
            val pinned = vm.sessions.filter { it.pinned > 0 && matches(it) }.sortedByDescending { it.pinned }
            val loose = vm.sessions.filter { it.pinned == 0.0 && it.folder !in known && matches(it) }

            LazyColumn(
                Modifier.weight(1f),
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                if (pinned.isNotEmpty()) {
                    item(key = "h-pinned") { Heading(Glyph.Pin, "Закреплённые", pinned.size) }
                    items(pinned, key = { "p" + it.id }) { s ->
                        SessionRow(vm, s, { renaming = s }, { moving = s }, { locking = s }, Modifier.animateItem())
                    }
                }
                for (folder in vm.folders) {
                    val inside = vm.sessions.filter { it.pinned == 0.0 && it.folder == folder.id && matches(it) }
                    if (q.isNotEmpty() && inside.isEmpty()) continue
                    val open = folder.id !in vm.collapsed || q.isNotEmpty()
                    item(key = "f-" + folder.id) {
                        FolderHeading(folder, inside.size, open, { vm.toggleFolder(folder) }, { naming = folder to null }, { vm.deleteFolder(folder) })
                    }
                    if (open) {
                        if (inside.isEmpty()) {
                            item(key = "e-" + folder.id) {
                                Label("Пусто — перенеси сюда сессию через «⋮»", 12.sp, p.muted, modifier = Modifier.padding(start = 36.dp, top = 2.dp, bottom = 6.dp))
                            }
                        }
                        items(inside, key = { "s" + it.id }) { s ->
                            SessionRow(vm, s, { renaming = s }, { moving = s }, { locking = s }, Modifier.animateItem())
                        }
                    }
                }
                var group = ""
                for (s in loose) {
                    val heading = dayGroup(s.modified)
                    if (heading != group) {
                        group = heading
                        item(key = "d-$heading") { Heading(null, heading, 0) }
                    }
                    item(key = "l" + s.id) {
                        SessionRow(vm, s, { renaming = s }, { moving = s }, { locking = s }, Modifier.animateItem())
                    }
                }
                if (vm.sessions.isEmpty()) {
                    item {
                        Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                            when {
                                vm.link == Link.Connecting || (vm.link == Link.Online && !vm.sessionsLoaded) -> Dots(p.muted)
                                vm.link == Link.Online -> Label("Пока нет ни одной сессии", 13.5.sp, p.muted)
                                else -> Label("Подключись, чтобы увидеть сессии", 13.5.sp, p.muted)
                            }
                        }
                    }
                }
            }

            Column(Modifier.fillMaxWidth().background(p.mix(p.rail, p.side, 0.55f))) {
                HorizontalDivider(thickness = 1.dp, color = p.borderSubtle)
                Row(Modifier.fillMaxWidth().padding(12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(
                        Modifier
                            .clip(RoundedCornerShape(12.dp))
                            .background(p.surface)
                            .specularBorder(RoundedCornerShape(12.dp), p.specularTop, p.line)
                            .clickable(enabled = vm.link == Link.Online) { naming = null to null }
                            .padding(horizontal = 15.dp, vertical = 11.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        VectorGlyph(Glyph.Folder, p.muted, size = 15.dp)
                        Label("+ Папка", 14.sp, p.text, FontWeight.Medium)
                    }
                    PrimaryButton("Новая сессия", vm.link == Link.Online, Modifier.weight(1f)) { vm.newChat() }
                }
            }
        }
    }

    if (agentsSheet) AgentsSheet(vm) { agentsSheet = false }
    if (radarSheet) AgentRadarSheet(vm) { radarSheet = false }
    renaming?.let { s ->
        NameDialog("Название сессии", s.title, "Пусто — вернуть название от агента", { renaming = null }) {
            vm.rename(s, it)
            renaming = null
        }
    }
    naming?.let { (folder, session) ->
        NameDialog(if (folder == null) "Новая папка" else "Папка", folder?.name ?: "", "Видна всем, кто подключается к серверу", { naming = null }) {
            if (it.isNotBlank()) {
                if (folder == null) vm.createFolder(it, session) else vm.renameFolder(folder, it)
            }
            naming = null
        }
    }
    moving?.let { s ->
        Sheet({ moving = null }) {
            Label("Переместить в папку", 18.sp, p.text, FontWeight.SemiBold)
            Spacer(Modifier.height(10.dp))
            for (f in vm.folders) {
                MenuLine(f.name, glyph = Glyph.Folder, enabled = s.folder != f.id) { vm.moveTo(s, f.id); moving = null }
            }
            if (s.folder.isNotEmpty()) MenuLine("Без папки") { vm.moveTo(s, ""); moving = null }
            HorizontalDivider(color = p.line, modifier = Modifier.padding(vertical = 6.dp))
            MenuLine("+ Новая папка…") { naming = null to s; moving = null }
        }
    }
    locking?.let { s ->
        SessionLockDialog(vm, s.id, currentlyLocked = s.locked, currentlyUnlocked = s.unlocked) { locking = null }
    }
}

@Composable
fun ActiveAgentsBanner(activeSessions: List<Session>, onClick: () -> Unit) {
    val p = LocalPalette.current
    val leadBackend = activeSessions.first().backend.ifBlank { "claude" }
    val leadTone = backendColor(p, leadBackend)
    val shape = RoundedCornerShape(16.dp)
    val count = activeSessions.size
    val word = when {
        count % 10 == 1 && count % 100 != 11 -> "агент активен"
        count % 10 in 2..4 && count % 100 !in 12..14 -> "агента активны"
        else -> "агентов активны"
    }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .clip(shape)
            .background(
                Brush.horizontalGradient(
                    listOf(
                        p.mix(p.elevated, leadTone, if (p.light) 0.10f else 0.15f),
                        p.elevated,
                    )
                )
            )
            .specularBorder(shape, leadTone.copy(alpha = 0.55f), p.borderSubtle)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        OrbitalEmblem(leadTone, active = true, size = 30.dp)
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Label("$count $word на сервере", 13.sp, p.text, FontWeight.SemiBold, maxLines = 1)
                NeuralWaveform(leadTone, active = true, width = 18.dp, height = 11.dp)
            }
            val names = activeSessions.joinToString(" · ") { if (it.isLockedGate()) "Защищённая сессия" else it.title }
            Label(names, 11.5.sp, p.muted, maxLines = 1)
        }
        Row(
            Modifier
                .clip(RoundedCornerShape(9.dp))
                .background(leadTone.copy(alpha = 0.15f))
                .border(1.dp, leadTone.copy(alpha = 0.38f), RoundedCornerShape(9.dp))
                .padding(horizontal = 9.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            VectorGlyph(Glyph.Radar, leadTone, size = 13.dp)
            Label("Радар", 11.5.sp, leadTone, FontWeight.SemiBold, maxLines = 1)
        }
    }
}

@Composable
fun SessionLockDialog(vm: RelayViewModel, sid: String, currentlyLocked: Boolean, currentlyUnlocked: Boolean, close: () -> Unit) {
    val p = LocalPalette.current
    var currentPw by remember { mutableStateOf("") }
    var newPw by remember { mutableStateOf("") }
    var confirmPw by remember { mutableStateOf("") }
    var localError by remember { mutableStateOf("") }

    Sheet(close) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp)) {
            VectorGlyph(if (currentlyLocked) Glyph.Lock else Glyph.ShieldLock, p.amber, size = 22.dp)
            Label(if (currentlyLocked) "Защита сессии паролем" else "Защитить сессию паролем", 18.sp, p.text, FontWeight.SemiBold)
        }
        Spacer(Modifier.height(6.dp))
        Label(
            if (currentlyLocked) "Введи текущий пароль, чтобы сменить или полностью снять защиту с этой сессии."
            else "История и отправка сообщений будут скрыты до ввода пароля на каждом устройстве.",
            12.5.sp,
            p.muted,
        )
        Spacer(Modifier.height(12.dp))
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (currentlyLocked && currentlyUnlocked) {
                SecondaryPillButton("Заблокировать сейчас на этом устройстве", Glyph.Lock, p.amber) {
                    vm.relockSession(sid)
                    close()
                }
                HorizontalDivider(color = p.borderSubtle, modifier = Modifier.padding(vertical = 4.dp))
            }
            if (currentlyLocked) {
                Field(currentPw, { currentPw = it; localError = "" }, "Текущий пароль сессии", password = true)
            }
            Field(newPw, { newPw = it; localError = "" }, if (currentlyLocked) "Новый пароль (от 4 символов)" else "Пароль сессии (от 4 символов)", password = true)
            Field(confirmPw, { confirmPw = it; localError = "" }, "Повтори пароль", password = true)
            if (localError.isNotEmpty()) Label(localError, 12.sp, p.red)
        }
        Spacer(Modifier.height(14.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            if (currentlyLocked) {
                GhostButton("Снять пароль", p.red) {
                    if (currentPw.isEmpty()) {
                        localError = "Введи текущий пароль, чтобы снять защиту."
                    } else {
                        vm.lockSession(sid, password = "", currentPassword = currentPw, remove = true)
                        close()
                    }
                }
            }
            Spacer(Modifier.weight(1f))
            GhostButton("Отмена", onClick = close)
            Spacer(Modifier.width(6.dp))
            PrimaryButton(if (currentlyLocked) "Сменить" else "Защитить", enabled = newPw.isNotEmpty()) {
                when {
                    currentlyLocked && currentPw.isEmpty() -> localError = "Укажи текущий пароль."
                    newPw.length < 4 -> localError = "Пароль должен быть не короче 4 символов."
                    newPw != confirmPw -> localError = "Пароли не совпадают."
                    else -> {
                        vm.lockSession(sid, password = newPw, currentPassword = currentPw, remove = false)
                        close()
                    }
                }
            }
        }
    }
}

@Composable
fun MenuLine(text: String, glyph: Glyph? = null, enabled: Boolean = true, color: Color = LocalPalette.current.text, onClick: () -> Unit) {
    val p = LocalPalette.current
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (glyph != null) VectorGlyph(glyph, if (enabled) p.accent else p.muted, size = 16.dp)
        Label(text, 15.sp, if (enabled) color else p.muted)
    }
}

@Composable
fun NameDialog(title: String, initial: String, hint: String, close: () -> Unit, done: (String) -> Unit) {
    val p = LocalPalette.current
    var text by remember { mutableStateOf(initial) }
    Sheet(close) {
        Label(title, 18.sp, p.text, FontWeight.SemiBold)
        Spacer(Modifier.height(4.dp))
        Label(hint, 12.5.sp, p.muted)
        Spacer(Modifier.height(12.dp))
        Field(text, { text = it }, "Название")
        Spacer(Modifier.height(14.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            GhostButton("Отмена", onClick = close)
            Spacer(Modifier.width(6.dp))
            PrimaryButton("Готово") { done(text) }
        }
    }
}

@Composable
fun Heading(glyph: Glyph?, text: String, count: Int) {
    val p = LocalPalette.current
    Row(
        Modifier.fillMaxWidth().padding(start = 8.dp, top = 12.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (glyph != null) {
            VectorGlyph(glyph, p.accent, size = 13.dp)
            Spacer(Modifier.width(6.dp))
        }
        Label(text.uppercase(), 11.sp, p.muted, FontWeight.SemiBold)
        if (count > 0) {
            Spacer(Modifier.width(6.dp))
            Label(count.toString(), 11.sp, p.muted.copy(alpha = 0.8f))
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun FolderHeading(folder: Folder, count: Int, open: Boolean, toggle: () -> Unit, rename: () -> Unit, delete: () -> Unit) {
    val p = LocalPalette.current
    var menu by remember { mutableStateOf(false) }
    val turn by animateFloatAsState(if (open) 90f else 0f, tween(180), label = "chevron")
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(top = 8.dp)
                .clip(RoundedCornerShape(11.dp))
                .combinedClickable(onClick = toggle, onLongClick = { menu = true })
                .padding(horizontal = 6.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = p.muted, modifier = Modifier.size(18.dp).rotate(turn))
            Spacer(Modifier.width(4.dp))
            VectorGlyph(if (open) Glyph.FolderOpen else Glyph.Folder, p.accent, size = 16.dp)
            Spacer(Modifier.width(8.dp))
            Label(folder.name, 14.sp, p.text, FontWeight.SemiBold, 1, Modifier.weight(1f, fill = false))
            Spacer(Modifier.width(8.dp))
            Label(count.toString(), 12.sp, p.muted)
        }
        DropdownMenu(menu, { menu = false }, Modifier.background(p.elevated)) {
            DropdownMenuItem({ Label("Переименовать папку") }, { menu = false; rename() })
            DropdownMenuItem({ Label("Удалить папку", color = p.red) }, { menu = false; delete() })
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SessionRow(
    vm: RelayViewModel,
    s: Session,
    rename: () -> Unit,
    move: () -> Unit,
    lockAction: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val p = LocalPalette.current
    var menu by remember { mutableStateOf(false) }
    var confirm by remember { mutableStateOf(false) }
    val busy = vm.chats[s.id]?.let { it.version >= 0 && it.transcript.busy } == true || s.id in vm.running || s.running
    val selected = vm.selected == s.id
    val shape = RoundedCornerShape(15.dp)
    val lockedGate = s.isLockedGate()
    val effectiveBackend = s.backend.takeIf { it in BACKENDS } ?: vm.chats[s.id]?.activeBackend() ?: vm.current?.effectiveBackend() ?: "claude"
    val engineTone = backendColor(p, effectiveBackend)

    Box(modifier) {
        Row(
            Modifier
                .fillMaxWidth()
                .clip(shape)
                .background(if (selected || busy) p.elevated else p.surface.copy(alpha = 0.80f))
                .specularBorder(
                    shape = shape,
                    topColor = when {
                        busy -> engineTone.copy(alpha = 0.55f)
                        selected -> p.accent.copy(alpha = 0.58f)
                        lockedGate -> p.amber.copy(alpha = 0.42f)
                        else -> p.specularTop.copy(alpha = 0.7f)
                    },
                    bottomColor = when {
                        busy -> engineTone.copy(alpha = 0.22f)
                        selected -> p.accent.copy(alpha = 0.25f)
                        lockedGate -> p.amber.copy(alpha = 0.22f)
                        else -> p.borderSubtle
                    },
                )
                .combinedClickable(onClick = { vm.open(s) }, onLongClick = { menu = true })
                .padding(start = if (selected || busy) 0.dp else 12.dp, top = 10.dp, bottom = 10.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (selected || busy) {
                Box(
                    Modifier
                        .padding(start = 5.dp, end = 9.dp)
                        .width(3.dp)
                        .height(30.dp)
                        .clip(CircleShape)
                        .background(if (busy) engineTone else p.accent),
                )
            }
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (s.locked) {
                        VectorGlyph(
                            glyph = if (s.unlocked) Glyph.Unlock else Glyph.Lock,
                            tint = if (s.unlocked) p.accent else p.amber,
                            size = 14.dp,
                        )
                    } else {
                        Box(Modifier.size(6.dp).clip(CircleShape).background(engineTone.copy(alpha = if (busy) 1f else 0.7f)))
                    }
                    Label(
                        text = if (lockedGate && s.title.isBlank()) "Защищённая сессия" else s.title,
                        size = 15.sp,
                        color = if (selected || busy) p.text else p.mix(p.text, p.muted, 0.12f),
                        weight = if (selected || busy) FontWeight.SemiBold else FontWeight.Medium,
                        maxLines = 1,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (s.backend in setOf("codex", "agy", "api") || busy) {
                        BackendBadge(effectiveBackend, compact = true)
                    }
                }
                Spacer(Modifier.height(2.dp))
                when {
                    lockedGate -> Label("Защищённая сессия · нужен пароль", 12.sp, p.amber, maxLines = 1)
                    busy -> Label("работает · ${folderName(s.cwd)}", 12.sp, engineTone, maxLines = 1)
                    else -> Label("${folderName(s.cwd)} · ${relative(s.modified)}", 12.sp, p.muted, maxLines = 1)
                }
            }
            if (busy) {
                NeuralWaveform(engineTone, active = true, width = 20.dp, height = 12.dp)
                Spacer(Modifier.width(4.dp))
            }
            IconButton(Icons.Default.MoreVert, "Меню") { menu = true }
        }

        DropdownMenu(menu, { menu = false }, Modifier.background(p.elevated)) {
            if (!s.locked) {
                DropdownMenuItem({ Label("Защитить паролем") }, { menu = false; lockAction() })
            } else {
                if (s.unlocked) {
                    DropdownMenuItem({ Label("Заблокировать сейчас", color = p.amber) }, { menu = false; vm.relockSession(s.id) })
                } else {
                    DropdownMenuItem({ Label("Разблокировать…", color = p.accent) }, { menu = false; vm.open(s) })
                }
                DropdownMenuItem({ Label("Сменить или снять пароль") }, { menu = false; lockAction() })
            }
            HorizontalDivider(color = p.line)
            if (!lockedGate) {
                DropdownMenuItem({ Label(if (s.pinned > 0) "Открепить" else "Закрепить сверху") }, { menu = false; vm.pin(s, s.pinned == 0.0) })
                DropdownMenuItem({ Label("Переименовать") }, { menu = false; rename() })
                DropdownMenuItem({ Label("Переместить в папку") }, { menu = false; move() })
                HorizontalDivider(color = p.line)
            }
            DropdownMenuItem(
                text = { Label("Удалить сессию", color = if (s.locked) p.muted else p.red) },
                onClick = {
                    menu = false
                    if (s.locked) {
                        vm.notice = "Сессия защищена паролем — сначала сними пароль, чтобы удалить её."
                    } else {
                        confirm = true
                    }
                },
            )
        }
    }

    if (confirm) {
        Sheet({ confirm = false }) {
            Label("Удалить «${s.title}»?", 18.sp, p.text, FontWeight.SemiBold)
            Spacer(Modifier.height(6.dp))
            Label("История этой сессии будет удалена с сервера.", 13.sp, p.muted)
            Spacer(Modifier.height(14.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Отмена") { confirm = false }
                Spacer(Modifier.width(6.dp))
                PrimaryButton("Удалить") { vm.delete(s); confirm = false }
            }
        }
    }
}

// --- Chat Screen -----------------------------------------------------------------------

sealed interface Item {
    data class Text(val index: Int, val message: Message) : Item
    data class Actions(val start: Int, val messages: List<Message>) : Item
}

/** Consecutive tool calls and their results collapse into one Agent Pipeline group. */
fun timeline(messages: List<Message>): List<Item> {
    val out = mutableListOf<Item>()
    var i = 0
    while (i < messages.size) {
        val m = messages[i]
        if (m.role == "tool" || m.role == "tool_result") {
            val start = i
            while (i < messages.size && (messages[i].role == "tool" || messages[i].role == "tool_result")) i++
            out.add(Item.Actions(start, messages.subList(start, i).toList()))
        } else {
            if (m.text.isNotBlank() || m.images > 0 || m.files.isNotEmpty()) out.add(Item.Text(i, m.copy()))
            i++
        }
    }
    return out
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ChatScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    val chat = vm.chat() ?: run { vm.screen = Screen.Sessions; return }
    BackHandler { vm.screen = Screen.Sessions }

    var lockDialog by remember { mutableStateOf(false) }
    var agentsSheet by remember { mutableStateOf(false) }
    var radarSheet by remember { mutableStateOf(false) }

    val version = chat.version
    val items = remember(version, chat.sid) { timeline(chat.transcript.messages) }
    val busy = chat.transcript.busy
    val lockedGate = chat.isLockedGate()
    val activeBackend = chat.activeBackend()
    val engineTone = backendColor(p, activeBackend)
    val list = rememberLazyListState()

    LaunchedEffect(items.size, version) {
        if (items.isNotEmpty() && !lockedGate) list.animateScrollToItem(items.size + 1)
    }

    Box(Modifier.fillMaxSize()) {
        AtmosphericHorizon(engineTone)
        Column(Modifier.fillMaxSize()) {
            val folder = vm.sessions.firstOrNull { it.id == chat.sid }?.folder?.let { f -> vm.folders.firstOrNull { it.id == f }?.name }
            TopBar(
                title = if (lockedGate) "Защищённая сессия" else chat.title,
                subtitle = listOfNotNull(folder, folderName(chat.cwd)).joinToString(" · "),
                link = vm.link,
                onBack = { vm.screen = Screen.Sessions },
            ) {
                BackendBadge(activeBackend, showDot = true) { agentsSheet = true }
                GlyphButton(
                    glyph = Glyph.Radar,
                    hint = "Радар агентов",
                    tint = if (busy || vm.running.isNotEmpty()) engineTone else p.muted,
                    highlighted = busy || vm.running.isNotEmpty(),
                ) {
                    radarSheet = true
                }
                GlyphButton(
                    glyph = if (chat.locked && !chat.unlocked) Glyph.Lock else if (chat.locked) Glyph.Unlock else Glyph.Lock,
                    hint = "Защита паролем",
                    tint = if (chat.locked) p.amber else p.muted,
                    highlighted = chat.locked,
                ) {
                    lockDialog = true
                }
            }
            if (vm.notice.isNotEmpty()) Notice(vm.notice) { vm.notice = "" }

            if (lockedGate) {
                LockedSessionCard(vm, chat, Modifier.weight(1f))
            } else {
                LazyColumn(
                    Modifier.weight(1f),
                    state = list,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp),
                ) {
                    if (chat.loading) {
                        item {
                            Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                                Dots(engineTone)
                                Spacer(Modifier.width(8.dp))
                                Label("Открываем разговор", 13.sp, p.muted)
                            }
                        }
                    }
                    if (items.isEmpty() && !chat.loading) {
                        item {
                            ChatEmptyState(vm, chat)
                        }
                    }
                    items(items, key = { when (it) { is Item.Text -> "m${it.index}"; is Item.Actions -> "a${it.start}" } }) { item ->
                        Box(Modifier.animateItem()) {
                            when (item) {
                                is Item.Text -> if (item.message.role == "user") {
                                    UserBubble(item.message)
                                } else {
                                    AssistantText(
                                        m = item.message,
                                        backend = activeBackend,
                                        streaming = busy && item.index == chat.transcript.messages.lastIndex,
                                    ) { path -> vm.download(path, chat.cwd) }
                                }
                                is Item.Actions -> ActionsGroup(
                                    item = item,
                                    running = busy && item.start + item.messages.size == chat.transcript.messages.size,
                                    backend = activeBackend,
                                )
                            }
                        }
                    }
                    item(key = "status") {
                        Column {
                            if (chat.transcript.error.isNotEmpty()) {
                                Box(
                                    Modifier
                                        .fillMaxWidth()
                                        .padding(vertical = 6.dp)
                                        .clip(RoundedCornerShape(12.dp))
                                        .background(p.red.copy(alpha = 0.12f))
                                        .border(1.dp, p.red.copy(alpha = 0.35f), RoundedCornerShape(12.dp))
                                        .padding(12.dp),
                                ) {
                                    Label(chat.transcript.error, 13.sp, p.text)
                                }
                            }
                            Spacer(Modifier.height(6.dp))
                        }
                    }
                }
                chat.permissions.firstOrNull()?.let { PermissionCard(it, vm) }
                MissingCliBanner(vm, activeBackend)
                AnimatedVisibility(
                    visible = busy,
                    enter = fadeIn(tween(180)) + expandVertically(tween(200)),
                    exit = fadeOut(tween(140)) + shrinkVertically(tween(180)),
                ) {
                    val telemetry = remember(version, busy, chat.transcript.status) {
                        analyzeTurnTelemetry(chat.transcript.messages, busy, chat.transcript.status)
                    }
                    AgentTelemetryPod(
                        backend = activeBackend,
                        telemetry = telemetry,
                        onOpenRadar = { radarSheet = true },
                    )
                }
                Composer(vm, chat, onOpenAgents = { agentsSheet = true })
            }
        }
    }

    if (lockDialog) {
        SessionLockDialog(vm, chat.sid, currentlyLocked = chat.locked, currentlyUnlocked = chat.unlocked) { lockDialog = false }
    }
    if (agentsSheet) {
        AgentsSheet(vm) { agentsSheet = false }
    }
    if (radarSheet) {
        AgentRadarSheet(vm) { radarSheet = false }
    }
}

@Composable
fun LockedSessionCard(vm: RelayViewModel, chat: Chat, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    var password by remember(chat.sid) { mutableStateOf("") }
    val shape = RoundedCornerShape(22.dp)
    Box(modifier.fillMaxWidth().padding(20.dp), contentAlignment = Alignment.Center) {
        Column(
            Modifier
                .fillMaxWidth()
                .widthIn(max = 420.dp)
                .clip(shape)
                .background(p.surface)
                .specularBorder(shape, p.amber.copy(alpha = 0.52f), p.amber.copy(alpha = 0.25f))
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Box(
                Modifier
                    .size(58.dp)
                    .clip(CircleShape)
                    .background(p.amber.copy(alpha = 0.14f))
                    .border(1.dp, p.amber.copy(alpha = 0.35f), CircleShape),
                contentAlignment = Alignment.Center,
            ) {
                VectorGlyph(Glyph.ShieldLock, p.amber, size = 30.dp)
            }
            Spacer(Modifier.height(16.dp))
            Label("Сессия защищена паролем", 19.sp, p.text, FontWeight.SemiBold)
            Spacer(Modifier.height(6.dp))
            Label(
                "История скрыта на сервере. Введи пароль сессии, чтобы открыть сообщения и продолжить разговор.",
                13.sp,
                p.muted,
            )
            Spacer(Modifier.height(16.dp))
            Field(
                value = password,
                onChange = { password = it; chat.lockError = "" },
                hint = "Пароль сессии",
                password = true,
            )
            if (chat.lockError.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Label(chat.lockError, 12.5.sp, p.red)
            }
            Spacer(Modifier.height(14.dp))
            PrimaryButton(
                text = if (chat.unlocking) "Проверяем…" else "Разблокировать",
                enabled = password.isNotEmpty() && !chat.unlocking,
                modifier = Modifier.fillMaxWidth(),
            ) {
                vm.unlockSession(chat.sid, password)
            }
        }
    }
}

private data class StarterMission(val title: String, val subtitle: String, val prompt: String, val glyph: Glyph, val phase: ToolPhase)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ChatEmptyState(vm: RelayViewModel, chat: Chat) {
    val p = LocalPalette.current
    val active = chat.activeBackend()
    val activeTone = backendColor(p, active)
    val crestShape = RoundedCornerShape(22.dp)

    Column(
        Modifier.fillMaxWidth().padding(top = 24.dp, bottom = 14.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .size(68.dp)
                .clip(crestShape)
                .background(
                    Brush.radialGradient(
                        listOf(activeTone.copy(alpha = if (p.light) 0.18f else 0.24f), p.surface)
                    )
                )
                .specularBorder(crestShape, activeTone.copy(alpha = 0.55f), activeTone.copy(alpha = 0.20f)),
            contentAlignment = Alignment.Center,
        ) {
            OrbitalEmblem(activeTone, active = true, size = 44.dp)
        }
        Spacer(Modifier.height(14.dp))
        Label("С чего начнём?", 23.sp, p.text, FontWeight.SemiBold)
        Spacer(Modifier.height(6.dp))
        Row(
            Modifier
                .clip(RoundedCornerShape(9.dp))
                .background(p.side)
                .specularBorder(RoundedCornerShape(9.dp), p.specularTop, p.borderSubtle)
                .padding(horizontal = 10.dp, vertical = 4.5.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            VectorGlyph(Glyph.Folder, p.muted, size = 13.dp)
            Label("Папка ${folderName(chat.cwd)}", 12.5.sp, p.muted, maxLines = 1)
        }
        Spacer(Modifier.height(16.dp))
        Row(
            Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val engineChoices = listOf(
                "claude" to "Claude Code",
                "codex" to "Codex",
                "agy" to "Antigravity",
                "api" to "API",
            )
            for ((b, title) in engineChoices) {
                val chosen = active == b
                val tone = backendColor(p, b)
                val pillShape = RoundedCornerShape(999.dp)
                Row(
                    Modifier
                        .clip(pillShape)
                        .background(if (chosen) tone.copy(alpha = if (p.light) 0.15f else 0.20f) else p.surface)
                        .specularBorder(
                            pillShape,
                            if (chosen) tone.copy(alpha = 0.72f) else p.specularTop,
                            if (chosen) tone.copy(alpha = 0.35f) else p.borderSubtle,
                        )
                        .clickable { vm.setBackend(chat, b) }
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Box(Modifier.size(6.5.dp).clip(CircleShape).background(if (chosen) tone else tone.copy(alpha = 0.55f)))
                    Label(title, 12.sp, if (chosen) tone else p.muted, FontWeight.SemiBold, maxLines = 1)
                }
            }
        }
        Spacer(Modifier.height(18.dp))
        val missions = listOf(
            StarterMission("Архитектура", "Карта модулей и связей", "Разобрать архитектуру проекта и ключевые модули", Glyph.Layers, ToolPhase.Research),
            StarterMission("Найти и починить", "Диагностика и патч", "Найти ошибку и предложить чистый фикс", Glyph.Bolt, ToolPhase.Edit),
            StarterMission("Сборка и тесты", "Git status и прогон", "Проверить git status и запустить тесты", Glyph.Terminal, ToolPhase.Terminal),
            StarterMission("Рой субагентов", "Параллельный аудит", "Разделить задачу между субагентами и свести отчёт", Glyph.Orbit, ToolPhase.Subagent),
        )
        Column(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            for (row in missions.chunked(2)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (m in row) {
                        val mColor = phaseColor(m.phase, p.light)
                        val cardShape = RoundedCornerShape(15.dp)
                        Column(
                            Modifier
                                .weight(1f)
                                .clip(cardShape)
                                .background(p.surface.copy(alpha = 0.88f))
                                .specularBorder(cardShape, mColor.copy(alpha = 0.38f), p.borderSubtle)
                                .clickable { chat.draft = m.prompt }
                                .padding(horizontal = 12.dp, vertical = 11.dp),
                            verticalArrangement = Arrangement.spacedBy(5.dp),
                        ) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Box(
                                    Modifier
                                        .size(24.dp)
                                        .clip(RoundedCornerShape(7.dp))
                                        .background(mColor.copy(alpha = 0.15f)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    VectorGlyph(m.glyph, mColor, size = 13.5.dp)
                                }
                                Label(m.title, 13.sp, p.text, FontWeight.SemiBold, maxLines = 1)
                            }
                            Label(m.subtitle, 11.5.sp, p.muted, maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun MissingCliBanner(vm: RelayViewModel, activeBackend: String) {
    if (vm.link != Link.Online || !vm.backendsKnown || activeBackend !in CLI_AGENTS) return
    val status = vm.backendsStatus[activeBackend] ?: return
    if (status.installed) return
    val p = LocalPalette.current
    val installing = vm.installingAgents[activeBackend] == true
    val msg = vm.agentMessages[activeBackend].orEmpty()

    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 10.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(p.elevated)
            .specularBorder(RoundedCornerShape(14.dp), p.amber.copy(alpha = 0.5f), p.amber.copy(alpha = 0.25f))
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        VectorGlyph(Glyph.Download, p.amber, size = 20.dp)
        Column(Modifier.weight(1f)) {
            Label("На сервере не установлен ${backendLabel(activeBackend)}", 13.sp, p.text, FontWeight.SemiBold, maxLines = 1)
            Label(
                if (installing) msg.ifBlank { "Устанавливаем на сервере…" }
                else "Установи официальный CLI в 1 нажатие прямо по SSH.",
                11.5.sp,
                p.muted,
                maxLines = 2,
            )
        }
        if (installing) {
            Box(
                Modifier
                    .clip(RoundedCornerShape(10.dp))
                    .background(p.amber.copy(alpha = 0.16f))
                    .border(1.dp, p.amber.copy(alpha = 0.35f), RoundedCornerShape(10.dp))
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                Label("Установка…", 12.5.sp, p.amber, FontWeight.SemiBold)
            }
        } else {
            PrimaryButton("Установить ${backendShortLabel(activeBackend)}") {
                vm.installAgent(activeBackend, "install")
            }
        }
    }
}

@Composable
fun UserBubble(m: Message) {
    val p = LocalPalette.current
    val bubbleShape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp, bottomEnd = 6.dp, bottomStart = 20.dp)
    Box(Modifier.fillMaxWidth().padding(vertical = 6.dp), contentAlignment = Alignment.CenterEnd) {
        Column(
            Modifier
                .widthIn(max = 324.dp)
                .clip(bubbleShape)
                .background(p.mix(p.surface, p.accent, 0.13f))
                .specularBorder(bubbleShape, p.mix(p.line, p.accent, 0.45f), p.mix(p.line, p.accent, 0.22f))
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            if (m.text.isNotBlank()) Text(m.text, color = p.text, fontSize = 15.sp)
            if (m.images > 0) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    VectorGlyph(Glyph.Image, p.accent, size = 13.dp)
                    Label("Изображений: ${m.images}", 12.sp, p.muted)
                }
            }
            for (f in m.files) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    VectorGlyph(Glyph.Paperclip, p.accent, size = 13.dp)
                    Label(f, 12.sp, p.muted, maxLines = 1)
                }
            }
        }
    }
}

fun engineSignatureTitle(backend: String): String = when (backend) {
    "codex" -> "OpenAI Codex"
    "agy" -> "Antigravity"
    "api" -> "API"
    else -> "Claude Code"
}

@Composable
fun AssistantText(m: Message, backend: String = "claude", streaming: Boolean = false, onFile: ((String) -> Unit)? = null) {
    val p = LocalPalette.current
    val engineTone = backendColor(p, backend)
    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Row(
            Modifier.padding(bottom = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Box(Modifier.size(6.dp).clip(CircleShape).background(engineTone))
            VectorGlyph(
                glyph = when (backend) {
                    "codex" -> Glyph.Bolt
                    "agy" -> Glyph.Orbit
                    "api" -> Glyph.Layers
                    else -> Glyph.Spark
                },
                tint = engineTone,
                size = 12.5.dp,
            )
            Label(engineSignatureTitle(backend), 11.5.sp, engineTone, FontWeight.SemiBold, maxLines = 1)
            if (streaming) {
                NeuralWaveform(engineTone, active = true, width = 16.dp, height = 10.dp)
            }
        }
        MarkdownText(m.text, p, Modifier.fillMaxWidth(), streaming, onFile)
        val files = remember(m.text, streaming) { if (streaming) emptyList() else mentionedFiles(m.text) }
        if (onFile != null && files.isNotEmpty()) {
            Column(Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (path in files) {
                    val dlShape = RoundedCornerShape(12.dp)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(dlShape)
                            .background(p.side)
                            .specularBorder(dlShape, p.specularTop, p.line)
                            .clickable { onFile(path) }
                            .padding(horizontal = 12.dp, vertical = 9.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        VectorGlyph(Glyph.Download, p.accent, size = 18.dp)
                        Column(Modifier.weight(1f)) {
                            Label(path.substringAfterLast('/'), 13.5.sp, p.text, FontWeight.Medium, maxLines = 1)
                            Label(path.substringBeforeLast('/', ""), 11.sp, p.muted, maxLines = 1)
                        }
                        Label("Скачать", 13.sp, p.accent, FontWeight.SemiBold)
                    }
                }
            }
        }
    }
}

fun stepsWord(count: Int): String = when {
    count % 10 == 1 && count % 100 != 11 -> "шаг"
    count % 10 in 2..4 && count % 100 !in 12..14 -> "шага"
    else -> "шагов"
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ActionsGroup(item: Item.Actions, running: Boolean, backend: String = "claude") {
    val p = LocalPalette.current
    var open by rememberSaveable(item.start) { mutableStateOf(running) }
    LaunchedEffect(running) { if (running) open = true }
    val calls = item.messages.filter { it.role == "tool" }
    val resultsById = remember(item.messages) {
        item.messages.filter { it.role == "tool_result" && it.toolId.isNotEmpty() }.associateBy { it.toolId }
    }
    val telemetry = remember(item.messages, running) { analyzeTurnTelemetry(item.messages, running) }
    val failed = telemetry.failedActions > 0
    val engineTone = backendColor(p, backend)
    val badgeTone = when {
        failed -> p.red
        running -> engineTone
        else -> p.accent
    }
    val turn by animateFloatAsState(if (open) 90f else 0f, tween(200), label = "turn")
    val shape = RoundedCornerShape(16.dp)

    Column(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp)
            .clip(shape)
            .background(
                Brush.verticalGradient(
                    listOf(p.mix(p.surface, badgeTone, if (p.light) 0.05f else 0.07f), p.surface)
                )
            )
            .specularBorder(
                shape = shape,
                topColor = when {
                    failed -> p.red.copy(alpha = 0.58f)
                    running -> engineTone.copy(alpha = 0.55f)
                    else -> p.specularTop
                },
                bottomColor = if (failed) p.red.copy(alpha = 0.28f) else p.line,
            )
            .animateContentSize(tween(220)),
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .clickable { open = !open }
                .padding(horizontal = 12.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                OrbitalEmblem(badgeTone, active = running, size = 24.dp)
                Spacer(Modifier.width(9.dp))
                val count = calls.size
                Column(Modifier.weight(1f)) {
                    if (running) {
                        Label(
                            "Выполняется · ${telemetry.activeHeadline}",
                            13.sp,
                            p.text,
                            FontWeight.SemiBold,
                            maxLines = 1,
                        )
                        Label(
                            "Конвейер агента · ${telemetry.doneActions}/$count ${stepsWord(count)}",
                            11.sp,
                            p.muted,
                            maxLines = 1,
                        )
                    } else {
                        Label(
                            "Конвейер агента · $count ${stepsWord(count)}" + if (failed) " · есть ошибки" else "",
                            13.sp,
                            if (failed) p.red else p.text,
                            FontWeight.SemiBold,
                            maxLines = 1,
                        )
                    }
                }
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = p.muted, modifier = Modifier.size(20.dp).rotate(turn))
            }

            if (calls.isNotEmpty()) {
                // Proportional segmented color-coded phase bar
                Row(
                    Modifier
                        .fillMaxWidth()
                        .height(4.5.dp)
                        .clip(CircleShape)
                        .background(p.base),
                    horizontalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    for (call in calls) {
                        val res = resultsById[call.toolId]
                        val phase = classifyToolPhase(call.tool)
                        val segTone = when {
                            res?.failed == true -> p.red
                            res == null && running -> phaseColor(phase, p.light).copy(alpha = 0.55f)
                            else -> phaseColor(phase, p.light)
                        }
                        Box(Modifier.weight(1f).fillMaxHeight().clip(CircleShape).background(segTone))
                    }
                }

                // Compact phase counter pills
                val pills = buildList {
                    if (telemetry.reads > 0) add(ToolPhase.Research to telemetry.reads)
                    if (telemetry.delegations > 0) add(ToolPhase.Subagent to telemetry.delegations)
                    if (telemetry.edits > 0) add(ToolPhase.Edit to telemetry.edits)
                    if (telemetry.commands > 0) add(ToolPhase.Terminal to telemetry.commands)
                    if (telemetry.desktopOps > 0) add(ToolPhase.Desktop to telemetry.desktopOps)
                }
                if (pills.isNotEmpty()) {
                    FlowRow(
                        Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(5.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        for ((phase, cnt) in pills) {
                            val cTone = phaseColor(phase, p.light)
                            Row(
                                Modifier
                                    .clip(RoundedCornerShape(6.dp))
                                    .background(cTone.copy(alpha = if (p.light) 0.11f else 0.14f))
                                    .padding(horizontal = 6.dp, vertical = 2.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                            ) {
                                Box(Modifier.size(5.dp).clip(CircleShape).background(cTone))
                                Label("${phase.label} $cnt", 9.5.sp, cTone, FontWeight.SemiBold, maxLines = 1)
                            }
                        }
                    }
                }
            }
        }

        AnimatedVisibility(
            open,
            enter = expandVertically(tween(220)) + fadeIn(tween(220)),
            exit = shrinkVertically(tween(180)) + fadeOut(tween(140)),
        ) {
            Column(Modifier.padding(start = 10.dp, end = 12.dp, bottom = 11.dp)) {
                for ((idx, call) in calls.withIndex()) {
                    val result = resultsById[call.toolId]
                    val stepState = when {
                        result == null && running -> 0
                        result?.failed == true -> 2
                        else -> 1
                    }
                    val phase = classifyToolPhase(call.tool)
                    val nodeColor = when (stepState) {
                        2 -> p.red
                        0 -> phaseColor(phase, p.light)
                        else -> phaseColor(phase, p.light)
                    }
                    val spineColor = p.line.copy(alpha = 0.85f)
                    val isFirst = idx == 0
                    val isLast = idx == calls.lastIndex

                    Box(
                        Modifier
                            .fillMaxWidth()
                            .drawBehind {
                                val x = 10.dp.toPx()
                                val nodeY = 20.dp.toPx()
                                val r = 5.dp.toPx()
                                val strokeW = 1.8.dp.toPx()
                                if (!isFirst) {
                                    drawLine(spineColor, Offset(x, 0f), Offset(x, nodeY - r - 2.dp.toPx()), strokeW)
                                }
                                if (!isLast) {
                                    drawLine(spineColor, Offset(x, nodeY + r + 2.dp.toPx()), Offset(x, size.height), strokeW)
                                }
                                drawCircle(nodeColor.copy(alpha = 0.22f), r * 1.7f, Offset(x, nodeY))
                                drawCircle(p.surface, r, Offset(x, nodeY))
                                drawCircle(nodeColor, r, Offset(x, nodeY), style = Stroke(width = 1.7.dp.toPx()))
                                if (stepState == 1) {
                                    drawCircle(nodeColor, r * 0.45f, Offset(x, nodeY))
                                } else if (stepState == 2) {
                                    drawCircle(p.red, r * 0.5f, Offset(x, nodeY))
                                }
                            }
                            .padding(start = 24.dp, bottom = if (isLast) 0.dp else 7.dp),
                    ) {
                        ToolCall(call, result, running)
                    }
                }
            }
        }
    }
}

@Composable
fun PhaseBadge(phase: ToolPhase, failed: Boolean = false) {
    val p = LocalPalette.current
    val tone = if (failed) p.red else phaseColor(phase, p.light)
    Box(
        Modifier
            .clip(RoundedCornerShape(5.dp))
            .background(tone.copy(alpha = if (p.light) 0.13f else 0.17f))
            .border(1.dp, tone.copy(alpha = 0.34f), RoundedCornerShape(5.dp))
            .padding(horizontal = 5.5.dp, vertical = 1.5.dp),
    ) {
        Label(phase.label, 9.5.sp, tone, FontWeight.Bold, maxLines = 1)
    }
}

@Composable
fun ToolCall(call: Message, result: Message?, running: Boolean = false) {
    val p = LocalPalette.current
    var full by remember { mutableStateOf(false) }
    val appear = remember { MutableTransitionState(false).apply { targetState = true } }
    val phase = classifyToolPhase(call.tool)
    val pTone = phaseColor(phase, p.light)
    val state = when {
        result == null && running -> 0
        result?.failed == true -> 2
        else -> 1
    }
    val shape = RoundedCornerShape(12.dp)

    AnimatedVisibility(appear, enter = fadeIn(tween(200)) + slideInVertically(tween(200)) { it / 4 }) {
        if (phase == ToolPhase.Subagent) {
            val trace = extractSubagentTrace(call, result, running)
            val violet = phaseColor(ToolPhase.Subagent, p.light)
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(shape)
                    .background(
                        Brush.horizontalGradient(
                            listOf(violet.copy(alpha = if (p.light) 0.10f else 0.16f), p.base)
                        )
                    )
                    .specularBorder(
                        shape = shape,
                        topColor = if (result?.failed == true) p.red.copy(alpha = 0.55f) else violet.copy(alpha = 0.58f),
                        bottomColor = violet.copy(alpha = 0.25f),
                    )
                    .clickable { full = !full }
                    .padding(10.dp)
                    .animateContentSize(tween(200)),
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                    VectorGlyph(Glyph.Orbit, violet, size = 15.dp)
                    PhaseBadge(ToolPhase.Subagent, failed = result?.failed == true)
                    Label(trace.role, 13.sp, p.text, FontWeight.SemiBold, maxLines = 1, modifier = Modifier.weight(1f))
                    if (trace.model.isNotEmpty()) {
                        Box(
                            Modifier
                                .clip(RoundedCornerShape(5.dp))
                                .background(violet.copy(alpha = 0.15f))
                                .padding(horizontal = 5.dp, vertical = 1.dp),
                        ) {
                            Label(trace.model, 9.5.sp, violet, FontWeight.Medium, maxLines = 1)
                        }
                    }
                    StatusMark(state, accentOverride = violet)
                }
                if (trace.summary.isNotEmpty()) {
                    Label(trace.summary, 12.sp, p.mix(p.text, p.muted, 0.22f), maxLines = if (full) 12 else 2)
                }
                if (result == null && running) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        NeuralWaveform(violet, active = true, width = 16.dp, height = 10.dp)
                        Label("субагент работает…", 11.5.sp, violet, FontWeight.Medium)
                    }
                } else if (result != null && result.text.isNotBlank()) {
                    val lines = result.text.trimEnd().lines().filter { it.isNotBlank() }
                    val shown = if (full) lines.takeLast(80) else lines.takeLast(2)
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(7.dp))
                            .background(if (result.failed) p.red.copy(alpha = 0.08f) else p.side.copy(alpha = 0.85f))
                            .padding(horizontal = 8.dp, vertical = 5.dp),
                    ) {
                        Text(
                            shown.joinToString("\n"),
                            color = if (result.failed) p.red else p.mix(p.text, p.muted, 0.25f),
                            fontSize = 11.5.sp,
                            fontFamily = FontFamily.Monospace,
                            maxLines = if (full) 40 else 3,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        } else {
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(shape)
                    .background(p.base)
                    .specularBorder(
                        shape = shape,
                        topColor = when {
                            result?.failed == true -> p.red.copy(alpha = 0.50f)
                            result == null && running -> pTone.copy(alpha = 0.52f)
                            else -> p.specularTop.copy(alpha = 0.65f)
                        },
                        bottomColor = when {
                            result?.failed == true -> p.red.copy(alpha = 0.28f)
                            else -> p.line
                        },
                    )
                    .clickable { full = !full }
                    .padding(10.dp)
                    .animateContentSize(tween(200)),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                    PhaseBadge(phase, failed = result?.failed == true)
                    val heading = call.input.optString("description").trim()
                        .takeIf { it.isNotEmpty() && !call.tool.startsWith("mcp__relay_cluster__") }
                        ?: toolName(call.tool)
                    Label(heading, 13.sp, if (result?.failed == true) p.red else p.text, FontWeight.Medium, maxLines = 1, modifier = Modifier.weight(1f))
                    StatusMark(state, accentOverride = pTone)
                }
                val line = commandLine(call)
                if (line.isNotEmpty()) {
                    Text(
                        line,
                        color = p.mix(p.text, p.muted, 0.38f),
                        fontSize = 11.5.sp,
                        fontFamily = FontFamily.Monospace,
                        maxLines = if (full) 20 else 2,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
                Column(Modifier.padding(top = 5.dp)) {
                    if (call.tool in setOf("Edit", "MultiEdit", "edit_file")) {
                        val pairs = if (call.tool == "MultiEdit") {
                            call.input.optJSONArray("edits")?.let { a ->
                                (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map { it.optString("old_string") to it.optString("new_string") }
                            }.orEmpty()
                        } else {
                            val old = call.input.optString("old_string").ifEmpty { call.input.optString("Target") }
                            val new = call.input.optString("new_string").ifEmpty { call.input.optString("Replacement") }
                            listOf(old to new)
                        }
                        val lines = pairs.flatMap { (old, new) ->
                            old.lines().filter { it.isNotBlank() }.take(if (full) 40 else 3).map { "−" to it } +
                                new.lines().filter { it.isNotBlank() }.take(if (full) 40 else 3).map { "+" to it }
                        }
                        for ((sign, text) in lines.take(if (full) 200 else 8)) {
                            val color = if (sign == "+") p.green else p.red
                            Text(
                                "$sign ${text.trimEnd()}",
                                color = color,
                                fontSize = 11.5.sp,
                                fontFamily = FontFamily.Monospace,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(vertical = 1.dp)
                                    .clip(RoundedCornerShape(4.dp))
                                    .background(color.copy(alpha = 0.11f))
                                    .padding(horizontal = 6.dp, vertical = 1.dp),
                            )
                        }
                    } else if (result == null && running) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            NeuralWaveform(pTone, active = true, width = 16.dp, height = 10.dp)
                            Label("выполняется…", 11.5.sp, pTone, FontWeight.Medium)
                        }
                    } else if (result != null && result.text.isNotBlank() && call.tool !in setOf("Read", "read_file")) {
                        val all = result.text.trimEnd().lines().filter { it.isNotBlank() }
                        val shown = if (full) all.takeLast(400) else all.takeLast(4)
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(7.dp))
                                .background(if (result.failed) p.red.copy(alpha = 0.08f) else p.side)
                                .padding(horizontal = 9.dp, vertical = 6.dp),
                        ) {
                            if (all.size > shown.size) Label("… ещё ${lines(all.size - shown.size)} выше · нажми, чтобы раскрыть", 10.5.sp, p.muted)
                            Text(
                                shown.joinToString("\n"),
                                color = if (result.failed) p.red else p.mix(p.text, p.muted, 0.3f),
                                fontSize = 11.5.sp,
                                fontFamily = FontFamily.Monospace,
                            )
                        }
                    }
                }
            }
        }
    }
}

/** 0 — orbital active arc (static/finite so Robolectric never hangs!), 1 — green tick, 2 — red cross. */
@Composable
fun StatusMark(state: Int, accentOverride: Color? = null) {
    val p = LocalPalette.current
    val targetColor = when (state) {
        0 -> accentOverride ?: p.accent
        1 -> p.green
        else -> p.red
    }
    val color by animateColorAsState(targetColor, tween(220), label = "statusColor")
    Canvas(Modifier.size(18.dp)) {
        val r = size.minDimension / 2f - 1.5f
        val stroke = Stroke(width = 1.7.dp.toPx(), cap = StrokeCap.Round)
        when (state) {
            0 -> {
                drawCircle(color.copy(alpha = 0.18f), r)
                drawArc(color, -55f, 220f, false, topLeft = center - Offset(r, r), size = Size(r * 2, r * 2), style = stroke)
                drawCircle(color, r * 0.32f, center)
            }
            1 -> {
                drawCircle(color.copy(alpha = 0.18f), r)
                val path = Path().apply {
                    moveTo(center.x - r * 0.42f, center.y)
                    lineTo(center.x - r * 0.1f, center.y + r * 0.33f)
                    lineTo(center.x + r * 0.45f, center.y - r * 0.3f)
                }
                drawPath(path, color, style = stroke)
            }
            else -> {
                drawCircle(color.copy(alpha = 0.18f), r)
                val d = r * 0.36f
                drawLine(color, center - Offset(d, d), center + Offset(d, d), stroke.width, StrokeCap.Round)
                drawLine(color, center + Offset(d, -d), center + Offset(-d, d), stroke.width, StrokeCap.Round)
            }
        }
    }
}

/** Live Agent Neural Telemetry Pod shown above the Composer whenever `busy` is true. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AgentTelemetryPod(
    backend: String,
    telemetry: TurnTelemetry,
    onOpenRadar: () -> Unit,
) {
    val p = LocalPalette.current
    val engineTone = backendColor(p, backend)
    val podShape = RoundedCornerShape(18.dp)
    val violet = phaseColor(ToolPhase.Subagent, p.light)

    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 10.dp)
            .padding(top = 6.dp)
            .clip(podShape)
            .background(
                Brush.horizontalGradient(
                    listOf(
                        p.mix(p.elevated, engineTone, if (p.light) 0.09f else 0.14f),
                        p.elevated,
                    )
                )
            )
            .specularBorder(podShape, engineTone.copy(alpha = 0.55f), p.line)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            OrbitalEmblem(engineTone, active = true, size = 32.dp)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Label(engineSignatureTitle(backend), 11.sp, engineTone, FontWeight.Bold, maxLines = 1)
                    NeuralWaveform(engineTone, active = true, width = 20.dp, height = 11.dp)
                    if (telemetry.totalActions > 0) {
                        PhaseBadge(telemetry.activePhase)
                        Label("${telemetry.doneActions}/${telemetry.totalActions}", 11.sp, p.muted, FontWeight.SemiBold, maxLines = 1)
                    } else {
                        Box(
                            Modifier
                                .clip(RoundedCornerShape(5.dp))
                                .background(engineTone.copy(alpha = 0.16f))
                                .padding(horizontal = 5.dp, vertical = 1.dp),
                        ) {
                            Label("LIVE", 9.5.sp, engineTone, FontWeight.Bold)
                        }
                    }
                }
                Label(telemetry.activeHeadline, 13.sp, p.text, FontWeight.SemiBold, maxLines = 1)
                if (telemetry.activeTarget.isNotEmpty()) {
                    Text(
                        telemetry.activeTarget,
                        color = p.muted,
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            GlyphButton(Glyph.Radar, "Радар агентов", tint = engineTone, highlighted = true, onClick = onOpenRadar)
        }

        if (telemetry.subagents.isNotEmpty()) {
            FlowRow(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                for (sub in telemetry.subagents.take(4)) {
                    Row(
                        Modifier
                            .clip(RoundedCornerShape(999.dp))
                            .background(violet.copy(alpha = if (p.light) 0.12f else 0.18f))
                            .border(1.dp, violet.copy(alpha = 0.38f), RoundedCornerShape(999.dp))
                            .padding(horizontal = 8.dp, vertical = 3.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        VectorGlyph(Glyph.Orbit, violet, size = 11.dp)
                        Label(sub.role, 10.5.sp, violet, FontWeight.SemiBold, maxLines = 1)
                    }
                }
            }
        }
    }
}

fun lines(n: Int) = "$n " + when { n % 10 == 1 && n % 100 != 11 -> "строка"; n % 10 in 2..4 && n % 100 !in 12..14 -> "строки"; else -> "строк" }

fun commandLine(call: Message): String {
    val i = call.input
    return when (call.tool) {
        "Bash", "shell", "command_execution", "run_command", "mcp__relay_desktop__run_local_command" ->
            "$ " + i.optString("command").ifEmpty { i.optString("CommandLine") }.trim().lines().joinToString(" ⏎ ")
        "Grep", "grep_search" -> "«${i.optString("pattern")}»" + i.optString("path").let { if (it.isEmpty()) "" else " в $it" }
        "Read", "Write", "Edit", "MultiEdit", "read_file", "write_file", "edit_file" ->
            i.optString("file_path").ifEmpty { i.optString("path") }.ifEmpty { i.optString("Path") }
        "Glob", "find_by_name" -> i.optString("pattern").ifEmpty { i.optString("Pattern") }
        "SlashCommand" -> i.optString("command")
        "WebSearch", "web_search" -> i.optString("query")
        "WebFetch" -> i.optString("url")
        else -> if (call.tool.startsWith("mcp__relay_cluster__")) "" else toolSummary(call.tool, i)
    }
}

@Composable
fun PermissionCard(permission: Permission, vm: RelayViewModel) {
    val p = LocalPalette.current
    val shape = RoundedCornerShape(16.dp)
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .clip(shape)
            .background(p.elevated)
            .specularBorder(shape, p.accent.copy(alpha = 0.65f), p.accent.copy(alpha = 0.30f))
            .padding(14.dp),
    ) {
        Label("Агент запрашивает разрешение", 12.5.sp, p.accent, FontWeight.Medium)
        Spacer(Modifier.height(4.dp))
        Label(toolName(permission.tool), 15.sp, p.text, FontWeight.SemiBold)
        val summary = toolSummary(permission.tool, permission.input)
        if (summary.isNotEmpty()) {
            Spacer(Modifier.height(6.dp))
            Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(9.dp)).background(p.base).border(1.dp, p.line, RoundedCornerShape(9.dp)).padding(10.dp)) {
                Text(summary, color = p.text, fontSize = 12.sp, fontFamily = FontFamily.Monospace, maxLines = 6, overflow = TextOverflow.Ellipsis)
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            GhostButton("Отклонить", p.red) { vm.permit(permission, false) }
            Spacer(Modifier.width(6.dp))
            PrimaryButton("Разрешить") { vm.permit(permission, true) }
        }
    }
}

@Composable
fun TransferCard(vm: RelayViewModel, t: Transfer) {
    val p = LocalPalette.current
    val progress by animateFloatAsState(if (t.size > 0) (t.received.toFloat() / t.size).coerceIn(0f, 1f) else 0f, tween(200), label = "progress")
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp)
            .padding(top = 6.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(p.side)
            .border(1.dp, if (t.error.isNotEmpty()) p.red.copy(alpha = 0.5f) else p.line, RoundedCornerShape(12.dp))
            .padding(horizontal = 12.dp, vertical = 9.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            StatusMark(when { t.error.isNotEmpty() -> 2; t.done -> 1; else -> 0 })
            Spacer(Modifier.width(9.dp))
            Column(Modifier.weight(1f)) {
                Label(t.name, 13.5.sp, p.text, FontWeight.Medium, maxLines = 1)
                Label(
                    when {
                        t.error.isNotEmpty() -> t.error
                        t.done -> "В «Загрузки/Relay» · ${megabytes(t.size)}"
                        t.size > 0 -> "${megabytes(t.received)} из ${megabytes(t.size)}"
                        else -> "Ищем файл на сервере…"
                    },
                    12.sp,
                    if (t.error.isNotEmpty()) p.red else p.muted,
                    maxLines = 2,
                )
            }
            if (t.done) {
                GhostButton("Открыть", p.accent) { vm.openTransfer(t) }
                IconButton(Icons.Default.Share, "Поделиться") { vm.shareTransfer(t) }
            }
            IconButton(Icons.Default.Close, if (t.done || t.error.isNotEmpty()) "Убрать" else "Отменить") { vm.dismiss(t) }
        }
        if (!t.done && t.error.isEmpty() && t.size > 0) {
            Box(Modifier.padding(top = 6.dp).fillMaxWidth().height(3.dp).clip(RoundedCornerShape(2.dp)).background(p.line)) {
                Box(Modifier.fillMaxWidth(progress).fillMaxHeight().background(p.accent))
            }
        }
    }
}

fun megabytes(bytes: Long) = if (bytes < 1024 * 1024) "${(bytes + 1023) / 1024} КБ" else "%.1f МБ".format(bytes / 1048576.0)

@Composable
fun Composer(vm: RelayViewModel, chat: Chat, onOpenAgents: () -> Unit = {}) {
    val p = LocalPalette.current
    val busy = chat.version >= 0 && chat.transcript.busy
    val activeBackend = chat.activeBackend()
    val engineTone = backendColor(p, activeBackend)
    for (t in vm.transfers.take(3)) key(t.id) { TransferCard(vm, t) }
    val online = vm.link == Link.Online
    var apiSettings by remember { mutableStateOf(false) }
    if (apiSettings) vm.current?.let { server -> ApiDialog(vm, server) { apiSettings = false } }

    val suggestions = slashMatches(chat.draft, vm.commandsFor(activeBackend))
    if (chat.queued.isNotEmpty()) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp)
                .padding(top = 6.dp)
                .clip(RoundedCornerShape(12.dp))
                .background(p.side)
                .border(1.dp, p.line, RoundedCornerShape(12.dp))
                .padding(10.dp),
        ) {
            Label("В очереди: ${chat.queued.size} — ответ придёт по порядку, даже если закрыть приложение", 12.sp, p.muted)
            for ((i, q) in chat.queued.withIndex()) Label("${i + 1}  $q", 13.sp, p.text, maxLines = 1, modifier = Modifier.padding(top = 4.dp))
        }
    }
    AnimatedVisibility(suggestions.isNotEmpty()) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp)
                .padding(top = 6.dp)
                .heightIn(max = 260.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(p.elevated)
                .specularBorder(RoundedCornerShape(14.dp), p.specularTop, p.line)
                .verticalScroll(rememberScrollState())
                .padding(4.dp),
        ) {
            for (c in suggestions.take(12)) {
                Column(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(9.dp))
                        .clickable { chat.draft = "/${c.name} " }
                        .padding(horizontal = 10.dp, vertical = 7.dp),
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Label("/${c.name}", 14.sp, p.accent, FontWeight.SemiBold)
                        if (c.hint.isNotEmpty()) Label("  ${c.hint}", 12.sp, p.muted, maxLines = 1)
                    }
                    if (c.description.isNotEmpty()) Label(c.description, 12.sp, p.muted, maxLines = 2)
                }
            }
            Label("/model, /mode и /stop срабатывают сразу, даже во время ответа", 11.sp, p.muted, modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp))
        }
    }

    val picker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.GetMultipleContents()
    ) { uris ->
        if (uris.isNotEmpty()) vm.pick(uris)
    }

    val containerShape = RoundedCornerShape(22.dp)
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 10.dp, vertical = 8.dp)
            .clip(containerShape)
            .background(p.surface)
            .specularBorder(
                shape = containerShape,
                topColor = if (chat.draft.isNotEmpty() || busy) engineTone.copy(alpha = 0.48f) else p.specularTop,
                bottomColor = if (chat.draft.isNotEmpty()) p.mix(p.line, engineTone, 0.25f) else p.line,
            )
            .padding(start = 14.dp, end = 10.dp, top = 11.dp, bottom = 8.dp),
    ) {
        if (chat.picked.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(bottom = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (f in chat.picked) key(f) {
                    Row(
                        Modifier
                            .clip(RoundedCornerShape(11.dp))
                            .background(p.side)
                            .border(1.dp, p.line, RoundedCornerShape(11.dp))
                            .padding(5.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (f.preview != null) {
                            Image(f.preview, f.name, Modifier.size(38.dp).clip(RoundedCornerShape(7.dp)), contentScale = ContentScale.Crop)
                        } else {
                            Box(Modifier.size(38.dp).clip(RoundedCornerShape(7.dp)).background(p.elevated), contentAlignment = Alignment.Center) {
                                VectorGlyph(Glyph.Paperclip, p.accent, size = 16.dp)
                            }
                        }
                        Column(Modifier.padding(horizontal = 8.dp).widthIn(max = 136.dp)) {
                            Label(f.name, 12.sp, p.text, maxLines = 1)
                            Label(megabytes(f.bytes.size.toLong()), 11.sp, p.muted)
                        }
                        IconButton(Icons.Default.Close, "Убрать") { vm.unpick(chat, f) }
                    }
                }
            }
        }

        BasicTextField(
            value = chat.draft,
            onValueChange = { chat.draft = it },
            textStyle = TextStyle(color = p.text, fontSize = 15.sp),
            cursorBrush = SolidColor(engineTone),
            modifier = Modifier.fillMaxWidth().heightIn(min = 26.dp, max = 160.dp),
            decorationBox = { inner ->
                Box {
                    if (chat.draft.isEmpty()) {
                        Label(
                            when {
                                !online -> "Нет связи с сервером"
                                busy && vm.canSend(chat) -> "Добавь уточнение — ${backendShortLabel(activeBackend)} учтёт по ходу работы…"
                                busy -> "${backendShortLabel(activeBackend)} отвечает…"
                                else -> "Напиши задачу… или / для команд"
                            },
                            15.sp,
                            p.muted,
                        )
                    }
                    inner()
                }
            },
        )

        Row(
            Modifier.fillMaxWidth().padding(top = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Box(
                Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(p.elevated)
                    .border(1.dp, p.borderSubtle, CircleShape)
                    .clickable { picker.launch("*/*") },
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Default.Add, "Прикрепить файлы или фото", tint = p.text, modifier = Modifier.size(18.dp))
            }

            Row(
                Modifier.weight(1f).horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Selector(
                    label = backendShortLabel(activeBackend),
                    options = listOf(
                        "claude" to "Claude Code",
                        "codex" to "Codex",
                        "agy" to "Antigravity",
                        "api" to "API",
                        "agents" to "Установить агентов…",
                        "settings" to "Настроить API…",
                    ),
                    accent = true,
                    accentColor = engineTone,
                ) { source ->
                    when (source) {
                        "agents" -> onOpenAgents()
                        "settings" -> apiSettings = true
                        else -> {
                            vm.setBackend(chat, source)
                            if (source == "api" && vm.current?.api?.baseUrl.isNullOrBlank()) apiSettings = true
                        }
                    }
                }

                val catalog = vm.catalogFor(activeBackend)
                val currentModel = chat.selectedModel(vm.current)
                Selector(
                    label = modelLabel(catalog, currentModel),
                    options = catalog.map {
                        it.value to if (it.value == "default") "Авто · ${it.description}".trimEnd(' ', '·') else modelLabel(catalog, it.value)
                    },
                ) {
                    chat.updateSelectedModel(it)
                    vm.liveChange(chat)
                }

                Selector(
                    label = when (chat.mode) { "auto" -> "Авто"; "acceptEdits" -> "Правки"; "plan" -> "План"; else -> "Подтверждение" },
                    options = listOf("auto" to "Авто", "manual" to "С подтверждением", "acceptEdits" to "Правки файлов", "plan" to "Планирование"),
                    accent = chat.mode == "auto",
                ) {
                    chat.mode = it
                    vm.liveChange(chat)
                }

                Selector(
                    label = "Глубина: " + if (chat.effort == "default") "авто" else chat.effort,
                    options = listOf("default" to "Автоматически", "low" to "Низкая", "medium" to "Средняя", "high" to "Высокая", "xhigh" to "Очень высокая", "max" to "Максимальная"),
                ) {
                    chat.effort = it
                }
            }

            val ready = (chat.draft.isNotBlank() || chat.picked.isNotEmpty()) && (vm.canSend(chat) || chat.draft.trim().startsWith("/"))
            if (busy) {
                Box(
                    Modifier.size(38.dp).clip(CircleShape).background(p.elevated).border(1.dp, p.line, CircleShape).clickable { vm.stop() },
                    contentAlignment = Alignment.Center,
                ) {
                    Box(Modifier.size(12.dp).clip(RoundedCornerShape(3.dp)).background(p.red))
                }
            }
            if (!busy || ready) {
                Box(
                    Modifier
                        .size(38.dp)
                        .clip(CircleShape)
                        .background(if (ready) p.accent else p.elevated)
                        .border(1.dp, if (ready) p.accent else p.borderSubtle, CircleShape)
                        .clickable(enabled = ready) { vm.submit() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.AutoMirrored.Filled.Send,
                        "Отправить",
                        tint = if (ready) (if (p.light) Color.White else Color(0xFF0E1318)) else p.muted,
                        modifier = Modifier.size(17.dp),
                    )
                }
            }
        }
    }
}

@Composable
fun Selector(
    label: String,
    options: List<Pair<String, String>>,
    accent: Boolean = false,
    accentColor: Color? = null,
    choose: (String) -> Unit,
) {
    val p = LocalPalette.current
    var open by remember { mutableStateOf(false) }
    val tone = accentColor ?: p.accent
    val shape = RoundedCornerShape(999.dp)
    Box {
        Row(
            Modifier
                .clip(shape)
                .background(if (accent) tone.copy(alpha = 0.14f) else p.elevated)
                .border(1.dp, if (accent) tone.copy(alpha = 0.35f) else p.borderSubtle, shape)
                .clickable { open = true }
                .padding(horizontal = 10.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(label, 12.sp, if (accent) tone else p.muted, FontWeight.Medium, maxLines = 1)
            Icon(Icons.Default.KeyboardArrowDown, null, tint = if (accent) tone else p.muted, modifier = Modifier.size(15.dp))
        }
        DropdownMenu(open, { open = false }, Modifier.background(p.elevated)) {
            for ((id, name) in options) {
                DropdownMenuItem({ Label(name) }, { open = false; choose(id) })
            }
        }
    }
}

// --- 1-Click CLI Agent Installer & Server OAuth UI -------------------------------------

@Composable
fun AgentsSheet(vm: RelayViewModel, close: () -> Unit) {
    val p = LocalPalette.current
    LaunchedEffect(Unit) { vm.refreshBackends() }
    Sheet(close) {
        Column(Modifier.heightIn(max = 560.dp).verticalScroll(rememberScrollState())) {
            AgentsSectionCard(vm)
            Spacer(Modifier.height(12.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Закрыть", onClick = close)
            }
        }
    }
}

@Composable
fun AgentsSectionCard(vm: RelayViewModel) {
    val p = LocalPalette.current
    val anyInstalling = vm.installingAgents.values.any { it }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .background(p.surface)
            .border(1.dp, p.line, RoundedCornerShape(16.dp))
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            VectorGlyph(Glyph.Terminal, p.accent, size = 18.dp)
            Spacer(Modifier.width(8.dp))
            Column(Modifier.weight(1f)) {
                Label("Агенты CLI на сервере", 15.sp, p.text, FontWeight.SemiBold)
                Label("Установка и вход в 1 нажатие прямо по SSH", 11.5.sp, p.muted)
            }
            SecondaryPillButton("Установить все", Glyph.Download, p.accent, enabled = vm.link == Link.Online && !anyInstalling) {
                vm.installAgent("all", "install")
            }
        }

        for (agent in CLI_AGENTS) {
            val status = vm.backendsStatus[agent]
            val installed = status?.installed == true || (agent == "claude" && !vm.backendsKnown && vm.claudeVersion.isNotEmpty())
            val ver = status?.version?.ifBlank { null } ?: if (agent == "claude") vm.claudeVersion.ifBlank { null } else null
            val installing = vm.installingAgents[agent] == true || (agent == "claude" && vm.updating)
            val msg = vm.agentMessages[agent].orEmpty()
            val log = vm.agentLogs[agent].orEmpty()
            var showLog by remember(agent) { mutableStateOf(false) }

            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(12.dp))
                    .background(p.base)
                    .border(1.dp, p.borderSubtle, RoundedCornerShape(12.dp))
                    .padding(11.dp),
                verticalArrangement = Arrangement.spacedBy(7.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    BackendBadge(agent, compact = true)
                    Label(backendLabel(agent), 14.sp, p.text, FontWeight.Medium, modifier = Modifier.weight(1f), maxLines = 1)
                    val pillColor = if (installed) p.green else p.amber
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .background(pillColor.copy(alpha = 0.14f))
                            .padding(horizontal = 7.dp, vertical = 2.dp),
                    ) {
                        Label(
                            text = when {
                                installing -> "установка…"
                                installed -> ver?.take(22) ?: "установлен"
                                vm.backendsKnown -> "не установлен"
                                else -> "проверка"
                            },
                            size = 10.5.sp,
                            color = pillColor,
                            weight = FontWeight.Medium,
                            maxLines = 1,
                        )
                    }
                }
                if (msg.isNotEmpty()) {
                    Label(msg, 11.5.sp, p.muted, maxLines = 2)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    SecondaryPillButton(
                        text = when {
                            installing -> "Работаем…"
                            installed -> "Обновить"
                            else -> "Установить"
                        },
                        glyph = Glyph.Download,
                        tint = if (installed) p.text else p.accent,
                        enabled = vm.link == Link.Online && !installing,
                    ) {
                        vm.installAgent(agent, if (installed) "update" else "install")
                    }
                    SecondaryPillButton(
                        text = "Войти",
                        glyph = Glyph.Key,
                        tint = p.accent,
                        enabled = vm.link == Link.Online && !installing,
                    ) {
                        vm.startAuth(agent)
                    }
                    if (log.isNotEmpty()) {
                        GhostButton(if (showLog) "Скрыть лог" else "Лог", p.muted) { showLog = !showLog }
                    }
                }
                if (showLog && log.isNotEmpty()) {
                    Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(p.side).padding(8.dp)) {
                        Text(log.takeLast(600), color = p.muted, fontSize = 11.sp, fontFamily = FontFamily.Monospace, maxLines = 8, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
    }
}

@Composable
fun AuthSheet(vm: RelayViewModel, flow: AuthFlowState) {
    val p = LocalPalette.current
    var code by remember(flow.id) { mutableStateOf("") }
    var browserText by remember(flow.id) { mutableStateOf("") }
    var boxSize by remember { mutableStateOf(IntSize.Zero) }

    Sheet({ vm.cancelAuth() }) {
        Column(Modifier.heightIn(max = 580.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                BackendBadge(flow.provider)
                Label("Вход · ${backendLabel(flow.provider)}", 17.sp, p.text, FontWeight.SemiBold)
            }
            Label(
                flow.message.ifBlank { "Следуй инструкциям для входа на сервере." },
                13.sp,
                when (flow.state) {
                    "success" -> p.green
                    "failed", "input_error" -> p.red
                    else -> p.muted
                },
            )

            if (flow.frame != null) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .aspectRatio(flow.frameWidth.toFloat() / flow.frameHeight.coerceAtLeast(1).toFloat())
                        .clip(RoundedCornerShape(12.dp))
                        .background(p.base)
                        .border(1.dp, p.line, RoundedCornerShape(12.dp))
                        .onSizeChanged { boxSize = it }
                        .pointerInput(flow.id, flow.frameWidth, flow.frameHeight) {
                            detectTapGestures { offset ->
                                if (boxSize.width > 0 && boxSize.height > 0) {
                                    val rx = (offset.x / boxSize.width * flow.frameWidth).toInt().coerceIn(0, flow.frameWidth - 1)
                                    val ry = (offset.y / boxSize.height * flow.frameHeight).toInt().coerceIn(0, flow.frameHeight - 1)
                                    vm.tapAuthBrowser(rx, ry)
                                }
                            }
                        },
                ) {
                    Image(
                        bitmap = flow.frame,
                        contentDescription = "Браузер на сервере",
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Fit,
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Field(browserText, { browserText = it }, "Ввод в поле браузера (email / пароль)", modifier = Modifier.weight(1f))
                    SecondaryPillButton("Ввести", Glyph.Check, p.accent, enabled = browserText.isNotEmpty()) {
                        vm.sendAuthBrowserText(browserText, pressEnter = true)
                        browserText = ""
                    }
                }
            }

            if (flow.message.contains("не подготовлен браузер", ignoreCase = true)) {
                SecondaryPillButton(
                    if (vm.browserSetupRunning) "Готовим браузер…" else "Подготовить браузер на сервере",
                    Glyph.Download,
                    p.accent,
                    enabled = !vm.browserSetupRunning,
                ) {
                    vm.setupServerBrowser()
                }
            }

            if (flow.active) {
                Field(
                    value = code,
                    onChange = { code = it },
                    hint = if (flow.provider == "codex") "Код OAuth или API-ключ sk-…" else "Код входа или URL страницы возврата",
                )
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton(if (flow.active) "Отмена" else "Закрыть") { vm.cancelAuth() }
                if (flow.active) {
                    Spacer(Modifier.width(6.dp))
                    PrimaryButton("Подтвердить", enabled = code.isNotBlank()) {
                        vm.submitAuthInput(code)
                        code = ""
                    }
                }
            }
        }
    }
}

// --- Settings Screen -------------------------------------------------------------------

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    val back = { vm.screen = if (vm.current != null && vm.link != Link.Off) Screen.Sessions else Screen.Servers }
    BackHandler { back() }
    var name by remember { mutableStateOf(vm.actor) }

    Column(Modifier.fillMaxSize()) {
        TopBar("Настройки", "24 темы, профиль и агенты сервера", onBack = back)
        LazyColumn(
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // Compact 3-column grid near the top so all 24 themes (including "Дракула") fit on 400x860dp without scrolling!
            item {
                Label("Цветовая тема · 24 палитры", 14.sp, p.text, FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                FlowRow(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(7.dp),
                ) {
                    for (t in THEMES) ThemeTile(t, vm.theme == t.id) { vm.chooseTheme(t.id) }
                }
            }
            if (vm.link == Link.Online) {
                item {
                    AgentsSectionCard(vm)
                }
            }
            item {
                Column(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(16.dp))
                        .background(p.surface)
                        .border(1.dp, p.line, RoundedCornerShape(16.dp))
                        .padding(14.dp),
                ) {
                    Label("Твоё имя", 15.sp, p.text, FontWeight.SemiBold)
                    Spacer(Modifier.height(3.dp))
                    Label("Видно на доске задач и в общих названиях сессий", 12.sp, p.muted)
                    Spacer(Modifier.height(8.dp))
                    Field(name, { name = it; vm.chooseActor(it) }, "Например, champ")
                }
            }
            item {
                if (vm.notice.isNotEmpty()) {
                    Label(vm.notice, 12.sp, p.accent)
                    Spacer(Modifier.height(6.dp))
                }
                Label("Relay для Android 0.5.0 · SSH и общие сессии с ПК", 12.sp, p.muted)
                Spacer(Modifier.height(8.dp))
            }
        }
    }
}

// --- Live Agent Observatory Sheet ("Радар агентов") ------------------------------------

private data class RadarNode(
    val sid: String,
    val title: String,
    val cwd: String,
    val backend: String,
    val active: Boolean,
    val lockedGate: Boolean,
    val modified: Double,
    val session: Session?,
    val telemetry: TurnTelemetry?,
)

@Composable
fun AgentRadarSheet(vm: RelayViewModel, close: () -> Unit) {
    val p = LocalPalette.current
    val server = vm.current
    val defaultBackend = server?.effectiveBackend() ?: "claude"

    val nodes = remember(vm.sessions, vm.running, vm.selected, vm.chats.size) {
        val fromSessions = vm.sessions.map { s ->
            val chat = vm.chats[s.id]
            val isBusy = s.running || s.id in vm.running || (chat != null && chat.version >= 0 && chat.transcript.busy)
            val b = s.backend.takeIf { it in BACKENDS } ?: chat?.activeBackend() ?: defaultBackend
            val tel = if (chat != null && !s.isLockedGate() && chat.transcript.messages.isNotEmpty()) {
                analyzeTurnTelemetry(chat.transcript.messages, isBusy, chat.transcript.status)
            } else null
            RadarNode(
                sid = s.id,
                title = if (s.isLockedGate() && s.title.isBlank()) "Защищённая сессия" else s.title,
                cwd = s.cwd,
                backend = b,
                active = isBusy,
                lockedGate = s.isLockedGate(),
                modified = s.modified,
                session = s,
                telemetry = tel,
            )
        }
        val extraCurrent = vm.chat()?.takeIf { cur -> fromSessions.none { it.sid == cur.sid } }?.let { cur ->
            val isBusy = cur.version >= 0 && cur.transcript.busy
            listOf(
                RadarNode(
                    sid = cur.sid,
                    title = cur.title,
                    cwd = cur.cwd,
                    backend = cur.activeBackend(),
                    active = isBusy,
                    lockedGate = cur.isLockedGate(),
                    modified = System.currentTimeMillis() / 1000.0,
                    session = null,
                    telemetry = if (!cur.isLockedGate() && cur.transcript.messages.isNotEmpty()) {
                        analyzeTurnTelemetry(cur.transcript.messages, isBusy, cur.transcript.status)
                    } else null,
                )
            )
        }.orEmpty()
        (extraCurrent + fromSessions)
            .sortedWith(compareByDescending<RadarNode> { it.active }.thenByDescending { it.modified })
            .take(6)
    }

    val activeCount = nodes.count { it.active }

    Sheet(close) {
        Column(
            Modifier.heightIn(max = 580.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                OrbitalEmblem(if (activeCount > 0) p.green else p.accent, active = activeCount > 0, size = 30.dp)
                Column(Modifier.weight(1f)) {
                    Label("Радар агентов", 18.sp, p.text, FontWeight.SemiBold)
                    Label(
                        "${server?.name?.ifBlank { server.host } ?: "Сервер"} · живая топология сессий",
                        12.sp,
                        p.muted,
                        maxLines = 1,
                    )
                }
                Box(
                    Modifier
                        .clip(RoundedCornerShape(999.dp))
                        .background((if (activeCount > 0) p.green else p.muted).copy(alpha = 0.15f))
                        .padding(horizontal = 9.dp, vertical = 4.dp),
                ) {
                    Label(
                        if (activeCount > 0) "LIVE · $activeCount" else "Ожидание",
                        11.sp,
                        if (activeCount > 0) p.green else p.muted,
                        FontWeight.Bold,
                    )
                }
            }

            // Visual Server -> Active/Recent Agents Topology Graph
            val graphShape = RoundedCornerShape(16.dp)
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(126.dp)
                    .clip(graphShape)
                    .background(
                        Brush.horizontalGradient(
                            listOf(p.base, p.mix(p.base, p.side, 0.6f))
                        )
                    )
                    .specularBorder(graphShape, p.specularTop, p.line)
                    .padding(horizontal = 14.dp, vertical = 10.dp),
            ) {
                val graphNodes = nodes.take(5)
                val violet = phaseColor(ToolPhase.Subagent, p.light)
                Canvas(Modifier.fillMaxSize()) {
                    val w = size.width
                    val h = size.height
                    val hub = Offset(w * 0.14f, h * 0.50f)

                    // Radar background grid rings around Server Hub
                    drawCircle(p.accent.copy(alpha = 0.08f), h * 0.46f, hub)
                    drawCircle(p.line.copy(alpha = 0.5f), h * 0.44f, hub, style = Stroke(width = 1.dp.toPx()))
                    drawCircle(p.line.copy(alpha = 0.35f), h * 0.27f, hub, style = Stroke(width = 1.dp.toPx()))

                    if (graphNodes.isNotEmpty()) {
                        val count = graphNodes.size
                        for ((idx, node) in graphNodes.withIndex()) {
                            val ny = if (count == 1) h * 0.5f else h * (0.14f + 0.72f * idx / (count - 1).toFloat())
                            val target = Offset(w * 0.76f, ny)
                            val nTone = backendColor(node.backend, p.light)
                            val curve = Path().apply {
                                moveTo(hub.x + 12.dp.toPx(), hub.y)
                                cubicTo(
                                    w * 0.42f, hub.y,
                                    w * 0.48f, target.y,
                                    target.x - 10.dp.toPx(), target.y,
                                )
                            }
                            drawPath(
                                path = curve,
                                color = nTone.copy(alpha = if (node.active) 0.78f else 0.32f),
                                style = Stroke(width = if (node.active) 2.2.dp.toPx() else 1.3.dp.toPx(), cap = StrokeCap.Round),
                            )

                            val nr = if (node.active) 7.5.dp.toPx() else 5.5.dp.toPx()
                            if (node.active) {
                                drawCircle(nTone.copy(alpha = 0.22f), nr * 1.9f, target)
                                drawArc(
                                    color = nTone,
                                    startAngle = -50f,
                                    sweepAngle = 220f,
                                    useCenter = false,
                                    topLeft = target - Offset(nr * 1.35f, nr * 1.35f),
                                    size = Size(nr * 2.7f, nr * 2.7f),
                                    style = Stroke(width = 1.5.dp.toPx(), cap = StrokeCap.Round),
                                )
                            }
                            drawCircle(p.elevated, nr, target)
                            drawCircle(nTone, nr, target, style = Stroke(width = 1.8.dp.toPx()))
                            drawCircle(nTone, nr * 0.45f, target)

                            // Subagent satellite branch when delegations exist
                            val subs = node.telemetry?.delegations ?: 0
                            if (subs > 0) {
                                val sat = Offset(w * 0.91f, (target.y - 6.dp.toPx()).coerceIn(8.dp.toPx(), h - 8.dp.toPx()))
                                drawLine(violet.copy(alpha = 0.65f), Offset(target.x + nr, target.y), sat, 1.4.dp.toPx(), StrokeCap.Round)
                                drawCircle(violet.copy(alpha = 0.25f), 6.5.dp.toPx(), sat)
                                drawCircle(violet, 3.8.dp.toPx(), sat)
                            }
                        }
                    }

                    // Draw Server Hub Node on top
                    val hr = 11.dp.toPx()
                    drawCircle(p.accent.copy(alpha = 0.24f), hr * 1.55f, hub)
                    drawCircle(p.elevated, hr, hub)
                    drawCircle(p.accent, hr, hub, style = Stroke(width = 2.dp.toPx()))
                    drawCircle(if (vm.link == Link.Online) p.green else p.amber, 4.dp.toPx(), hub)
                }
                Label(
                    server?.name?.ifBlank { server.host } ?: "SSH Hub",
                    10.5.sp,
                    p.muted,
                    FontWeight.SemiBold,
                    maxLines = 1,
                    modifier = Modifier.align(Alignment.BottomStart),
                )
                Label(
                    if (graphNodes.isEmpty()) "Нет сессий" else "Узлов: ${graphNodes.size}",
                    10.5.sp,
                    p.muted,
                    maxLines = 1,
                    modifier = Modifier.align(Alignment.BottomEnd),
                )
            }

            if (nodes.isEmpty()) {
                Box(Modifier.fillMaxWidth().padding(vertical = 16.dp), contentAlignment = Alignment.Center) {
                    Label("Пока нет активных или недавних сессий на сервере.", 13.sp, p.muted)
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
                    for ((idx, node) in nodes.withIndex()) {
                        val nTone = backendColor(p, node.backend)
                        val cardShape = RoundedCornerShape(14.dp)
                        val isLast = idx == nodes.lastIndex
                        Box(
                            Modifier
                                .fillMaxWidth()
                                .drawBehind {
                                    val x = 8.dp.toPx()
                                    val midY = size.height * 0.5f
                                    drawLine(p.line, Offset(x, 0f), Offset(x, if (isLast) midY else size.height), 1.5.dp.toPx())
                                    drawLine(nTone.copy(alpha = 0.7f), Offset(x, midY), Offset(18.dp.toPx(), midY), 1.5.dp.toPx())
                                    drawCircle(nTone, 3.2.dp.toPx(), Offset(x, midY))
                                }
                                .padding(start = 20.dp),
                        ) {
                            Column(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(cardShape)
                                    .background(if (node.active) p.mix(p.surface, nTone, 0.08f) else p.surface)
                                    .specularBorder(
                                        cardShape,
                                        if (node.active) nTone.copy(alpha = 0.55f) else p.specularTop,
                                        if (node.active) nTone.copy(alpha = 0.25f) else p.borderSubtle,
                                    )
                                    .clickable {
                                        if (node.session != null) {
                                            vm.open(node.session)
                                        } else {
                                            vm.screen = Screen.Chat
                                        }
                                        close()
                                    }
                                    .padding(horizontal = 12.dp, vertical = 9.dp),
                                verticalArrangement = Arrangement.spacedBy(5.dp),
                            ) {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                                    BackendBadge(node.backend, compact = true, showDot = true)
                                    Label(node.title, 13.5.sp, p.text, FontWeight.SemiBold, maxLines = 1, modifier = Modifier.weight(1f))
                                    if (node.active) {
                                        NeuralWaveform(nTone, active = true, width = 16.dp, height = 10.dp)
                                    } else {
                                        Label(relative(node.modified), 11.sp, p.muted, maxLines = 1)
                                    }
                                }
                                val tel = node.telemetry
                                when {
                                    node.lockedGate -> Label("Защищена паролем · нажми для входа", 11.5.sp, p.amber, maxLines = 1)
                                    tel != null && tel.totalActions > 0 -> {
                                        Label(
                                            "${tel.activeHeadline} · ${tel.doneActions}/${tel.totalActions} ${stepsWord(tel.totalActions)}",
                                            11.5.sp,
                                            if (node.active) nTone else p.muted,
                                            maxLines = 1,
                                        )
                                        Row(
                                            Modifier.fillMaxWidth().height(3.5.dp).clip(CircleShape).background(p.base),
                                            horizontalArrangement = Arrangement.spacedBy(2.dp),
                                        ) {
                                            if (tel.reads > 0) Box(Modifier.weight(tel.reads.toFloat()).fillMaxHeight().background(phaseColor(ToolPhase.Research, p.light)))
                                            if (tel.delegations > 0) Box(Modifier.weight(tel.delegations.toFloat()).fillMaxHeight().background(phaseColor(ToolPhase.Subagent, p.light)))
                                            if (tel.edits > 0) Box(Modifier.weight(tel.edits.toFloat()).fillMaxHeight().background(phaseColor(ToolPhase.Edit, p.light)))
                                            if (tel.commands > 0) Box(Modifier.weight(tel.commands.toFloat()).fillMaxHeight().background(phaseColor(ToolPhase.Terminal, p.light)))
                                            if (tel.desktopOps > 0) Box(Modifier.weight(tel.desktopOps.toFloat()).fillMaxHeight().background(phaseColor(ToolPhase.Desktop, p.light)))
                                            val rest = (tel.totalActions - tel.reads - tel.delegations - tel.edits - tel.commands - tel.desktopOps).coerceAtLeast(0)
                                            if (rest > 0) Box(Modifier.weight(rest.toFloat()).fillMaxHeight().background(phaseColor(ToolPhase.Other, p.light)))
                                        }
                                    }
                                    node.active -> Label("Выполняется в ${folderName(node.cwd)} · открыть →", 11.5.sp, nTone, maxLines = 1)
                                    else -> Label("Папка ${folderName(node.cwd)} · открыть →", 11.5.sp, p.muted, maxLines = 1)
                                }
                            }
                        }
                    }
                }
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Закрыть", onClick = close)
            }
        }
    }
}

@Composable
fun ThemeTile(t: ThemeInfo, selected: Boolean, choose: () -> Unit) {
    val p = LocalPalette.current
    val c = palette(t.id)
    val shape = RoundedCornerShape(11.dp)
    Column(
        Modifier
            .width(114.dp)
            .clip(shape)
            .background(c.base)
            .border(if (selected) 2.dp else 1.dp, if (selected) p.accent else p.line, shape)
            .clickable(onClick = choose)
            .padding(horizontal = 8.dp, vertical = 6.dp),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.width(12.dp).height(16.dp).clip(RoundedCornerShape(3.dp)).background(c.rail))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Box(Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(2.dp)).background(c.surface))
                Box(Modifier.width(28.dp).height(4.dp).clip(RoundedCornerShape(2.dp)).background(c.accent))
            }
            if (selected) {
                Box(Modifier.size(7.dp).clip(CircleShape).background(c.accent))
            }
        }
        Spacer(Modifier.height(4.dp))
        Text(
            text = t.name,
            color = c.text,
            fontSize = 11.5.sp,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.align(Alignment.CenterHorizontally),
        )
    }
}
