package dev.relay.mobile

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn

import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.InputStream

private fun sampledPreview(open: () -> InputStream?): ImageBitmap? {
    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    open()?.use { BitmapFactory.decodeStream(it, null, options) }
    if (options.outWidth <= 0 || options.outHeight <= 0) return null
    options.inJustDecodeBounds = false
    options.inSampleSize = 1
    while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 1280) options.inSampleSize *= 2
    return open()?.use { BitmapFactory.decodeStream(it, null, options)?.asImageBitmap() }
}

@Composable
internal fun ImageAttachmentPreview(data: String) {
    PicturePreview(data) {
        val bytes = java.util.Base64.getDecoder().decode(data)
        sampledPreview { bytes.inputStream() }
    }
}

@Composable
internal fun ServerImagePreview(vm: RelayViewModel, chat: Chat, path: String) {
    if (!Downloads.mime(path).startsWith("image/")) return
    LaunchedEffect(chat.sid, path, vm.link) { vm.preview(chat, path) }
    val data = chat.previews[path] ?: return
    when {
        data == "pending" -> androidx.compose.material3.Text("Загрузка снимка…")
        data.isNotEmpty() -> ImageAttachmentPreview(data)
        else -> {
            val transfer = vm.transfers.firstOrNull { it.path == path }
            val uri = transfer?.uri
            if (uri != null) DownloadPreview(uri)
            else GhostButton(if (transfer != null && transfer.error.isEmpty()) "Загрузка оригинала…" else "Показать оригинал") {
                vm.download(path, chat.cwd)
            }
        }
    }
}

@Composable
internal fun DownloadPreview(uri: android.net.Uri) {
    val context = androidx.compose.ui.platform.LocalContext.current
    PicturePreview(uri) { sampledPreview { context.contentResolver.openInputStream(uri) } }
}

@Composable
private fun PicturePreview(key: Any, decode: () -> ImageBitmap?) {
    val bitmap by produceState<ImageBitmap?>(null, key) {
        value = withContext(Dispatchers.IO) { runCatching(decode).getOrNull() }
    }
    val image = bitmap ?: return
    var expanded by remember(key) { mutableStateOf(false) }
    Image(image, "Открыть изображение", Modifier.fillMaxWidth().heightIn(max = 180.dp).clickable { expanded = true }, contentScale = ContentScale.Fit)
    if (expanded) Dialog({ expanded = false }) {
        Image(image, "Закрыть изображение", Modifier.fillMaxWidth().clickable { expanded = false }, contentScale = ContentScale.Fit)
    }
}
