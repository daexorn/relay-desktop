package dev.relay.mobile

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.TimeUnit

/** Runs the real server helper the way the phone starts it over SSH, minus the SSH. */
class BridgeProtocolTest {
    private val bridgeDir = generateSequence(File("").absoluteFile) { it.parentFile }
        .map { File(it, "bridge") }.first { File(it, "relay_bridge.py").exists() }

    @Test fun pythonStringKeepsSlashesAndEscapesControls() {
        assertEquals("\"a/b \\\"q\\\" \\\\ \\n \\u0001 я\"", Bridge.pythonString("a/b \"q\" \\ \n \u0001 я"))
    }

    @Test fun helperStartsAndSpeaksTheProtocol() {
        val helper = Bridge.helper { name -> File(bridgeDir, name).readText() }
        val home = Files.createTempDirectory("relay-home").toFile()
        val process = ProcessBuilder("sh", "-c", Bridge.command(helper.size)).apply {
            environment()["HOME"] = home.path
            environment()["RELAY_IN_PROCESS"] = "1"
            environment()["RELAY_BOARD_DIR"] = File(home, "board").path
            redirectErrorStream(false)
        }.start()
        val errors = StringBuffer()
        Thread { process.errorStream.bufferedReader().forEachLine { errors.append(it).append('\n') } }.start()
        val out = process.outputStream
        out.write(helper)
        out.write((JSONObject().put("op", "sessions").toString() + "\n").toByteArray())
        out.write((JSONObject().put("op", "history").put("sid", "../../etc/passwd").toString() + "\n").toByteArray())
        out.flush()
        val reader = process.inputStream.bufferedReader()
        val seen = mutableMapOf<String, JSONObject>()
        val deadline = System.currentTimeMillis() + 20_000
        while (System.currentTimeMillis() < deadline && seen.keys.containsAll(listOf("hello", "sessions", "error")).not()) {
            val line = reader.readLine() ?: break
            val value = JSONObject(line)
            seen.putIfAbsent(value.optString("type"), value)
        }
        out.close()
        process.waitFor(10, TimeUnit.SECONDS)
        process.destroy()
        val stderr = errors.toString()
        assertTrue("no hello: $seen\n$stderr", seen.containsKey("hello"))
        assertEquals(home.path, seen["hello"]!!.optString("home"))
        assertEquals(0, seen["sessions"]!!.getJSONArray("sessions").length())
        assertTrue(seen["sessions"]!!.has("folders"))
        assertEquals("history", seen["error"]!!.optString("op"))
        assertTrue("python warnings: $stderr", !stderr.contains("SyntaxWarning") && !stderr.contains("Traceback"))
    }
}
