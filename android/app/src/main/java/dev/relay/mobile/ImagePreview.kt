package dev.relay.mobile

import android.graphics.BitmapFactory
import android.net.Uri
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.InputStream

private fun sampledPreview(edge: Int, open: () -> InputStream?): ImageBitmap? {
    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    open()?.use { BitmapFactory.decodeStream(it, null, options) }
    if (options.outWidth <= 0 || options.outHeight <= 0) return null
    options.inJustDecodeBounds = false
    options.inSampleSize = 1
    while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > edge) options.inSampleSize *= 2
    return open()?.use { BitmapFactory.decodeStream(it, null, options)?.asImageBitmap() }
}

@Composable
internal fun ImageAttachmentPreview(data: String) {
    PicturePreview(data, "Изображение", load = { edge ->
        val bytes = java.util.Base64.getDecoder().decode(data)
        sampledPreview(edge) { bytes.inputStream() }
    })
}

@Composable
internal fun ServerImagePreview(vm: RelayViewModel, chat: Chat, path: String) {
    if (!Downloads.mime(path).startsWith("image/")) return
    val context = LocalContext.current
    val title = path.substringAfterLast('/')
    LaunchedEffect(chat.sid, path, vm.link) { vm.preview(chat, path) }
    val data = chat.previews[path]
    val transfer = vm.transferFor(path, chat.cwd)
    val uri = transfer?.uri
    val download = { vm.download(path, chat.cwd) }
    if (uri == null && (data.isNullOrEmpty() || data == "pending")) {
        val waiting = data == "pending" || (transfer != null && !transfer.done && transfer.error.isEmpty())
        PreviewPlaceholder(title, if (waiting) "Загрузка…" else "Показать оригинал", if (waiting) null else download)
        return
    }
    val action = when {
        uri != null -> "Открыть оригинал"
        transfer != null && transfer.error.isEmpty() -> "Загружается…"
        else -> "Скачать оригинал"
    }
    PicturePreview(uri ?: data.orEmpty(), title, action, {
        if (uri != null) vm.openTransfer(transfer) else download()
    }) { edge ->
        if (uri != null) sampledPreview(edge) { context.contentResolver.openInputStream(uri) }
        else {
            val bytes = java.util.Base64.getDecoder().decode(data)
            sampledPreview(edge) { bytes.inputStream() }
        }
    }
}

@Composable
internal fun DownloadPreview(uri: Uri, name: String = "Изображение") {
    val context = LocalContext.current
    PicturePreview(uri, name, load = { edge -> sampledPreview(edge) { context.contentResolver.openInputStream(uri) } })
}

@Composable
private fun PreviewPlaceholder(name: String, status: String, click: (() -> Unit)? = null) {
    val p = LocalPalette.current
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(p.surface)
        .then(if (click == null) Modifier else Modifier.clickable(onClick = click)).padding(12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        VectorGlyph(Glyph.Image, p.muted, 18.dp)
        Column { Label(name, 13.sp, p.text, maxLines = 1); Label(status, 12.sp, p.muted) }
    }
}

@Composable
private fun PicturePreview(source: Any, title: String, action: String? = null, original: (() -> Unit)? = null,
                           load: (Int) -> ImageBitmap?) {
    val p = LocalPalette.current
    val thumb by produceState<ImageBitmap?>(null, source) {
        value = withContext(Dispatchers.IO) { runCatching { load(1280) }.getOrNull() }
    }
    val image = thumb
    if (image == null) {
        PreviewPlaceholder(title, if (original == null) "Предпросмотр изображения" else action.orEmpty(), original)
        return
    }
    var expanded by remember(title) { mutableStateOf(false) }
    val shape = RoundedCornerShape(15.dp)
    Column(Modifier.fillMaxWidth().clip(shape).background(p.surface).border(1.dp, p.line, shape)
        .semantics { contentDescription = "Открыть изображение" }.clickable(role = Role.Button) { expanded = true }) {
        Box(Modifier.fillMaxWidth().aspectRatio((image.width.toFloat() / image.height).coerceIn(1.35f, 2.05f))
            .background(Color(0xff17191d)), contentAlignment = Alignment.Center) {
            Image(image, null, Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
        }
        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            VectorGlyph(Glyph.Image, p.muted, 15.dp)
            Label(title, 12.5.sp, p.text, FontWeight.Medium, maxLines = 1, modifier = Modifier.weight(1f))
            Canvas(Modifier.size(14.dp)) {
                val lo = size.width * 0.15f
                val hi = size.width - lo
                val mid = size.width * 0.5f
                val stroke = 1.5.dp.toPx()
                drawLine(p.muted, Offset(lo, hi), Offset(hi, lo), stroke)
                drawLine(p.muted, Offset(mid, lo), Offset(hi, lo), stroke)
                drawLine(p.muted, Offset(hi, lo), Offset(hi, mid), stroke)
                drawLine(p.muted, Offset(lo, mid), Offset(lo, hi), stroke)
                drawLine(p.muted, Offset(lo, hi), Offset(mid, hi), stroke)
            }
        }
    }
    if (expanded) {
        val large by produceState(image, source) {
            value = withContext(Dispatchers.IO) { runCatching { load(3072) }.getOrNull() } ?: image
        }
        ImageViewer(large, title, action, original) { expanded = false }
    }
}

@Composable
private fun ImageViewer(image: ImageBitmap, title: String, action: String?, original: (() -> Unit)?, close: () -> Unit) {
    val ink = Color(0xfff1f1f3)
    var zoom by remember(image) { mutableFloatStateOf(1f) }
    var pan by remember(image) { mutableStateOf(Offset.Zero) }
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Column(Modifier.fillMaxSize().background(Color(0xff101114)).systemBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Label(title, 15.sp, ink, FontWeight.Medium, maxLines = 1, modifier = Modifier.weight(1f))
                Box(Modifier.size(48.dp).clip(CircleShape).clickable(role = Role.Button, onClick = close),
                    contentAlignment = Alignment.Center) { Icon(Icons.Default.Close, "Закрыть изображение", tint = ink) }
            }
            BoxWithConstraints(Modifier.fillMaxWidth().weight(1f).clipToBounds(), contentAlignment = Alignment.Center) {
                val w = constraints.maxWidth.toFloat()
                val h = constraints.maxHeight.toFloat()
                val fit = minOf(w / image.width, h / image.height)
                fun transform(focus: Offset, movement: Offset, next: Float) {
                    val centre = Offset(w / 2, h / 2)
                    val adjusted = focus - centre + (pan - (focus - centre)) * (next / zoom) + movement
                    val dx = ((image.width * fit * next - w) / 2).coerceAtLeast(0f)
                    val dy = ((image.height * fit * next - h) / 2).coerceAtLeast(0f)
                    pan = Offset(adjusted.x.coerceIn(-dx, dx), adjusted.y.coerceIn(-dy, dy))
                    zoom = next
                }
                Box(Modifier.fillMaxSize()
                    .pointerInput(image, w, h) { detectTapGestures(onDoubleTap = { transform(it, Offset.Zero, if (zoom > 1f) 1f else 2.5f) }) }
                    .pointerInput(image, w, h) { detectTransformGestures { focus, delta, factor, _ ->
                        transform(focus, delta, (zoom * factor).coerceIn(1f, 6f))
                    } }) {
                    Image(image, title, Modifier.fillMaxSize().graphicsLayer {
                        scaleX = zoom; scaleY = zoom; translationX = pan.x; translationY = pan.y
                    }, contentScale = ContentScale.Fit)
                }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.clip(RoundedCornerShape(12.dp)).background(Color(0xff25272c)),
                    verticalAlignment = Alignment.CenterVertically) {
                    ZoomButton("−", "Уменьшить", zoom > 1f) { zoom = (zoom / 1.5f).coerceAtLeast(1f); pan = Offset.Zero }
                    Label("${(zoom * 100).toInt()}%", 12.sp, ink)
                    ZoomButton("+", "Увеличить", zoom < 6f) { zoom = (zoom * 1.5f).coerceAtMost(6f); pan = Offset.Zero }
                }
                Spacer(Modifier.weight(1f))
                if (original != null && action != null) GhostButton(action, ink, original)
            }
        }
    }
}

@Composable
private fun ZoomButton(symbol: String, description: String, enabled: Boolean, click: () -> Unit) {
    Box(Modifier.size(40.dp).semantics { contentDescription = description }
        .clickable(enabled = enabled, role = Role.Button, onClick = click), contentAlignment = Alignment.Center) {
        Label(symbol, 18.sp, if (enabled) Color.White else Color.Gray)
    }
}

@Composable
internal fun TestGallery(vm: RelayViewModel, chat: Chat, paths: List<String>) {
    val p = LocalPalette.current
    val state = rememberPagerState(pageCount = { paths.size })
    Column(Modifier.fillMaxWidth().padding(vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Label("Снимки тестов на ПК", 14.sp, p.text, FontWeight.SemiBold, modifier = Modifier.weight(1f))
            if (paths.size > 1) Label("${state.currentPage + 1} / ${paths.size}", 12.sp, p.muted)
        }
        HorizontalPager(state = state, key = { paths[it] }, pageSpacing = 10.dp,
            contentPadding = PaddingValues(end = if (paths.size > 1) 24.dp else 0.dp)) { index ->
            ServerImagePreview(vm, chat, paths[index])
        }
    }
}

@Composable
internal fun PickedImagePreview(file: Picked) {
    val thumbnail = file.preview ?: return
    var expanded by remember(file) { mutableStateOf(false) }
    Image(thumbnail, file.name, Modifier.size(44.dp).clip(RoundedCornerShape(8.dp))
        .clickable(role = Role.Button) { expanded = true }, contentScale = ContentScale.Crop)
    if (expanded) {
        val large by produceState(thumbnail, file) {
            value = withContext(Dispatchers.IO) { sampledPreview(3072) { file.bytes.inputStream() } } ?: thumbnail
        }
        ImageViewer(large, file.name, null, null) { expanded = false }
    }
}
