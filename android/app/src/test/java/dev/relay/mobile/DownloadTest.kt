package dev.relay.mobile

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.security.MessageDigest
import java.util.Base64

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class DownloadTest {
    @Test fun serverLinksAreFilesWebLinksAreNot() {
        assertTrue(Downloads.isServerFile("/opt/sbrp/build/libs/sbrp-client-0.7.300.jar"))
        assertTrue(Downloads.isServerFile("build/libs/mod.jar"))
        assertTrue(Downloads.isServerFile("file:///root/out.zip"))
        assertFalse(Downloads.isServerFile("https://github.com/daexorn/relay-desktop"))
        assertFalse(Downloads.isServerFile("mailto:a@b.c"))
        assertEquals("/root/мой файл.txt", Downloads.serverPath("/root/%D0%BC%D0%BE%D0%B9%20%D1%84%D0%B0%D0%B9%D0%BB.txt"))
        assertEquals("application/vnd.android.package-archive", Downloads.mime("Relay.apk"))
    }

    @Test fun fileArrivesInChunksIsCheckedAndSaved() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Test", host = "example"))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4))
        vm.download("/opt/mod.jar", "/opt")
        val t = vm.transfers.single()
        val body = ByteArray(300_000) { (it % 251).toByte() }
        vm.handle(JSONObject().put("type", "artifact_info").put("id", t.id).put("name", "mod.jar").put("size", body.size))
        for ((i, part) in body.toList().chunked(131072).withIndex()) {
            vm.handle(JSONObject().put("type", "desktop_chunk").put("id", t.id).put("sequence", i).put("data", Base64.getEncoder().encodeToString(part.toByteArray())))
        }
        assertEquals(body.size.toLong(), t.received)
        val sha = MessageDigest.getInstance("SHA-256").digest(body).joinToString("") { "%02x".format(it) }
        vm.handle(JSONObject().put("type", "desktop_end").put("id", t.id).put("size", body.size).put("sha256", sha))
        assertEquals("", t.error)
        assertTrue(t.done)
        // Robolectric cannot read MediaStore back; the row is created in Downloads.
        assertTrue(t.uri.toString(), t.uri.toString().startsWith("content://media/external"))
    }

    @Test fun damagedFileIsRejected() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Test", host = "example"))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4))
        vm.download("/opt/a.txt", "/opt")
        val t = vm.transfers.single()
        vm.handle(JSONObject().put("type", "artifact_info").put("id", t.id).put("name", "a.txt").put("size", 3))
        vm.handle(JSONObject().put("type", "desktop_chunk").put("id", t.id).put("data", Base64.getEncoder().encodeToString("abc".toByteArray())))
        vm.handle(JSONObject().put("type", "desktop_end").put("id", t.id).put("sha256", "00"))
        assertFalse(t.done)
        assertTrue(t.error.contains("повредился"))
    }
}
