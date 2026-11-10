package dev.relay.mobile

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.io.File
import java.nio.file.Files
import java.util.concurrent.TimeUnit

/** The real server helper sends a file; the phone's download code checks and saves it. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class RealDownloadTest {
    @Test fun helperFileArrivesIntact() {
        val bridgeDir = generateSequence(File("").absoluteFile) { it.parentFile }.map { File(it, "bridge") }.first { File(it, "relay_bridge.py").exists() }
        val helper = Bridge.helper { name -> File(bridgeDir, name).readText() }
        val home = Files.createTempDirectory("relay-home").toFile()
        val file = File(home, "build/libs/sbrp-client-0.7.300.jar").apply { parentFile.mkdirs(); writeBytes(ByteArray(400_000) { (it * 7 % 256).toByte() }) }
        val process = ProcessBuilder("sh", "-c", Bridge.command(helper.size)).apply {
            environment()["HOME"] = home.path
            environment()["RELAY_IN_PROCESS"] = "1"
            environment()["RELAY_BOARD_DIR"] = File(home, "board").path
        }.start()
        Thread { process.errorStream.readBytes() }.start()
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Test", host = "example"))
        vm.handle(JSONObject().put("type", "hello").put("home", home.path).put("protocol", 4))
        vm.download("sbrp-client-0.7.300.jar", home.path) // A bare name is looked up in build/libs.
        val t = vm.transfers.single()
        val out = process.outputStream
        out.write(helper)
        out.write((JSONObject().put("op", "file_info").put("id", t.id).put("path", t.path).put("cwd", home.path).toString() + "\n").toByteArray())
        out.flush()
        val reader = process.inputStream.bufferedReader()
        val deadline = System.currentTimeMillis() + 20_000
        while (System.currentTimeMillis() < deadline && !t.done && t.error.isEmpty()) {
            val value = JSONObject(reader.readLine() ?: break)
            vm.handle(value)
            // The phone asks for the stream once it knows the file; here the test plays the SSH link.
            if (value.optString("type") == "artifact_info") {
                out.write((JSONObject().put("op", "file_stream").put("id", t.id).toString() + "\n").toByteArray()); out.flush()
            }
        }
        out.close()
        process.waitFor(10, TimeUnit.SECONDS)
        process.destroy()
        assertEquals("", t.error)
        assertTrue(t.done)
        assertEquals("sbrp-client-0.7.300.jar", t.name)
        assertEquals(file.length(), t.received)
    }
}
