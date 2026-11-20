package dev.relay.mobile

import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas as BitmapCanvas
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.ByteArrayOutputStream
import java.util.Base64
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.max
import kotlin.math.min

private const val IconSize = 96
private const val MaxEncoded = 60_000

private enum class Fit(val label: String) {
    Contain("Вписать"), Cover("Заполнить"), Original("По рисунку")
}

private fun decodeIcon(encoded: String): Bitmap? {
    if (encoded.isEmpty() || encoded.length > MaxEncoded) return null
    val bytes = runCatching { Base64.getDecoder().decode(encoded) }.getOrNull() ?: return null
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    return if (bounds.outWidth == IconSize && bounds.outHeight == IconSize && bounds.outMimeType == "image/png") {
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
    } else null
}

private fun importIcon(resolver: ContentResolver, uri: Uri): Bitmap {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
    require(bounds.outWidth in 1..8192 && bounds.outHeight in 1..8192) { "Выбери изображение размером до 8192 × 8192." }
    val options = BitmapFactory.Options()
    while (max(bounds.outWidth, bounds.outHeight) / options.inSampleSize.coerceAtLeast(1) > 768) {
        options.inSampleSize = options.inSampleSize.coerceAtLeast(1) * 2
    }
    return resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) }
        ?: error("Не удалось прочитать изображение.")
}

private fun renderIcon(source: Bitmap, fit: Fit, zoom: Float, margin: Float, radius: Float, background: Color): Bitmap {
    val image = Bitmap.createBitmap(IconSize, IconSize, Bitmap.Config.ARGB_8888)
    val center = IconSize / 2f
    val inset = IconSize * margin
    val area = IconSize - inset * 2
    val scale = zoom * when (fit) {
        Fit.Contain -> min(area / source.width, area / source.height)
        Fit.Cover -> max(area / source.width, area / source.height)
        Fit.Original -> 1f
    }
    val halfW = source.width * scale / 2
    val halfH = source.height * scale / 2
    val canvas = BitmapCanvas(image)
    val shape = Path().apply { addRoundRect(RectF(0f, 0f, IconSize.toFloat(), IconSize.toFloat()), radius, radius, Path.Direction.CW) }
    canvas.clipPath(shape)
    canvas.drawColor(background.toArgb())
    canvas.clipRect(inset, inset, IconSize - inset, IconSize - inset)
    canvas.drawBitmap(source, null, RectF(center - halfW, center - halfH, center + halfW, center + halfH),
        Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG))
    return image
}

@Composable
fun ProjectFolderIcon(folder: Folder, open: Boolean) {
    val p = LocalPalette.current
    val icon = remember(folder.icon) { decodeIcon(folder.icon)?.asImageBitmap() }
    if (icon == null) {
        VectorGlyph(if (open) Glyph.FolderOpen else Glyph.Folder, p.accent, size = 22.dp)
    } else {
        Image(icon, null, Modifier.size(22.dp))
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ProjectIconEditor(folder: Folder, close: () -> Unit, save: (String) -> Unit) {
    val p = LocalPalette.current
    val resolver = LocalContext.current.contentResolver
    val scope = rememberCoroutineScope()
    var source by remember(folder.id) { mutableStateOf(decodeIcon(folder.icon)) }
    var fit by remember { mutableStateOf(Fit.Contain) }
    var zoom by remember { mutableFloatStateOf(1f) }
    var margin by remember { mutableFloatStateOf(0f) }
    var radius by remember { mutableFloatStateOf(0f) }
    var paletteIndex by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf("") }
    var loading by remember { mutableStateOf(false) }
    val colors = listOf(
        "Прозрачный" to Color.Transparent, "Белый" to Color.White, "Чёрный" to Color.Black,
        "Коралловый" to Color(0xFFE27860), "Золотой" to Color(0xFFE2B44D), "Зелёный" to Color(0xFF67BC87),
        "Бирюзовый" to Color(0xFF49BCBA), "Синий" to Color(0xFF669FF0), "Сиреневый" to Color(0xFFAC83E4),
        "Розовый" to Color(0xFFD97CAB),
    )
    val preview = remember(source, fit, zoom, margin, radius, paletteIndex) {
        source?.let { renderIcon(it, fit, zoom, margin, radius, colors[paletteIndex].second) }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch {
            loading = true
            runCatching { withContext(Dispatchers.IO) { importIcon(resolver, uri) } }.onSuccess {
                source = it
                error = ""
            }.onFailure { error = it.message ?: "Не удалось открыть изображение." }
            loading = false
        }
    }
    fun transform(matrix: Matrix) {
        source?.let { source = Bitmap.createBitmap(it, 0, 0, it.width, it.height, matrix, true) }
    }
    Sheet(close) {
        Column(Modifier.heightIn(max = 570.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Label("Значок проекта", 19.sp, p.text, FontWeight.SemiBold)
            Label(folder.name, 13.sp, p.muted, maxLines = 1)
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(88.dp).clip(RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
                    Canvas(Modifier.matchParentSize()) {
                        val cell = this.size.width / 8
                        for (y in 0..7) for (x in 0..7) {
                            drawRect(if ((x + y) % 2 == 0) p.base else p.surface,
                                Offset(x * cell, y * cell), Size(cell, cell))
                        }
                    }
                    if (preview != null) Image(preview.asImageBitmap(), "Предпросмотр", Modifier.matchParentSize())
                    else VectorGlyph(Glyph.Folder, p.muted, 29.dp)
                }
                SecondaryPillButton("Выбрать файл", Glyph.Image, enabled = !loading) { picker.launch("image/*") }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(5.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                for (option in Fit.entries) {
                    SecondaryPillButton(option.label, tint = if (fit == option) p.accent else p.text,
                        enabled = source != null) { fit = option }
                }
                SecondaryPillButton("Повернуть", enabled = source != null) { transform(Matrix().apply { setRotate(90f) }) }
                SecondaryPillButton("Отразить", enabled = source != null) { transform(Matrix().apply { setScale(-1f, 1f) }) }
            }
            IconSlider("Масштаб", zoom, 0.5f..3f, (zoom * 100).toInt(), source != null) { zoom = it }
            IconSlider("Отступ", margin, 0f..0.35f, (margin * 100).toInt(), source != null) { margin = it }
            IconSlider("Скругление", radius, 0f..48f, (radius / 48f * 100).toInt(), source != null) { radius = it }
            Label("Фон", 13.sp, p.muted)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                colors.forEachIndexed { index, (label, color) ->
                    Box(Modifier.size(27.dp).clip(CircleShape).background(if (index == 0) p.base else color)
                        .border(if (index == paletteIndex) 2.dp else 1.dp, if (index == paletteIndex) p.accent else p.line, CircleShape)
                        .semantics { contentDescription = label }.clickable { paletteIndex = index },
                        contentAlignment = Alignment.Center) {
                        if (index == 0) Icon(Icons.Default.Close, null, tint = p.muted, modifier = Modifier.size(12.dp))
                    }
                }
            }
            if (error.isNotEmpty()) Label(error, 12.sp, p.red)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                if (folder.icon.isNotEmpty()) GhostButton("Сбросить") { save("") }
                GhostButton("Отмена", onClick = close)
                PrimaryButton("Сохранить", enabled = preview != null && !loading) {
                    preview?.let {
                        val buffer = ByteArrayOutputStream()
                        if (it.compress(Bitmap.CompressFormat.PNG, 100, buffer)) {
                            save(Base64.getEncoder().encodeToString(buffer.toByteArray()))
                        } else error = "Не удалось сохранить значок."
                    }
                }
            }
        }
    }
}

@Composable
private fun IconSlider(label: String, value: Float, range: ClosedFloatingPointRange<Float>, percent: Int,
    enabled: Boolean, change: (Float) -> Unit) {
    val p = LocalPalette.current
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Label(label, 13.sp, p.muted)
        Label("$percent%", 13.sp, p.muted)
    }
    Slider(value, change, modifier = Modifier.height(24.dp), valueRange = range, enabled = enabled)
}
