package dev.relay.mobile

import android.content.Context
import com.jcraft.jsch.ChannelExec
import com.jcraft.jsch.HostKey
import com.jcraft.jsch.HostKeyRepository
import com.jcraft.jsch.JSch
import com.jcraft.jsch.Session
import com.jcraft.jsch.UserInfo
import org.json.JSONObject
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.OutputStream
import java.security.MessageDigest
import java.util.Base64
import java.util.concurrent.CompletableFuture
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * One SSH connection running Relay's server helper, the same Python bridge the desktop app
 * uses. Protocol: JSON objects, one per line, both ways.
 */
class Bridge(
    private val context: Context,
    private val server: ServerProfile,
    private val password: String,
    /** Asks the user to trust an unknown host key; returns true to continue. */
    private val trust: (String) -> CompletableFuture<Boolean>,
    private val onEvent: (JSONObject) -> Unit,
    private val onClosed: (String?) -> Unit,
) {
    @Volatile private var session: Session? = null
    @Volatile private var channel: ChannelExec? = null
    @Volatile private var closed = false
    private val outbox = LinkedBlockingQueue<String>()
    var fingerprint: String? = null
        private set

    fun start() {
        Thread({ run() }, "relay-ssh").apply { isDaemon = true }.start()
    }

    fun send(value: JSONObject) {
        if (!closed) outbox.put(value.toString())
    }

    fun close() {
        closed = true
        outbox.put("")
        runCatching { channel?.disconnect() }
        runCatching { session?.disconnect() }
    }

    private fun run() {
        var error: String? = null
        try {
            val jsch = JSch()
            jsch.setHostKeyRepository(Keys(server.fingerprint))
            val s = jsch.getSession(server.user, server.host, server.port)
            s.setPassword(password)
            s.userInfo = userInfo(password)
            s.setConfig("PreferredAuthentications", "password,keyboard-interactive")
            // OpenSSH 10 offers post-quantum hybrids first (mlkem768x25519, sntrup761x25519); with the
            // Bouncy Castle classes Android uses, JSch fails on them, so stay with the classic set.
            s.setConfig("kex", KEX)
            s.serverAliveInterval = 20_000
            s.serverAliveCountMax = 3
            s.connect(15_000)
            session = s
            val helper = helperSource()
            val exec = s.openChannel("exec") as ChannelExec
            exec.setCommand(command(helper.size))
            val input = exec.inputStream
            val output = exec.outputStream
            exec.connect(15_000)
            channel = exec
            output.write(helper)
            output.flush()
            Thread({ writer(output) }, "relay-out").apply { isDaemon = true }.start()
            val reader = BufferedReader(InputStreamReader(input, Charsets.UTF_8), 1 shl 16)
            while (!closed) {
                val line = reader.readLine() ?: break
                if (line.isBlank()) continue
                val value = try { JSONObject(line) } catch (_: Throwable) { continue }
                onEvent(value)
            }
            if (!closed && exec.exitStatus > 0) {
                error = "Не удалось запустить помощник на сервере. Нужны Python 3 и Claude Code."
            }
        } catch (e: Throwable) {
            // Throwable, not Exception: a missing class on some Android version must not kill the app.
            error = when {
                closed -> null
                e is com.jcraft.jsch.JSchChangedHostKeyException || e.message?.contains("has been changed") == true ->
                    "Ключ сервера изменился. Если сервер переустанавливали, нажми «⋮» у сервера → «Забыть ключ» и сверь новый отпечаток."
                e.message?.contains("Auth", ignoreCase = true) == true -> "Сервер не принял логин или пароль."
                e.message?.contains("UnknownHostKey") == true || e.message?.contains("reject HostKey") == true ->
                    "Ключ сервера не подтверждён."
                e.message?.contains("timeout", ignoreCase = true) == true -> "Сервер не ответил. Проверь адрес и порт."
                e !is Exception -> "Ошибка подключения: $e\n" + e.stackTrace.take(6).joinToString("\n") { "  ${it.className}.${it.methodName}:${it.lineNumber}" }
                else -> e.message ?: e.toString()
            }
        } finally {
            closed = true
            runCatching { channel?.disconnect() }
            runCatching { session?.disconnect() }
            onClosed(error)
        }
    }

    private fun writer(output: OutputStream) {
        try {
            while (!closed) {
                val line = outbox.poll(1, TimeUnit.SECONDS) ?: continue
                if (line.isEmpty()) break
                output.write((line + "\n").toByteArray(Charsets.UTF_8))
                output.flush()
            }
        } catch (_: Throwable) {
            close()
        }
    }

    private fun helperSource(): ByteArray =
        helper { name -> context.assets.open("bridge/$name").readBytes().toString(Charsets.UTF_8) }

    /** Trust on first use: a remembered SHA256 fingerprint must match, an unknown one is asked. */
    private inner class Keys(private val expected: String?) : HostKeyRepository {
        override fun check(host: String?, key: ByteArray?): Int {
            val seen = sha256(key ?: return HostKeyRepository.NOT_INCLUDED)
            fingerprint = seen
            if (expected != null) {
                return if (expected == seen) HostKeyRepository.OK else HostKeyRepository.CHANGED
            }
            val answer = runCatching { trust(seen).get(180, TimeUnit.SECONDS) }.getOrDefault(false)
            return if (answer) HostKeyRepository.OK else HostKeyRepository.NOT_INCLUDED
        }
        override fun add(hostkey: HostKey?, ui: UserInfo?) {}
        override fun remove(host: String?, type: String?) {}
        override fun remove(host: String?, type: String?, key: ByteArray?) {}
        override fun getKnownHostsRepositoryID() = "relay"
        override fun getHostKey(): Array<HostKey> = emptyArray()
        override fun getHostKey(host: String?, type: String?): Array<HostKey> = emptyArray()
    }

    companion object {
        const val KEX = "curve25519-sha256,curve25519-sha256@libssh.org,ecdh-sha2-nistp256,ecdh-sha2-nistp384," +
            "ecdh-sha2-nistp521,diffie-hellman-group-exchange-sha256,diffie-hellman-group16-sha512,diffie-hellman-group14-sha256"

        /** DESKTOP_SOURCE = "...", ... then relay_bridge.py, exactly as the desktop builds it. */
        fun helper(read: (String) -> String): ByteArray = buildString {
            append("DESKTOP_SOURCE = ").append(pythonString(read("desktop_mcp.py"))).append('\n')
            append("BOARD_SOURCE = ").append(pythonString(read("board.py"))).append('\n')
            append("CLUSTER_SOURCE = ").append(pythonString(read("cluster_mcp.py"))).append('\n')
            append("ATTACHMENTS_SOURCE = ").append(pythonString(read("attachments.py"))).append('\n')
            append(read("relay_bridge.py"))
        }.toByteArray(Charsets.UTF_8)

        /** The source travels over stdin, so its size never hits the shell's argument limit. */
        fun command(size: Int) =
            "python3 -u -c " + shellQuote("import sys; exec(compile(sys.stdin.buffer.read($size), '<relay>', 'exec'))")

        /**
         * Inside `object : UserInfo` a bare `password` means the object's own getPassword(), which
         * then calls itself forever: the secret comes in under another name.
         */
        fun userInfo(secret: String): UserInfo = object : UserInfo {
            override fun getPassphrase(): String? = null
            override fun getPassword(): String = secret
            override fun promptPassword(message: String?) = true
            override fun promptPassphrase(message: String?) = false
            override fun promptYesNo(message: String?) = false
            override fun showMessage(message: String?) {}
        }

        fun sha256(blob: ByteArray): String =
            "SHA256:" + Base64.getEncoder().withoutPadding().encodeToString(MessageDigest.getInstance("SHA-256").digest(blob))

        fun shellQuote(s: String) = "'" + s.replace("'", "'\\''") + "'"

        /** A JSON string that is also a valid Python literal: unlike org.json, `/` stays as is. */
        fun pythonString(s: String): String {
            val out = StringBuilder(s.length + 16).append('"')
            for (c in s) {
                when {
                    c == '"' -> out.append("\\\"")
                    c == '\\' -> out.append("\\\\")
                    c == '\n' -> out.append("\\n")
                    c == '\r' -> out.append("\\r")
                    c == '\t' -> out.append("\\t")
                    c < ' ' -> out.append(String.format("\\u%04x", c.code))
                    else -> out.append(c)
                }
            }
            return out.append('"').toString()
        }
    }
}
