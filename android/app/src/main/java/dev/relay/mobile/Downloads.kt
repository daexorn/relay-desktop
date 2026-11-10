package dev.relay.mobile

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import java.io.File
import java.io.FileOutputStream
import java.security.MessageDigest
import java.util.Base64

/** A file from the server on its way to this phone's Downloads/Relay folder. */
class Transfer(val id: String, val path: String) {
    var name by mutableStateOf(path.substringAfterLast('/'))
    var size by mutableLongStateOf(0L)
    var received by mutableLongStateOf(0L)
    var error by mutableStateOf("")
    var uri by mutableStateOf<Uri?>(null)
    var mime = "application/octet-stream"
    internal var out: FileOutputStream? = null
    internal var temp: File? = null
    internal val digest: MessageDigest = MessageDigest.getInstance("SHA-256")
    val done get() = uri != null
}

object Downloads {
    fun mime(name: String): String {
        val ext = name.substringAfterLast('.', "").lowercase()
        if (ext == "apk") return "application/vnd.android.package-archive"
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
    }

    fun begin(context: Context, t: Transfer, name: String, size: Long) {
        t.name = name
        t.size = size
        t.mime = mime(name)
        val dir = File(context.cacheDir, "downloads").apply { mkdirs() }
        t.temp = File(dir, t.id).also { t.out = FileOutputStream(it) }
    }

    fun chunk(t: Transfer, data: String) {
        val body = Base64.getDecoder().decode(data)
        t.digest.update(body)
        t.out?.write(body)
        t.received += body.size
    }

    /** Checks the server's SHA-256, then publishes the file where the Files app shows it. */
    fun finish(context: Context, t: Transfer, sha256: String) {
        t.out?.close()
        t.out = null
        val temp = t.temp ?: return
        val seen = t.digest.digest().joinToString("") { "%02x".format(it) }
        if (sha256.isNotEmpty() && seen != sha256) {
            temp.delete()
            t.error = "Файл повредился при передаче. Скачай ещё раз."
            return
        }
        t.uri = if (Build.VERSION.SDK_INT >= 29) {
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, t.name)
                put(MediaStore.Downloads.MIME_TYPE, t.mime)
                put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Relay")
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val resolver = context.contentResolver
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: error("Нет доступа к папке «Загрузки».")
            resolver.openOutputStream(uri)!!.use { o -> temp.inputStream().use { it.copyTo(o) } }
            resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            uri
        } else {
            val dir = context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)!!.apply { mkdirs() }
            val target = File(dir, t.name)
            temp.copyTo(target, overwrite = true)
            FileProvider.getUriForFile(context, context.packageName + ".files", target)
        }
        temp.delete()
    }

    fun fail(t: Transfer, message: String) {
        runCatching { t.out?.close() }
        t.out = null
        t.temp?.delete()
        t.error = message
    }

    fun open(context: Context, t: Transfer): String? {
        val uri = t.uri ?: return null
        val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, t.mime)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        return try {
            context.startActivity(Intent.createChooser(intent, t.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            null
        } catch (e: Exception) {
            "Нет приложения, чтобы открыть ${t.name}. Файл лежит в «Загрузки/Relay»."
        }
    }

    fun share(context: Context, t: Transfer) {
        val uri = t.uri ?: return
        val intent = Intent(Intent.ACTION_SEND).setType(t.mime).putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        context.startActivity(Intent.createChooser(intent, t.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** A link in Claude's answer that points at a file on the server rather than a web page. */
    fun isServerFile(link: String): Boolean {
        val l = link.trim()
        if (l.isEmpty() || l.startsWith("#") || l.startsWith("mailto:") || l.startsWith("tel:")) return false
        if (l.startsWith("file://")) return true
        if (Regex("^[a-zA-Z][a-zA-Z0-9+.-]*://").containsMatchIn(l)) return false
        return l.startsWith("/") || l.startsWith("~/") || l.contains('.') || l.contains('/')
    }

    fun serverPath(link: String) = Uri.decode(link.trim().removePrefix("file://"))
}
