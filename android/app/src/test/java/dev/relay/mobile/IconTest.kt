package dev.relay.mobile

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Path
import android.graphics.drawable.AdaptiveIconDrawable
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** The launcher icon is adaptive, so launchers round it; saves it as a circle and a squircle for a look. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "xxxhdpi")
class IconTest {
    @Test
    fun launcherIconIsAdaptive() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val icon = context.getDrawable(R.mipmap.ic_launcher)
        assertTrue("icon is $icon", icon is AdaptiveIconDrawable)
        icon as AdaptiveIconDrawable
        val size = 432
        val out = File("build/screens").apply { mkdirs() }
        for ((name, radius) in listOf("circle" to size / 2f, "squircle" to size * 0.3f)) {
            val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            canvas.clipPath(Path().apply { addRoundRect(0f, 0f, size.toFloat(), size.toFloat(), radius, radius, Path.Direction.CW) })
            // The full 108dp layers; launchers show the middle 72dp.
            val pad = size / 4
            for (layer in listOf(icon.background, icon.foreground)) {
                layer.setBounds(-pad, -pad, size + pad, size + pad)
                layer.draw(canvas)
            }
            File(out, "icon-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
    }
}
