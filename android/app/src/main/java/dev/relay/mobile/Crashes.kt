package dev.relay.mobile

import android.content.Context
import android.os.Build
import java.io.File

/** Keeps the stack trace of a crash so the next start can show it and the user can copy it. */
object Crashes {
    private fun file(context: Context) = File(context.filesDir, "last-crash.txt")

    fun install(context: Context) {
        val app = context.applicationContext
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching {
                val version = runCatching { app.packageManager.getPackageInfo(app.packageName, 0).versionName }.getOrNull()
                file(app).writeText(
                    "Relay $version · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}) · ${Build.MANUFACTURER} ${Build.MODEL}\n" +
                        "Поток: ${thread.name}\n\n" + error.stackTraceToString().take(20_000)
                )
            }
            previous?.uncaughtException(thread, error)
        }
    }

    /** The saved report, removed once read. */
    fun take(context: Context): String = runCatching {
        val f = file(context)
        if (!f.exists()) "" else f.readText().also { f.delete() }
    }.getOrDefault("")
}
