package dev.relay.mobile

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.content.FileProvider
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

internal object Updates {
    private const val downloadRoot = "https://github.com/daexorn/relay-desktop/releases/latest/download/"
    private const val maxBytes = 64 * 1024 * 1024L
    private val apkName = Regex("""Relay-android-([0-9]+\.[0-9]+\.[0-9]+)\.apk""")

    internal fun newer(candidate: String, current: String): Boolean {
        fun parts(value: String): List<Int>? {
            val items = value.split('.')
            if (items.size != 3) return null
            return items.map { it.toIntOrNull() ?: return null }
        }
        val next = parts(candidate) ?: return false
        val installed = parts(current) ?: return false
        val first = next.indices.firstOrNull { next[it] != installed[it] } ?: return false
        return next[first] > installed[first]
    }

    private fun connection(url: String) = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 10000
        readTimeout = 20000
        setRequestProperty("User-Agent", "Relay-Android")
    }

    @Suppress("DEPRECATION")
    @Synchronized fun check(context: Context): File? {
        val pm = context.packageManager
        val installed = pm.getPackageInfo(context.packageName, PackageManager.GET_SIGNATURES)
        val dir = File(context.cacheDir, "updates").apply { mkdirs() }
        val target = File(dir, "Relay.apk")
        fun valid(file: File): Boolean {
            val info = pm.getPackageArchiveInfo(file.path, PackageManager.GET_SIGNATURES) ?: return false
            return info.packageName == installed.packageName && info.versionCode > installed.versionCode &&
                !installed.signatures.isNullOrEmpty() && info.signatures?.toSet() == installed.signatures?.toSet()
        }
        if (target.exists() && valid(target)) return target
        val prefs = context.getSharedPreferences("relay-updates", Context.MODE_PRIVATE)
        val now = System.currentTimeMillis()
        if (now - prefs.getLong("checked", 0L) in 0 until 3600000L) return null
        if (now - prefs.getLong("attempt", 0L) in 0 until 120000L) return null
        prefs.edit().putLong("attempt", now).apply()
        val manifest = connection(downloadRoot + "checksums.json")
        val assets = try {
            JSONObject(manifest.inputStream.bufferedReader().use { it.readText() })
        } finally { manifest.disconnect() }
        for (name in assets.keys()) {
            val match = apkName.matchEntire(name) ?: continue
            if (!newer(match.groupValues[1], installed.versionName.orEmpty())) continue
            val asset = assets.getJSONObject(name)
            val url = downloadRoot + name
            val size = asset.getLong("size")
            require(size in 1..maxBytes)
            val tmp = File.createTempFile("relay-", ".apk", dir)
            val download = connection(url)
            try {
                val hash = MessageDigest.getInstance("SHA-256")
                var total = 0L
                download.inputStream.use { input -> tmp.outputStream().use { output ->
                    val buffer = ByteArray(65536)
                    while (true) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        total += n
                        require(total <= size)
                        hash.update(buffer, 0, n)
                        output.write(buffer, 0, n)
                    }
                } }
                require(total == size && hash.digest().joinToString("") { "%02x".format(it) } == asset.getString("sha256"))
                require(valid(tmp) && tmp.renameTo(target))
                return target
            } finally { download.disconnect(); tmp.delete() }
        }
        prefs.edit().putLong("checked", now).apply()
        return null
    }

    fun install(context: Context, apk: File) {
        val uri = FileProvider.getUriForFile(context, context.packageName + ".files", apk)
        context.startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri,"application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}
