package dev.relay.mobile

import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowLooper
import java.io.File
import java.util.Base64

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class AttachTest {
    @Test fun photoAndFileGoWithTheMessage() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Test", host = "example"))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4))
        vm.newChat()
        val c = vm.chat()!!
        val png = File(context.cacheDir, "shot.png").apply {
            val bitmap = android.graphics.Bitmap.createBitmap(8, 8, android.graphics.Bitmap.Config.ARGB_8888)
            outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
        }
        val log = File(context.cacheDir, "latest.log").apply { writeText("[Server thread/ERROR]: crash") }
        vm.pick(listOf(Uri.fromFile(png), Uri.fromFile(log)))
        val end = System.currentTimeMillis() + 5000
        while (c.picked.size < 2 && System.currentTimeMillis() < end) { Thread.sleep(20); ShadowLooper.idleMainLooper() }
        assertEquals(listOf("shot.png", "latest.log"), c.picked.map { it.name })
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }
        c.draft = "Почему упал сервер?"
        vm.submit()
        val prompt = sent.single { it.optString("op") == "prompt" }
        val image = prompt.getJSONArray("images").getJSONObject(0)
        assertEquals("base64", image.getString("type"))
        assertEquals("image/png", image.getString("media_type"))
        assertTrue(Base64.getDecoder().decode(image.getString("data")).contentEquals(png.readBytes()))
        val file = prompt.getJSONArray("files").getJSONObject(0)
        assertEquals("file", file.getString("type"))
        assertEquals("latest.log", file.getString("name"))
        assertEquals("[Server thread/ERROR]: crash", String(Base64.getDecoder().decode(file.getString("data"))))
        assertTrue(c.picked.isEmpty())
        val bubble = c.transcript.messages.last { it.role == "user" }
        assertEquals(1, bubble.images)
        assertEquals(listOf("latest.log"), bubble.files)
    }
}
