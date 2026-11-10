package dev.relay.mobile

import android.app.Application
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.CompletableFuture

enum class Screen { Servers, Sessions, Chat, Settings }
enum class Link { Off, Connecting, Online }

class Chat(val sid: String, cwd: String, title: String, model: String, mode: String, effort: String) {
    var cwd by mutableStateOf(cwd)
    var title by mutableStateOf(title)
    var model by mutableStateOf(model)
    var mode by mutableStateOf(mode)
    var effort by mutableStateOf(effort)
    var draft by mutableStateOf("")
    var loading by mutableStateOf(false)
    var loaded by mutableStateOf(false)
    /** Bumped after every change of the transcript, which is not itself observable. */
    var version by mutableIntStateOf(0)
    val transcript = Transcript()
    val permissions = mutableStateListOf<Permission>()
    /** Sent while Claude was answering: Claude Code answers them in order, one after another. */
    val queued = mutableStateListOf<String>()
    fun changed() {
        version++
    }
}

data class TrustRequest(val server: String, val fingerprint: String, val answer: CompletableFuture<Boolean>)

class RelayViewModel(app: Application) : AndroidViewModel(app) {
    private val store = Store(app)
    private val main = Handler(Looper.getMainLooper())
    private var bridge: Bridge? = null
    private var poller: Runnable? = null

    var theme by mutableStateOf(store.theme)
        private set
    var actor by mutableStateOf(store.actor)
        private set
    var servers by mutableStateOf(store.servers)
        private set
    var screen by mutableStateOf(Screen.Servers)
    var current by mutableStateOf<ServerProfile?>(null)
        private set
    var link by mutableStateOf(Link.Off)
        private set
    var notice by mutableStateOf("")
    var home by mutableStateOf("~")
        private set
    var sessions by mutableStateOf<List<Session>>(emptyList())
        private set
    var folders by mutableStateOf<List<Folder>>(emptyList())
        private set
    var sessionsLoaded by mutableStateOf(false)
        private set
    /** 4 and later: Claude runs in a host process on the server and keeps working without the app. */
    var protocol by mutableIntStateOf(0)
        private set
    var running by mutableStateOf<Set<String>>(emptySet())
        private set
    var models by mutableStateOf<List<ModelInfo>>(emptyList())
        private set
    var commands by mutableStateOf<List<CommandInfo>>(emptyList())
        private set
    var claudeVersion by mutableStateOf("")
        private set
    var updating by mutableStateOf(false)
        private set
    val transfers = mutableStateListOf<Transfer>()
    var collapsed by mutableStateOf(store.collapsed)
        private set
    var trust by mutableStateOf<TrustRequest?>(null)
        private set
    val chats = mutableStateMapOf<String, Chat>()
    var selected by mutableStateOf<String?>(null)
        private set

    fun chat(): Chat? = selected?.let { chats[it] }

    fun chooseTheme(id: String) {
        theme = id
        store.theme = id
    }

    fun chooseActor(name: String) {
        actor = name
        store.actor = name
    }

    fun savedPassword(server: ServerProfile) = store.password(server.id)

    fun saveServer(profile: ServerProfile, password: String?, remember: Boolean) {
        val list = servers.toMutableList()
        val at = list.indexOfFirst { it.id == profile.id }
        if (at >= 0) list[at] = profile else list.add(profile)
        servers = list
        store.servers = list
        if (password != null) store.setPassword(profile.id, if (remember) password else null)
    }

    fun deleteServer(profile: ServerProfile) {
        if (current?.id == profile.id) disconnect()
        servers = servers.filter { it.id != profile.id }
        store.servers = servers
        store.setPassword(profile.id, null)
    }

    fun answerTrust(ok: Boolean) {
        trust?.answer?.complete(ok)
        trust = null
    }

    fun connect(profile: ServerProfile, password: String) {
        disconnect()
        current = profile
        link = Link.Connecting
        notice = ""
        sessions = emptyList()
        folders = emptyList()
        sessionsLoaded = false
        chats.clear()
        selected = null
        screen = Screen.Sessions
        lateinit var b: Bridge
        b = Bridge(
            getApplication(),
            profile,
            password,
            trust = { fingerprint ->
                val answer = CompletableFuture<Boolean>()
                main.post { trust = TrustRequest(profile.name.ifEmpty { profile.host }, fingerprint, answer) }
                answer
            },
            onEvent = { value -> main.post { if (bridge === b) safely { handle(value) } } },
            onClosed = { error -> main.post { if (bridge === b) closed(error) } },
        )
        bridge = b
        b.start()
    }

    /** One bad event from the server shows a message instead of closing the app. */
    private fun safely(block: () -> Unit) {
        try {
            block()
        } catch (e: Throwable) {
            android.util.Log.e("Relay", "event", e)
            notice = "Не удалось обработать ответ сервера: $e"
        }
    }

    /** Text of the last crash, shown once on the next start so it can be sent to the developer. */
    var crash by mutableStateOf(Crashes.take(app))

    /** Tests and previews: a server without a connection. */
    internal fun attach(profile: ServerProfile) {
        current = profile
        screen = Screen.Sessions
    }

    fun disconnect() {
        bridge?.close()
        bridge = null
        poller?.let { main.removeCallbacks(it) }
        poller = null
        link = Link.Off
    }

    private fun closed(error: String?) {
        if (bridge == null) return
        // Remember the key the user just trusted, so the next connection checks it.
        val seen = bridge?.fingerprint
        current?.let { c ->
            if (c.fingerprint == null && seen != null && link == Link.Online) {
                val trusted = c.copy(fingerprint = seen)
                current = trusted
                saveServer(trusted, null, false)
            }
        }
        bridge = null
        link = Link.Off
        chats.values.forEach { c ->
            if (c.transcript.busy) {
                c.transcript.busy = false
                c.transcript.error = "Подключение прервалось. Подключись снова, чтобы продолжить."
                c.changed()
            }
        }
        if (!error.isNullOrBlank()) notice = error
    }

    private fun send(value: JSONObject) {
        bridge?.send(value)
    }

    internal fun handle(value: JSONObject) {
        val sid = value.optString("sid")
        when (value.optString("type")) {
            "hello" -> {
                link = Link.Online
                home = value.optString("home", "~")
                protocol = value.optInt("protocol", 0)
                claudeVersion = value.optString("version")
                val c = current
                if (c != null && c.fingerprint == null && bridge?.fingerprint != null) {
                    val trusted = c.copy(fingerprint = bridge?.fingerprint, cwd = if (c.cwd == "~") home else c.cwd)
                    current = trusted
                    saveServer(trusted, null, false)
                }
                send(JSONObject().put("op", "sessions"))
                selected?.let { s -> send(JSONObject().put("op", "history").put("sid", s)) }
                schedulePoll()
            }
            "sessions" -> {
                val rows = value.optJSONArray("sessions") ?: JSONArray()
                sessions = (0 until rows.length()).mapNotNull { rows.optJSONObject(it) }.map {
                    Session(it.optString("id"), it.optString("title"), it.optString("cwd"), it.optDouble("modified", 0.0),
                        it.optDouble("pinned", 0.0).let { p -> if (p.isNaN()) 0.0 else p }, it.optString("folder"), it.optString("auto_title"),
                        it.optBoolean("running"))
                }.filter { it.id.isNotEmpty() }.distinctBy { it.id }
                if (protocol >= 4) running = sessions.filter { it.running }.map { it.id }.toSet()
                val f = value.optJSONArray("folders")
                if (f != null) folders = (0 until f.length()).mapNotNull { f.optJSONObject(it) }.map { Folder(it.optString("id"), it.optString("name")) }
                sessionsLoaded = true
                sessions.forEach { s -> chats[s.id]?.let { c -> c.title = s.title; c.cwd = s.cwd } }
            }
            "history" -> chats[sid]?.let { c ->
                if (!c.transcript.busy) {
                    c.transcript.loadHistory(value.optJSONArray("messages") ?: JSONArray())
                    c.loading = false
                    c.loaded = true
                    // Opened while Claude is still answering, maybe started from another device.
                    if (sid in running) {
                        c.transcript.busy = true
                        c.transcript.userAt = c.transcript.messages.indexOfLast { it.role == "user" }.coerceAtLeast(0)
                        c.transcript.status = "Работает"
                    }
                    c.changed()
                }
            }
            "claude" -> chats[sid]?.let { c ->
                val data = value.optJSONObject("data") ?: return@let
                c.transcript.reduce(data)
                if (data.optString("type") == "result" && c.queued.isNotEmpty()) {
                    // The next queued message is the one Claude answers now.
                    c.transcript.userAt = c.transcript.messages.size
                    c.transcript.messages.add(Message("user", c.queued.removeAt(0)))
                    c.transcript.busy = true
                    c.transcript.status = "Думает над ответом"
                }
                c.changed()
                if (data.optString("type") == "result") send(JSONObject().put("op", "sessions"))
            }
            "catalog" -> {
                models = value.optJSONArray("models")?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map {
                    ModelInfo(it.optString("value"), it.optString("displayName"), it.optString("description"))
                } }.orEmpty().filter { it.value.isNotEmpty() }
                commands = value.optJSONArray("commands")?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map {
                    CommandInfo(it.optString("name"), it.optString("description"), it.optString("hint"))
                } }.orEmpty().filter { it.name.isNotEmpty() }
            }
            "run_state" -> {
                val busy = value.optBoolean("busy")
                running = if (busy) running + sid else running - sid
                chats[sid]?.let { c ->
                    if (busy && !c.transcript.busy && c.loaded) {
                        c.transcript.busy = true
                        if (c.transcript.status.isEmpty()) c.transcript.status = "Работает"
                        c.changed()
                    } else if (!busy && c.transcript.busy) {
                        c.transcript.busy = false
                        c.transcript.status = ""
                        c.queued.clear()
                        c.changed()
                    }
                }
            }
            "permission" -> chats[sid]?.permissions?.add(
                Permission(value.optString("id"), value.optString("tool", "Действие"), value.optJSONObject("input") ?: JSONObject())
            )
            "permission_cancelled" -> chats[sid]?.permissions?.removeAll { it.id == value.optString("id") }
            "run_closed" -> chats[sid]?.let { c ->
                if (c.transcript.busy) {
                    c.transcript.error = value.optString("detail").ifEmpty { "Claude остановлен. История сохранена, можно продолжить." }
                }
                c.transcript.busy = false
                c.transcript.status = ""
                c.permissions.clear()
                c.queued.clear()
                c.changed()
            }
            "error" -> {
                val c = chats[sid]
                if (c != null) {
                    c.transcript.error = value.optString("message", "Не удалось выполнить действие.")
                    if (value.optString("op") == "prompt") c.transcript.busy = false
                    if (value.optString("op") == "history") c.loading = false
                    c.changed()
                } else {
                    notice = value.optString("message")
                }
            }
            "session_deleted" -> {
                sessions = sessions.filter { it.id != sid }
                chats.remove(sid)
                if (selected == sid) {
                    selected = null
                    screen = Screen.Sessions
                }
            }
            "model_changed" -> chats[sid]?.let { it.model = value.optString("model", "default") }
            "mode_changed" -> chats[sid]?.let { it.mode = value.optString("mode", "manual") }
            "notice" -> notice = value.optString("message")
            "claude_update" -> {
                updating = value.optBoolean("running")
                if (value.has("version")) claudeVersion = value.optString("version")
                if (!updating) notice = when {
                    value.optBoolean("changed") -> "Claude Code на сервере обновлён: ${value.optString("before")} → ${value.optString("version")}"
                    value.optBoolean("ok") -> "Claude Code уже последней версии: ${value.optString("version")}"
                    else -> "Не удалось обновить Claude Code: " + value.optString("output").takeLast(400)
                }
            }
            "artifact_info" -> transfers.firstOrNull { it.id == value.optString("id") }?.let { t ->
                val error = value.optString("error")
                if (error.isNotEmpty()) Downloads.fail(t, error)
                else runCatching {
                    Downloads.begin(getApplication(), t, value.optString("name"), value.optLong("size"))
                    send(JSONObject().put("op", "file_stream").put("id", t.id))
                }.onFailure { Downloads.fail(t, it.message ?: "Не удалось начать загрузку.") }
            }
            "desktop_chunk" -> transfers.firstOrNull { it.id == value.optString("id") }?.let { t ->
                runCatching { Downloads.chunk(t, value.optString("data")) }.onFailure {
                    Downloads.fail(t, "Не хватает места на телефоне.")
                    send(JSONObject().put("op", "file_cancel").put("id", t.id))
                }
            }
            "desktop_end" -> transfers.firstOrNull { it.id == value.optString("id") }?.let { t ->
                val error = value.optString("error")
                if (error.isNotEmpty()) Downloads.fail(t, error)
                else runCatching { Downloads.finish(getApplication(), t, value.optString("sha256")) }
                    .onFailure { Downloads.fail(t, it.message ?: "Не удалось сохранить файл.") }
            }
            "fatal" -> notice = value.optString("message", "Не удалось запустить помощник.")
        }
    }

    private fun schedulePoll() {
        poller?.let { main.removeCallbacks(it) }
        val r = object : Runnable {
            override fun run() {
                if (link == Link.Online) send(JSONObject().put("op", "sessions"))
                main.postDelayed(this, 20_000)
            }
        }
        poller = r
        main.postDelayed(r, 20_000)
    }

    fun refresh() {
        send(JSONObject().put("op", "sessions"))
        chat()?.let { c -> if (!c.transcript.busy) send(JSONObject().put("op", "history").put("sid", c.sid)) }
    }

    fun open(session: Session) {
        val s = current ?: return
        val c = chats.getOrPut(session.id) { Chat(session.id, session.cwd, session.title, s.model, s.mode, s.effort) }
        if (!c.transcript.busy) {
            c.loading = !c.loaded
            send(JSONObject().put("op", "history").put("sid", session.id))
        }
        selected = session.id
        screen = Screen.Chat
    }

    fun newChat() {
        val s = current ?: return
        val sid = UUID.randomUUID().toString()
        val c = Chat(sid, if (s.cwd == "~") home else s.cwd, "Новая сессия", s.model, s.mode, s.effort)
        c.loaded = true
        chats[sid] = c
        selected = sid
        screen = Screen.Chat
    }

    /** Can a message go now? While Claude answers, only a server with the host queues it. */
    fun canSend(c: Chat) = link == Link.Online && (!c.transcript.busy || protocol >= 4)

    /** `/model`, `/mode` and `/stop` act at once, also mid-answer; other commands go to Claude Code. */
    private fun localCommand(c: Chat, text: String): Boolean {
        val name = text.substringBefore(' ')
        val arg = text.substringAfter(' ', "").trim()
        when {
            name == "/model" && arg.isNotEmpty() -> {
                val value = resolveModel(models, arg)
                if (value == null) {
                    c.transcript.error = "Нет такой модели: $arg. Доступны: " + models.joinToString { it.value }
                } else {
                    c.model = value
                    send(JSONObject().put("op", "control").put("sid", c.sid).put("action", "model").put("value", value))
                    notice = "Модель: " + modelLabel(models, value)
                }
            }
            name == "/mode" && arg.isNotEmpty() -> {
                c.mode = when (arg) { "auto", "авто" -> "auto"; "plan", "план" -> "plan"; "edits", "acceptEdits", "правки" -> "acceptEdits"; else -> "manual" }
                send(JSONObject().put("op", "control").put("sid", c.sid).put("action", "mode").put("value", c.mode))
            }
            name == "/stop" -> stop()
            else -> return false
        }
        c.draft = ""
        c.changed()
        return true
    }

    /** Model or mode picked while Claude answers: switch it live. */
    fun liveChange(c: Chat) {
        if (protocol >= 4 && c.transcript.busy) {
            send(JSONObject().put("op", "control").put("sid", c.sid).put("action", "model").put("value", c.model))
            send(JSONObject().put("op", "control").put("sid", c.sid).put("action", "mode").put("value", c.mode))
        }
    }

    fun submit() {
        val c = chat() ?: return
        val text = c.draft.trim()
        if (text.isEmpty() || link != Link.Online) return
        if (localCommand(c, text)) return
        if (!canSend(c)) return
        c.transcript.error = ""
        if (c.transcript.busy) {
            c.queued.add(text)
        } else {
            c.transcript.userAt = c.transcript.messages.size
            c.transcript.messages.add(Message("user", text))
            c.transcript.busy = true
            c.transcript.status = "Отправляем"
        }
        if (c.title == "Новая сессия") c.title = text.take(60)
        c.draft = ""
        c.changed()
        send(
            JSONObject().put("op", "prompt").put("sid", c.sid).put("cwd", c.cwd).put("text", text)
                .put("images", JSONArray()).put("files", JSONArray()).put("model", c.model).put("mode", c.mode)
                .put("effort", c.effort).put("actor", actor.ifBlank { "Relay Android" }).put("desktop_enabled", false)
                .put("desktop_context", JSONObject())
        )
        current?.let { s ->
            if (s.model != c.model || s.mode != c.mode || s.effort != c.effort) {
                val updated = s.copy(model = c.model, mode = c.mode, effort = c.effort)
                current = updated
                saveServer(updated, null, false)
            }
        }
    }

    /** A file Claude mentioned: fetched over the same connection into Downloads/Relay. */
    fun download(path: String, cwd: String) {
        if (link != Link.Online) {
            notice = "Подключись к серверу, чтобы скачать файл."
            return
        }
        if (transfers.count { !it.done && it.error.isEmpty() } >= 4) {
            notice = "Уже скачивается несколько файлов. Дождись их."
            return
        }
        val t = Transfer(UUID.randomUUID().toString(), path)
        transfers.add(0, t)
        send(JSONObject().put("op", "file_info").put("id", t.id).put("path", path).put("cwd", cwd))
    }

    fun dismiss(t: Transfer) {
        if (!t.done && t.error.isEmpty()) send(JSONObject().put("op", "file_cancel").put("id", t.id))
        Downloads.fail(t, "")
        transfers.remove(t)
    }

    fun openTransfer(t: Transfer) {
        Downloads.open(getApplication(), t)?.let { notice = it }
    }

    fun shareTransfer(t: Transfer) = runCatching { Downloads.share(getApplication(), t) }

    fun updateClaude() {
        updating = true
        send(JSONObject().put("op", "claude_update"))
    }

    fun stop() {
        val c = chat() ?: return
        send(JSONObject().put("op", "stop").put("sid", c.sid))
        c.transcript.status = "Останавливаем"
        c.changed()
    }

    fun permit(permission: Permission, allow: Boolean) {
        val c = chat() ?: return
        send(JSONObject().put("op", "permission").put("sid", c.sid).put("id", permission.id).put("allow", allow))
        c.permissions.remove(permission)
    }

    // Shared session names, pins and folders (stored on the server for everyone).
    private fun meta(session: Session, change: JSONObject.() -> Unit) =
        send(JSONObject().put("op", "session_meta").put("sid", session.id).put("actor", actor.ifBlank { "Relay Android" }).apply(change))

    fun pin(session: Session, on: Boolean) = meta(session) { put("pinned", on) }
    fun rename(session: Session, title: String) = meta(session) { put("title", title.trim()) }
    fun moveTo(session: Session, folder: String) = meta(session) { put("folder", folder) }
    fun delete(session: Session) = send(JSONObject().put("op", "delete_session").put("sid", session.id))
    fun createFolder(name: String, session: Session? = null) =
        send(JSONObject().put("op", "session_folder").put("name", name.trim()).put("sid", session?.id ?: JSONObject.NULL).put("actor", actor))
    fun renameFolder(folder: Folder, name: String) = send(JSONObject().put("op", "session_folder").put("id", folder.id).put("name", name.trim()))
    fun deleteFolder(folder: Folder) = send(JSONObject().put("op", "session_folder").put("id", folder.id).put("delete", true))
    fun toggleFolder(folder: Folder) {
        collapsed = if (folder.id in collapsed) collapsed - folder.id else collapsed + folder.id
        store.collapsed = collapsed
    }

    override fun onCleared() {
        disconnect()
    }
}
