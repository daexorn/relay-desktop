package dev.relay.mobile

import android.app.Application
import android.graphics.BitmapFactory
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.AndroidViewModel
import org.json.JSONArray
import org.json.JSONObject
import java.util.Base64
import java.util.UUID
import java.util.concurrent.CompletableFuture

enum class Screen { Servers, Sessions, Chat, Settings }
enum class Link { Off, Connecting, Online }

data class TurnSummary(
    val backend: String = "",
    val diff: Pair<Long, Long>? = null,
    val context: Pair<Long, Long>? = null,
)

class Chat(val sid: String, cwd: String, title: String, model: String, mode: String, effort: String) {
    var backend by mutableStateOf("claude")
    var api by mutableStateOf(false)
    var apiModel by mutableStateOf("")
    var codexModel by mutableStateOf("default")
    var agyModel by mutableStateOf("default")
    var cwd by mutableStateOf(cwd)
    var title by mutableStateOf(title)
    var model by mutableStateOf(model)
    var mode by mutableStateOf(mode)
    var effort by mutableStateOf(concreteEffort(effort))
    var draft by mutableStateOf("")
    var turnSummary by mutableStateOf(TurnSummary())
    var loading by mutableStateOf(false)
    var loaded by mutableStateOf(false)
    var locked by mutableStateOf(false)
    var unlocked by mutableStateOf(true)
    var unlocking by mutableStateOf(false)
    var lockError by mutableStateOf("")
    /** Bumped after every change of the transcript, which is not itself observable. */
    var version by mutableIntStateOf(0)
    val transcript = Transcript()
    val permissions = mutableStateListOf<Permission>()
    /** Sent while the agent was answering: answered in order, one after another. */
    val queued = mutableStateListOf<String>()
    /** Written while there was no connection: sent by itself once the server is back. */
    val outbox = mutableStateListOf<String>()
    /** Server `modified` of the session when this chat last matched it. */
    var syncedModified = 0.0
    val picked = mutableStateListOf<Picked>()

    fun isLockedGate(): Boolean = locked && !unlocked

    fun activeBackend(): String = if (api) "api" else backend

    fun selectedModel(profile: ServerProfile? = null): String = when (activeBackend()) {
        "api" -> apiModel.ifBlank { profile?.api?.model.orEmpty() }
        "codex" -> codexModel.ifBlank { profile?.codexModel ?: "default" }
        "agy" -> agyModel.ifBlank { profile?.agyModel ?: "default" }
        else -> model.ifBlank { "default" }
    }

    fun updateSelectedModel(value: String) {
        when (activeBackend()) {
            "api" -> apiModel = value
            "codex" -> codexModel = value
            "agy" -> agyModel = value
            else -> model = value
        }
    }

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
    private var reconnector: Runnable? = null
    private var reconnectAttempts = 0
    private var lastPassword: String? = null

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
    /** 4 and later: agents run in a host process on the server and keep working without the app. */
    var protocol by mutableIntStateOf(0)
        private set
    var running by mutableStateOf<Set<String>>(emptySet())
        private set
    var models by mutableStateOf<List<ModelInfo>>(emptyList())
        private set
    var codexModels by mutableStateOf<List<ModelInfo>>(emptyList())
        private set
    var agyModels by mutableStateOf<List<ModelInfo>>(emptyList())
        private set
    var commands by mutableStateOf<List<CommandInfo>>(emptyList())
        private set
    var codexCommands by mutableStateOf<List<CommandInfo>>(emptyList())
        private set
    var agyCommands by mutableStateOf<List<CommandInfo>>(emptyList())
        private set
    var apiModels by mutableStateOf<List<ModelInfo>>(emptyList())
        private set
    var apiError by mutableStateOf("")
        private set
    var apiChecking by mutableStateOf(false)
        private set
    private var apiRequest = ""
    var claudeVersion by mutableStateOf("")
        private set
    var updating by mutableStateOf(false)
        private set

    // 1-Click CLI Agent Installer & Server Status
    var backendsStatus by mutableStateOf<Map<String, BackendStatus>>(emptyMap())
        private set
    var backendsKnown by mutableStateOf(false)
        private set
    val installingAgents = mutableStateMapOf<String, Boolean>()
    val agentMessages = mutableStateMapOf<String, String>()
    val agentLogs = mutableStateMapOf<String, String>()

    // Server OAuth Login Flow
    var authFlow by mutableStateOf<AuthFlowState?>(null)
        private set
    var browserSetupRunning by mutableStateOf(false)
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
    fun savedApiKey(server: ServerProfile) = store.apiKey(server.id).orEmpty()
    fun apiCatalog(): List<ModelInfo> = apiModels.ifEmpty { current?.api?.models.orEmpty().map { ModelInfo(it, it, "") } }

    fun catalogFor(backend: String): List<ModelInfo> = when (backend) {
        "api" -> apiCatalog()
        "codex" -> codexModels.ifEmpty { defaultBackendModels("codex") }
        "agy" -> agyModels.ifEmpty { defaultBackendModels("agy") }
        else -> models.ifEmpty { defaultBackendModels("claude") }
    }

    fun modelForChat(c: Chat): String =
        concreteModel(c.selectedModel(current), catalogFor(c.activeBackend()), c.activeBackend())

    private fun rememberLockedSessionName(sid: String, title: String) {
        val server = current?.id ?: return
        val oldAlias = "Сессия ${sid.take(8)}"
        if (title.isNotBlank() && title !in setOf("Защищённая сессия", "Новая сессия", oldAlias)
            && store.lockedSessionName(server, sid) in setOf(null, oldAlias)
        ) store.setLockedSessionName(server, sid, title)
    }

    private fun lockListName(sid: String, modified: Double = sessions.firstOrNull { it.id == sid }?.modified ?: 0.0): String =
        current?.let { store.lockedSessionName(it.id, sid) }?.takeUnless { it == "Сессия ${sid.take(8)}" }
            ?: lockedSessionTitle(modified)

    fun commandsFor(backend: String): List<CommandInfo> {
        val base = when (backend) {
            "codex" -> codexCommands
            "agy" -> agyCommands
            "api" -> emptyList()
            else -> commands
        }
        val merged = base.toMutableList()
        for (b in builtinSlashCommands()) {
            if (merged.none { it.name.equals(b.name, ignoreCase = true) }) {
                merged += b
            }
        }
        return merged
    }

    fun setBackend(chat: Chat, target: String) {
        if (target !in BACKENDS) return
        if (chat.transcript.busy) {
            notice = "Дождись завершения ответа перед сменой источника."
            return
        }
        chat.backend = target
        chat.api = target == "api"
        current?.let { s ->
            store.setSessionBackend(s.id, chat.sid, target)
            if (chat.apiModel.isBlank()) chat.apiModel = s.api.model
            if (chat.codexModel.isBlank()) chat.codexModel = s.codexModel
            if (chat.agyModel.isBlank()) chat.agyModel = s.agyModel
        }
        if (target in listOf("codex", "agy") && link == Link.Online) {
            send(JSONObject().put("op", "catalog").put("backend", target))
        }
    }

    fun setApiSource(chat: Chat, enabled: Boolean) {
        setBackend(chat, if (enabled) "api" else "claude")
    }

    fun saveApi(server: ServerProfile, profile: ApiProfile, key: String): Boolean {
        profile.error()?.let { notice = it; return false }
        if (current?.id == server.id && profile.enabled != server.api.enabled && chats.values.any { it.transcript.busy }) {
            notice = "Дождись завершения работы перед сменой источника по умолчанию."
            return false
        }
        if (store.setApiKey(server.id, key).isFailure) {
            notice = "Не удалось сохранить ключ в защищённом хранилище Android."
            return false
        }
        val nextDefault = when {
            profile.enabled -> "api"
            server.defaultBackend == "api" -> ""
            else -> server.defaultBackend
        }
        val updated = server.copy(api = profile, defaultBackend = nextDefault)
        saveServer(updated, null, false)
        if (current?.id == server.id) {
            current = updated
            apiModels = profile.models.map { ModelInfo(it, it, "") }
            chats.values.filter { it.apiModel.isBlank() }.forEach { it.apiModel = profile.model }
        }
        notice = "Настройки API сохранены"
        return true
    }

    fun checkApi(server: ServerProfile, profile: ApiProfile, key: String) {
        if (current?.id != server.id || link != Link.Online) {
            apiError = "Подключись к этому серверу для проверки API."
            return
        }
        apiRequest = UUID.randomUUID().toString()
        apiChecking = true
        apiError = ""
        send(JSONObject().put("op", "api_models").put("request", apiRequest).put("api", profile.request(key)))
    }

    fun saveServer(profile: ServerProfile, password: String?, remember: Boolean) {
        val list = servers.toMutableList()
        val at = list.indexOfFirst { it.id == profile.id }
        if (at >= 0) list[at] = profile else list.add(profile)
        servers = list
        store.servers = list
        if (password != null) store.setPassword(profile.id, if (remember) password else null)
        if (current?.id == profile.id) current = profile
    }

    fun deleteServer(profile: ServerProfile) {
        if (current?.id == profile.id) disconnect()
        servers = servers.filter { it.id != profile.id }
        store.servers = servers
        store.setPassword(profile.id, null)
        store.setApiKey(profile.id, "")
    }

    fun answerTrust(ok: Boolean) {
        trust?.answer?.complete(ok)
        trust = null
    }

    fun connect(profile: ServerProfile, password: String) = connect(profile, password, fresh = true)

    private fun connect(profile: ServerProfile, password: String, fresh: Boolean) {
        disconnect()
        lastPassword = password
        if (!fresh) {
            link = Link.Connecting
            open(profile, password)
            return
        }
        reconnectAttempts = 0
        current = profile
        apiModels = profile.api.models.map { ModelInfo(it, it, "") }
        apiError = ""
        apiChecking = false
        link = Link.Connecting
        notice = ""
        sessions = emptyList()
        folders = emptyList()
        sessionsLoaded = false
        backendsStatus = emptyMap()
        backendsKnown = false
        installingAgents.clear()
        agentMessages.clear()
        agentLogs.clear()
        authFlow = null
        chats.clear()
        selected = null
        screen = Screen.Sessions
        open(profile, password)
    }

    private fun open(profile: ServerProfile, password: String) {
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
        reconnector?.let { main.removeCallbacks(it) }
        reconnector = null
        bridge?.close()
        bridge = null
        poller?.let { main.removeCallbacks(it) }
        poller = null
        authFlow = null
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
        authFlow = null
        link = Link.Off
        chats.values.forEach { c ->
            c.loading = false
            if (c.transcript.busy) {
                c.transcript.busy = false
                c.transcript.status = "Связь прервалась · переподключаемся"
                c.changed()
            }
        }
        scheduleReconnect(error)
    }

    /** A dropped connection comes back by itself; chats, drafts and the outbox stay as they are. */
    private fun scheduleReconnect(error: String?) {
        val profile = current ?: return
        val password = lastPassword ?: store.password(profile.id)
        if (password == null || demo) {
            if (!error.isNullOrBlank()) notice = error
            return
        }
        val delay = longArrayOf(1, 2, 3, 5, 8, 13, 20, 30)[reconnectAttempts.coerceAtMost(7)] * 1000
        reconnectAttempts++
        if (reconnectAttempts > 3 && !error.isNullOrBlank()) notice = "Нет связи: $error · пробуем снова"
        reconnector?.let { main.removeCallbacks(it) }
        val r = Runnable {
            reconnector = null
            if (bridge == null && current?.id == profile.id) connect(current ?: profile, password, fresh = false)
        }
        reconnector = r
        main.postDelayed(r, delay)
    }

    /** Try at once, e.g. when the app comes back to the screen or the user presses send. */
    fun reconnectNow() {
        if (bridge != null || demo || current == null) return
        reconnectAttempts = 0
        val profile = current ?: return
        val password = lastPassword ?: store.password(profile.id) ?: return
        connect(profile, password, fresh = false)
    }

    private fun flushOutbox(c: Chat) {
        if (link != Link.Online || c.outbox.isEmpty() || c.loading || c.isLockedGate()) return
        if (c.transcript.busy && protocol < 4) return
        val keep = c.draft
        while (c.outbox.isNotEmpty() && link == Link.Online) {
            c.draft = c.outbox.removeAt(0)
            submitNow(c)
            if (c.draft.isNotBlank()) {
                c.outbox.add(0, c.draft)
                break
            }
        }
        c.draft = keep
        c.changed()
    }

    /** Tests read what would go to the server. */
    internal var sentForTest: ((JSONObject) -> Unit)? = null

    /** The demo server: everything is local, nothing leaves the phone. */
    var demo by mutableStateOf(false)
        private set

    fun startDemo() {
        disconnect()
        demo = true
        current = ServerProfile(id = "demo", name = "Демо", host = "без сервера", cwd = "/opt/sbrp-client")
        chats.clear()
        selected = null
        sessions = emptyList()
        backendsStatus = emptyMap()
        backendsKnown = false
        link = Link.Online
        screen = Screen.Sessions
        Demo.events().forEach { if (it.optString("type") != "history") handle(it) }
    }

    fun leaveDemo() {
        demo = false
        link = Link.Off
        current = null
        sessions = emptyList()
        backendsStatus = emptyMap()
        backendsKnown = false
        chats.clear()
        screen = Screen.Servers
    }

    private fun send(value: JSONObject) {
        sentForTest?.invoke(value)
        if (demo) {
            demoAnswer(value)
            return
        }
        bridge?.send(value)
    }

    private fun demoAnswer(value: JSONObject) {
        val sid = value.optString("sid")
        when (value.optString("op")) {
            "history" -> main.post {
                val history = Demo.events().firstOrNull { it.optString("type") == "history" && it.optString("sid") == sid }
                handle(history ?: JSONObject().put("type", "history").put("sid", sid).put("messages", JSONArray()))
            }
            "prompt" -> main.postDelayed({
                handle(JSONObject().put("type", "claude").put("sid", sid).put("data", JSONObject().put("type", "assistant")
                    .put("message", JSONObject().put("content", JSONArray().put(JSONObject().put("type", "text").put("text", Demo.reply(value.optString("text"))))))))
                handle(JSONObject().put("type", "claude").put("sid", sid).put("data", JSONObject().put("type", "result").put("result", "")))
            }, 700)
            "file_info" -> main.post {
                transfers.firstOrNull { it.id == value.optString("id") }?.let { Downloads.fail(it, "В демо-режиме файлы не скачиваются — подключи свой сервер.") }
            }
            "stop" -> main.post { handle(JSONObject().put("type", "run_closed").put("sid", sid)) }
            "session_lock" -> main.post {
                val remove = value.optBoolean("remove")
                handle(JSONObject().put("type", "session_lock_changed").put("sid", sid).put("locked", !remove).put("unlocked", true))
            }
            "session_relock" -> main.post {
                handle(JSONObject().put("type", "session_lock_changed").put("sid", sid).put("locked", true).put("unlocked", false))
            }
            "session_unlock" -> main.post {
                handle(JSONObject().put("type", "session_unlock_result").put("sid", sid).put("ok", true))
            }
            "agent_install" -> main.post {
                val agent = value.optString("agent")
                handle(JSONObject().put("type", "agent_install").put("agent", agent).put("running", false).put("ok", true).put("installed", true).put("version", "демо").put("message", "В демо-режиме все агенты уже готовы."))
            }
            "auth_start" -> main.post {
                handle(JSONObject().put("type", "auth").put("id", value.optString("id")).put("provider", value.optString("provider", "claude")).put("state", "failed").put("message", "Для входа в аккаунт подключи свой SSH-сервер."))
            }
        }
    }

    private fun parseBackends(obj: JSONObject?) {
        if (obj == null) return
        val map = mutableMapOf<String, BackendStatus>()
        for (id in BACKENDS) {
            val item = obj.optJSONObject(id) ?: continue
            map[id] = BackendStatus(
                id = id,
                label = item.optString("label").ifBlank { backendLabel(id) },
                installed = item.optBoolean("installed", id == "api"),
                version = item.optString("version"),
                running = item.optBoolean("running", false),
            )
        }
        if (map.isNotEmpty()) {
            backendsStatus = map
            backendsKnown = true
            map["claude"]?.version?.takeIf { it.isNotEmpty() }?.let { claudeVersion = it }
        }
    }

    internal fun handle(value: JSONObject) {
        val sid = value.optString("sid")
        when (value.optString("type")) {
            "hello" -> {
                link = Link.Online
                reconnectAttempts = 0
                home = value.optString("home", "~")
                protocol = value.optInt("protocol", 0)
                claudeVersion = value.optString("version")
                parseBackends(value.optJSONObject("backends"))
                val c = current
                if (c != null && c.fingerprint == null && bridge?.fingerprint != null) {
                    val trusted = c.copy(fingerprint = bridge?.fingerprint, cwd = if (c.cwd == "~") home else c.cwd)
                    current = trusted
                    saveServer(trusted, null, false)
                }
                send(JSONObject().put("op", "sessions"))
                selected?.let { s ->
                    val chat = chats[s]
                    if (chat == null || !chat.isLockedGate()) send(JSONObject().put("op", "history").put("sid", s))
                }
                schedulePoll()
            }
            "backends_status" -> parseBackends(value.optJSONObject("backends"))
            "agent_install" -> {
                val agent = value.optString("agent")
                val isRunning = value.optBoolean("running", false)
                if (agent.isNotEmpty()) {
                    installingAgents[agent] = isRunning
                    val msg = value.optString("message")
                    if (msg.isNotEmpty()) agentMessages[agent] = msg
                    val out = value.optString("output")
                    if (out.isNotEmpty()) agentLogs[agent] = out
                    if (!isRunning) {
                        val ok = value.optBoolean("ok", false)
                        val ver = value.optString("version")
                        val installed = value.optBoolean("installed", ok)
                        val prev = backendsStatus[agent]
                        backendsStatus = backendsStatus + (agent to BackendStatus(
                            id = agent,
                            label = prev?.label ?: backendLabel(agent),
                            installed = installed,
                            version = ver.ifEmpty { prev?.version.orEmpty() },
                            running = false,
                        ))
                        backendsKnown = true
                        if (agent == "claude" && ver.isNotEmpty()) claudeVersion = ver
                        if (msg.isNotEmpty()) notice = msg
                    }
                }
            }
            "auth" -> {
                val id = value.optString("id")
                val cur = authFlow
                if (cur != null && (id.isEmpty() || cur.id == id)) {
                    val state = value.optString("state", cur.state)
                    val msg = value.optString("message", cur.message)
                    if (state == "cancelled") {
                        authFlow = null
                    } else {
                        authFlow = cur.copy(
                            state = state,
                            message = msg,
                            frame = if (state in setOf("success", "failed")) null else cur.frame,
                        )
                        if (state == "success") {
                            notice = msg.ifBlank { "Вход выполнен на сервере" }
                            send(JSONObject().put("op", "backends_status"))
                        }
                    }
                }
            }
            "auth_frame" -> {
                val id = value.optString("id")
                val seq = value.optInt("frame", 0)
                val cur = authFlow
                if (cur != null && cur.id == id && seq >= 1) {
                    send(JSONObject().put("op", "auth_browser_ack").put("id", id).put("frame", seq))
                    val data = value.optString("data")
                    if (data.isNotEmpty()) {
                        val bmp = runCatching {
                            val bytes = Base64.getDecoder().decode(data)
                            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
                        }.getOrNull()
                        if (bmp != null) {
                            authFlow = cur.copy(
                                frame = bmp,
                                frameWidth = value.optInt("width", 1280).coerceAtLeast(1),
                                frameHeight = value.optInt("height", 960).coerceAtLeast(1),
                            )
                        }
                    }
                }
            }
            "browser_setup" -> {
                browserSetupRunning = value.optBoolean("running", false)
                val msg = value.optString("message")
                if (msg.isNotEmpty()) {
                    notice = msg
                    authFlow?.let { authFlow = it.copy(message = msg) }
                }
            }
            "sessions" -> {
                val rows = value.optJSONArray("sessions") ?: JSONArray()
                sessions = (0 until rows.length()).mapNotNull { rows.optJSONObject(it) }.map {
                    val locked = it.optBoolean("locked", false)
                    val unlocked = it.optBoolean("unlocked", !locked)
                    val sid = it.optString("id")
                    val title = it.optString("title")
                    val modified = it.optDouble("modified", 0.0)
                    if (locked && unlocked && sid.isNotEmpty()) rememberLockedSessionName(sid, title)
                    Session(
                        id = sid,
                        title = if (locked && !unlocked) lockListName(sid, modified) else title,
                        cwd = it.optString("cwd"),
                        modified = modified,
                        pinned = it.optDouble("pinned", 0.0).let { p -> if (p.isNaN()) 0.0 else p },
                        folder = it.optString("folder"),
                        autoTitle = it.optString("auto_title"),
                        running = it.optBoolean("running"),
                        backend = it.optString("backend"),
                        locked = locked,
                        unlocked = unlocked,
                    )
                }.filter { it.id.isNotEmpty() }.distinctBy { it.id }
                if (protocol >= 4) running = sessions.filter { it.running }.map { it.id }.toSet()
                val f = value.optJSONArray("folders")
                if (f != null) folders = (0 until f.length()).mapNotNull { f.optJSONObject(it) }.map { Folder(it.optString("id"), it.optString("name"), it.optString("icon")) }
                sessionsLoaded = true
                sessions.forEach { s ->
                    chats[s.id]?.let { c ->
                        if (s.modified > c.syncedModified + 0.25) {
                            // Written from another device or a terminal: pull it into the open chat.
                            if (c.syncedModified > 0 && s.id == selected && c.loaded && !c.loading && !c.transcript.busy && !s.locked) {
                                send(JSONObject().put("op", "history").put("sid", s.id))
                            }
                            c.syncedModified = s.modified
                        }
                        if (c.outbox.isNotEmpty() && s.id != selected) flushOutbox(c)
                        c.title = s.title
                        c.cwd = s.cwd
                        c.locked = s.locked
                        c.unlocked = s.unlocked
                        if (c.isLockedGate()) {
                            c.loading = false
                            c.loaded = false
                            c.transcript.messages.clear()
                            c.permissions.clear()
                            c.changed()
                        }
                    }
                }
            }
            "session_locked" -> {
                sessions.firstOrNull { it.id == sid && !it.isLockedGate() }?.let { rememberLockedSessionName(sid, it.title) }
                sessions = sessions.map { if (it.id == sid) it.copy(title = lockListName(sid), locked = true, unlocked = false) else it }
                chats[sid]?.let { c ->
                    c.title = lockListName(sid)
                    c.locked = true
                    c.unlocked = false
                    c.unlocking = false
                    c.loading = false
                    c.loaded = false
                    c.transcript.messages.clear()
                    c.permissions.clear()
                    c.changed()
                }
            }
            "session_unlock_result" -> {
                val ok = value.optBoolean("ok", false)
                if (ok) {
                    sessions = sessions.map { if (it.id == sid) it.copy(unlocked = true) else it }
                }
                chats[sid]?.let { c ->
                    c.unlocking = false
                    if (ok) {
                        c.unlocked = true
                        c.lockError = ""
                        c.loading = true
                        send(JSONObject().put("op", "history").put("sid", sid))
                    } else {
                        c.lockError = value.optString("message").ifBlank { "Неверный пароль сессии." }
                    }
                    c.changed()
                }
            }
            "session_lock_changed" -> {
                val isLocked = value.optBoolean("locked", false)
                val isUnlocked = value.optBoolean("unlocked", !isLocked)
                if (isLocked && !isUnlocked) {
                    sessions.firstOrNull { it.id == sid && !it.isLockedGate() }?.let { rememberLockedSessionName(sid, it.title) }
                }
                sessions = sessions.map {
                    if (it.id == sid) it.copy(title = if (isLocked && !isUnlocked) lockListName(sid) else it.title,
                        locked = isLocked, unlocked = isUnlocked) else it
                }
                chats[sid]?.let { c ->
                    c.locked = isLocked
                    c.unlocked = isUnlocked
                    c.unlocking = false
                    if (isLocked && !isUnlocked) {
                        c.title = lockListName(sid)
                        c.loading = false
                        c.loaded = false
                        c.transcript.messages.clear()
                        c.permissions.clear()
                    } else {
                        c.lockError = ""
                    }
                    c.changed()
                }
            }
            "history" -> chats[sid]?.let { c ->
                c.loading = false
                if ((!c.transcript.busy || !c.loaded) && !c.isLockedGate()) {
                    c.transcript.loadHistory(value.optJSONArray("messages") ?: JSONArray())
                    c.loading = false
                    c.loaded = true
                    // Opened while the agent is still answering, maybe started from another device.
                    if (sid in running) {
                        c.transcript.busy = true
                        c.transcript.userAt = c.transcript.messages.indexOfLast { it.role == "user" }.coerceAtLeast(0)
                        c.transcript.status = "Работает"
                    }
                    c.changed()
                }
                flushOutbox(c)
            }
            "claude" -> chats[sid]?.let { c ->
                if (c.isLockedGate()) return@let
                val data = value.optJSONObject("data") ?: return@let
                c.transcript.reduce(data)
                if (data.optString("type") == "result" && data.optBoolean("is_error")) c.queued.clear()
                if (data.optString("type") == "result" && c.queued.isNotEmpty()) {
                    c.transcript.userAt = c.transcript.messages.size
                    c.transcript.messages.add(Message("user", c.queued.removeAt(0)))
                    c.transcript.busy = true
                    c.transcript.status = "Думает над ответом"
                }
                c.changed()
                if (data.optString("type") == "result") send(JSONObject().put("op", "sessions"))
            }
            "catalog" -> {
                val b = value.optString("backend").ifBlank { "claude" }
                val parsedModels = value.optJSONArray("models")?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map {
                    ModelInfo(it.optString("value"), it.optString("displayName"), it.optString("description"))
                } }.orEmpty().filter { it.value.isNotEmpty() }
                val parsedCommands = value.optJSONArray("commands")?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map {
                    CommandInfo(it.optString("name"), it.optString("description"), it.optString("hint"))
                } }.orEmpty().filter { it.name.isNotEmpty() }
                when (b) {
                    "codex" -> {
                        if (parsedModels.isNotEmpty()) codexModels = parsedModels
                        if (parsedCommands.isNotEmpty()) codexCommands = parsedCommands
                    }
                    "agy" -> {
                        if (parsedModels.isNotEmpty()) agyModels = parsedModels
                        if (parsedCommands.isNotEmpty()) agyCommands = parsedCommands
                    }
                    else -> {
                        models = parsedModels
                        commands = parsedCommands
                    }
                }
            }
            "api_models" -> {
                if (value.optString("request") == apiRequest) {
                    apiChecking = false
                    apiError = value.optString("error")
                    if (apiError.isEmpty()) {
                        apiModels = value.optJSONArray("models")?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map {
                            ModelInfo(it.optString("value"), it.optString("displayName"), it.optString("description"))
                        } }.orEmpty()
                    }
                }
            }
            "turn_summary" -> chats[sid]?.takeUnless { it.isLockedGate() }?.let { c ->
                val backend = value.optString("backend")
                val previous = c.turnSummary.takeIf { it.backend == backend } ?: TurnSummary(backend)
                c.turnSummary = previous.copy(
                    diff = when {
                        value.optBoolean("reset_diff") -> null
                        value.has("added") && value.has("removed") -> value.optLong("added") to value.optLong("removed")
                        else -> previous.diff
                    },
                    context = if (value.optLong("limit") > 0 && value.has("used")) {
                        value.optLong("used") to value.optLong("limit")
                    } else previous.context
                )
            }
            "run_state" -> {
                val busy = value.optBoolean("busy")
                running = if (busy) running + sid else running - sid
                chats[sid]?.let { c ->
                    val b = value.optString("backend")
                    if (busy && b in BACKENDS) {
                        c.backend = b
                        c.api = b == "api"
                        current?.let { store.setSessionBackend(it.id, sid, b) }
                    }
                    val background = value.optJSONArray("background")?.let { a -> (0 until a.length()).map { a.optString(it) } }.orEmpty()
                    if (busy && !c.transcript.busy && c.loaded && !c.isLockedGate()) {
                        c.transcript.busy = true
                        if (c.transcript.status.isEmpty()) c.transcript.status = "Работает"
                        c.changed()
                    }
                    if (busy && background.isNotEmpty() && (c.transcript.status.isEmpty() || c.transcript.status == "Работает" || c.transcript.status.startsWith("В фоне"))) {
                        c.transcript.status = "В фоне: " + background.joinToString()
                        c.changed()
                    } else if (!busy && c.transcript.busy) {
                        c.transcript.busy = false
                        c.transcript.status = ""
                        c.queued.clear()
                        c.changed()
                    }
                }
            }
            "permission" -> chats[sid]?.let { c ->
                if (!c.isLockedGate()) {
                    c.permissions.add(Permission(value.optString("id"), value.optString("tool", "Действие"), value.optJSONObject("input") ?: JSONObject()))
                }
            }
            "permission_cancelled" -> chats[sid]?.permissions?.removeAll { it.id == value.optString("id") }
            "run_closed" -> chats[sid]?.let { c ->
                if (c.transcript.busy) {
                    c.transcript.error = value.optString("detail").ifEmpty { "Ответ остановлен. История сохранена, можно продолжить." }
                }
                c.transcript.busy = false
                c.transcript.status = ""
                c.permissions.clear()
                c.queued.clear()
                c.changed()
            }
            "error" -> {
                val c = chats[sid]
                if (c != null && value.optString("op") !in setOf("session_lock", "delete_session")) {
                    c.transcript.error = value.optString("message", "Не удалось выполнить действие.")
                    if (value.optString("op") == "prompt") c.transcript.busy = false
                    if (value.optString("op") == "history") c.loading = false
                    c.changed()
                } else {
                    notice = value.optString("message")
                }
            }
            "session_deleted" -> {
                current?.let { store.setLockedSessionName(it.id, sid, "") }
                sessions = sessions.filter { it.id != sid }
                chats.remove(sid)
                if (selected == sid) {
                    selected = null
                    screen = Screen.Sessions
                }
            }
            "model_changed", "mode_changed", "effort_changed" -> chats[sid]?.let { c ->
                if (value.optString("backend", c.activeBackend()) == c.activeBackend()) {
                    when (value.optString("type")) {
                        "model_changed" -> c.updateSelectedModel(value.optString("model", "default"))
                        "mode_changed" -> c.mode = value.optString("mode", "manual")
                        "effort_changed" -> c.effort = concreteEffort(value.optString("effort"))
                    }
                }
            }
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
        if (bridge == null) return
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
        send(JSONObject().put("op", "backends_status"))
        chat()?.let { c -> if (!c.transcript.busy && !c.isLockedGate()) send(JSONObject().put("op", "history").put("sid", c.sid)) }
    }

    fun open(session: Session) {
        val s = current ?: return
        val c = chats.getOrPut(session.id) { Chat(session.id, session.cwd, session.title, s.model, s.mode, s.effort) }
        val chosen = store.sessionBackend(s.id, c.sid)
            ?: session.backend.takeIf { it in BACKENDS }
            ?: s.effectiveBackend()
        c.backend = chosen
        c.api = chosen == "api"
        if (c.apiModel.isBlank()) c.apiModel = s.api.model
        if (c.codexModel.isBlank() || c.codexModel == "default") c.codexModel = s.codexModel
        if (c.agyModel.isBlank() || c.agyModel == "default") c.agyModel = s.agyModel
        c.locked = session.locked
        c.unlocked = session.unlocked
        c.unlocking = false
        c.lockError = ""
        if (c.isLockedGate()) {
            c.loading = false
            c.loaded = false
            c.transcript.messages.clear()
            c.permissions.clear()
            c.changed()
        } else if (!c.transcript.busy) {
            c.loading = !c.loaded
            send(JSONObject().put("op", "history").put("sid", session.id))
            if (chosen in listOf("codex", "agy") && link == Link.Online) {
                send(JSONObject().put("op", "catalog").put("backend", chosen))
            }
        }
        selected = session.id
        screen = Screen.Chat
    }

    fun newChat() {
        val s = current ?: return
        val sid = UUID.randomUUID().toString()
        val c = Chat(sid, if (s.cwd == "~") home else s.cwd, "Новая сессия", s.model, s.mode, s.effort)
        val b = s.effectiveBackend()
        c.backend = b
        c.api = b == "api"
        c.apiModel = s.api.model
        c.codexModel = s.codexModel
        c.agyModel = s.agyModel
        c.loaded = true
        chats[sid] = c
        selected = sid
        screen = Screen.Chat
        if (b in listOf("codex", "agy") && link == Link.Online) {
            send(JSONObject().put("op", "catalog").put("backend", b))
        }
    }

    /** Can a message go now? Locked sessions are gated; while busy, only protocol >= 4 steers/queues. */
    fun canSend(c: Chat) = link == Link.Online && !c.isLockedGate() && (!c.transcript.busy || protocol >= 4)

    /** `/model`, `/mode` and `/stop` act at once, also mid-answer; other commands go to the CLI engine. */
    private fun localCommand(c: Chat, text: String): Boolean {
        val name = text.substringBefore(' ').lowercase()
        val arg = text.substringAfter(' ', "").trim()
        when {
            name == "/clear" || name == "/new" -> {
                c.draft = ""
                newChat()
                notice = "Открыта новая чистая сессия."
                return true
            }
            name == "/effort" && c.activeBackend() == "agy" -> {
                notice = "В Antigravity глубина выбирается вместе с моделью."
            }
            name == "/effort" && arg.isNotEmpty() -> {
                c.effort = when (arg.lowercase()) {
                    "low", "быстро" -> "low"
                    "medium", "средне" -> "medium"
                    "high", "глубоко" -> "high"
                    "xhigh" -> "xhigh"
                    "max", "макс" -> "max"
                    else -> null
                } ?: run {
                    notice = "Глубина: low, medium, high, xhigh или max."
                    return true
                }
                notice = "Глубина: ${c.effort}"
            }
            name == "/model" && arg.isNotEmpty() -> {
                val active = c.activeBackend()
                val catalog = when (active) {
                    "api" -> apiCatalog()
                    "codex" -> catalogFor("codex")
                    "agy" -> catalogFor("agy")
                    else -> models
                }
                val value = (resolveModel(catalog.filter { explicitModel(it.value) }, arg)
                    ?: arg.takeIf { active == "api" }).takeIf { it != null && explicitModel(it) }
                if (value == null) {
                    c.transcript.error = "Нет такой модели: $arg. Доступны: " + catalog.joinToString { it.value }
                } else {
                    c.updateSelectedModel(value)
                    send(JSONObject().put("op", "control").put("sid", c.sid).put("backend", c.activeBackend()).put("action", "model").put("value", value))
                    notice = "Модель: " + modelLabel(catalog, value)
                }
            }
            name == "/mode" && arg.isNotEmpty() -> {
                c.mode = when (arg.lowercase()) {
                    "auto", "авто" -> "auto"
                    "plan", "план" -> "plan"
                    "edits", "acceptedits", "правки" -> "acceptEdits"
                    "manual", "ручной" -> "manual"
                    else -> {
                        notice = "Режим: auto, manual, edits или plan."
                        return true
                    }
                }
                send(JSONObject().put("op", "control").put("sid", c.sid).put("backend", c.activeBackend()).put("action", "mode").put("value", c.mode))
            }
            name == "/stop" -> stop()
            else -> return false
        }
        c.draft = ""
        c.changed()
        return true
    }

    /** Model or mode picked while the agent answers: switch it live. */
    fun liveChange(c: Chat) {
        if (protocol >= 4 && c.transcript.busy) {
            val model = modelForChat(c)
            if (model.isNotBlank()) {
                send(JSONObject().put("op", "control").put("sid", c.sid).put("backend", c.activeBackend()).put("action", "model").put("value", model))
            }
            send(JSONObject().put("op", "control").put("sid", c.sid).put("backend", c.activeBackend()).put("action", "mode").put("value", c.mode))
        }
    }

    /** Photos and files from the phone's picker, read now so the message goes in one piece. */
    fun pick(uris: List<android.net.Uri>) {
        val c = chat() ?: return
        val app = getApplication<Application>()
        Thread {
            val added = mutableListOf<Picked>()
            var problem = ""
            for (uri in uris) {
                if (c.picked.size + added.size >= 8) { problem = "До восьми вложений в одном сообщении."; break }
                runCatching {
                    val resolver = app.contentResolver
                    val name = resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cur ->
                        if (cur.moveToFirst()) cur.getString(0) else null
                    } ?: uri.lastPathSegment?.substringAfterLast('/') ?: "файл"
                    val mime = resolver.getType(uri) ?: Downloads.mime(name)
                    val bytes = resolver.openInputStream(uri)!!.use { it.readBytes() }
                    val total = c.picked.sumOf { it.bytes.size } + added.sumOf { it.bytes.size } + bytes.size
                    if (total > 12 * 1024 * 1024) { problem = "С телефона можно отправить до 12 МБ за раз."; return@runCatching }
                    val preview = if (mime.startsWith("image/")) runCatching {
                        val options = BitmapFactory.Options().apply { inSampleSize = 4 }
                        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.asImageBitmap()
                    }.getOrNull() else null
                    added.add(Picked(name, mime, bytes, preview))
                }.onFailure { problem = "Не удалось прочитать файл: ${it.message}" }
            }
            main.post {
                c.picked.addAll(added)
                if (problem.isNotEmpty()) notice = problem
            }
        }.start()
    }

    fun unpick(c: Chat, item: Picked) {
        c.picked.remove(item)
    }

    fun submit() {
        val c = chat() ?: return
        if (c.isLockedGate()) return
        if (!demo && link != Link.Online) {
            // No connection: keep the message and send it as soon as the server is back.
            val text = c.draft
            if (text.isNotBlank()) {
                c.outbox.add(text)
                c.draft = ""
                c.transcript.error = ""
                c.transcript.status = "Ждёт подключения · отправится автоматически"
                c.changed()
            }
            if (link == Link.Off) reconnectNow()
            return
        }
        submitNow(c)
    }

    private fun submitNow(c: Chat) {
        val profile = current ?: return
        if (c.isLockedGate()) return
        val activeBackend = c.activeBackend()
        val draft = c.draft
        val command = draft.trim()
        if ((command.isEmpty() && c.picked.isEmpty()) || link != Link.Online) return
        if (c.picked.isEmpty() && localCommand(c, command)) return
        val quickSlash = c.picked.isEmpty() && command.substringBefore(' ').lowercase() in setOf(
            "/usage", "/limits", "/quota", "/reset-limits", "/reset-usage", "/cost", "/tokens", "/status", "/info", "/help", "/commands",
            "/model", "/mode", "/effort"
        )
        val text = if (quickSlash) command else draft
        if (activeBackend == "api" && !quickSlash) {
            profile.api.copy(model = c.apiModel.ifBlank { profile.api.model }).error()?.let {
                c.transcript.error = it
                c.changed()
                return
            }
        }
        if (!canSend(c)) return
        val selectedModel = modelForChat(c)
        if (!quickSlash && selectedModel.isEmpty()) {
            c.transcript.error = "Выбери модель перед отправкой."
            c.changed()
            return
        }
        if (selectedModel.isNotEmpty()) c.updateSelectedModel(selectedModel)
        c.effort = concreteEffort(c.effort)
        c.transcript.error = ""
        val steering = c.transcript.busy
        c.transcript.userAt = c.transcript.messages.size
        val attached = c.picked.toList()
        c.picked.clear()
        val b64 = Base64.getEncoder()
        val images = JSONArray(attached.filter { it.image }.map { JSONObject().put("type", "base64").put("media_type", it.mime).put("data", b64.encodeToString(it.bytes)) })
        val files = JSONArray(attached.filter { !it.image }.map { JSONObject().put("type", "file").put("name", it.name).put("data", b64.encodeToString(it.bytes)) })
        val actorStr = actor.ifBlank { "Relay Android" }
        c.transcript.turnBackend = activeBackend
        c.transcript.turnModel = selectedModel
        c.transcript.turnMode = c.mode
        val turnEffort = if (activeBackend == "agy") "" else c.effort
        c.transcript.turnEffort = turnEffort
        c.transcript.turnActor = actorStr
        c.transcript.messages.add(
            Message(
                "user",
                text,
                images = images.length(),
                files = attached.filter { !it.image }.map { it.name },
                backend = activeBackend,
                model = selectedModel,
                actor = actorStr,
                mode = c.mode,
                effort = turnEffort,
                steer = steering,
            )
        )
        if (!steering) {
            c.transcript.busy = true
            c.transcript.status = "Отправляем"
        }
        if (c.title == "Новая сессия") c.title = text.ifEmpty { attached.firstOrNull()?.name.orEmpty() }.take(60)
        c.draft = ""
        c.changed()
        val request = JSONObject()
            .put("op", "prompt")
            .put("sid", c.sid)
            .put("cwd", c.cwd)
            .put("text", text)
            .put("images", images)
            .put("files", files)
            .put("model", selectedModel)
            .put("mode", c.mode)
            .put("effort", turnEffort.ifBlank { "default" })
            .put("actor", actor.ifBlank { "Relay Android" })
            .put("desktop_enabled", false)
            .put("desktop_context", JSONObject())
            .put("backend", activeBackend)
        if (activeBackend == "api" && !quickSlash) {
            request.put("api", profile.api.request(store.apiKey(profile.id).orEmpty(), c.apiModel))
        }
        send(request)
        current?.let { s ->
            if (s.model != c.model || s.codexModel != c.codexModel || s.agyModel != c.agyModel || s.mode != c.mode || s.effort != c.effort) {
                val updated = s.copy(model = c.model, codexModel = c.codexModel, agyModel = c.agyModel, mode = c.mode, effort = c.effort)
                current = updated
                saveServer(updated, null, false)
            }
        }
    }

    // --- Private Password-Locked Sessions ---

    fun lockSession(sid: String, password: String, currentPassword: String = "", remove: Boolean = false) {
        send(
            JSONObject()
                .put("op", "session_lock")
                .put("sid", sid)
                .put("password", password)
                .put("current_password", currentPassword)
                .put("remove", remove)
                .put("actor", actor.ifBlank { "Relay Android" })
        )
    }

    fun unlockSession(sid: String, password: String) {
        chats[sid]?.let {
            it.unlocking = true
            it.lockError = ""
        }
        send(JSONObject().put("op", "session_unlock").put("sid", sid).put("password", password))
    }

    fun relockSession(sid: String) {
        send(JSONObject().put("op", "session_relock").put("sid", sid))
    }

    // --- 1-Click CLI Agent Installer & Server OAuth Login ---

    fun installAgent(agent: String, action: String = "install") {
        if (agent == "all") {
            val targets = CLI_AGENTS.filter { backendsStatus[it]?.installed != true }.ifEmpty { CLI_AGENTS }
            for (a in targets) installingAgents[a] = true
        } else {
            installingAgents[agent] = true
        }
        send(JSONObject().put("op", "agent_install").put("agent", agent).put("action", action))
    }

    fun refreshBackends() {
        if (link == Link.Online) send(JSONObject().put("op", "backends_status"))
    }

    fun startAuth(provider: String) {
        if (link != Link.Online) {
            notice = "Подключись к серверу, чтобы войти в ${backendLabel(provider)}."
            return
        }
        val id = UUID.randomUUID().toString()
        authFlow = AuthFlowState(
            id = id,
            provider = provider,
            state = "starting",
            message = "Готовим вход в ${backendLabel(provider)} на сервере…",
        )
        send(JSONObject().put("op", "auth_start").put("id", id).put("provider", provider))
    }

    fun submitAuthInput(code: String) {
        val flow = authFlow ?: return
        val trimmed = code.trim()
        if (trimmed.isEmpty()) return
        send(JSONObject().put("op", "auth_input").put("id", flow.id).put("code", trimmed))
    }

    fun tapAuthBrowser(x: Int, y: Int) {
        val flow = authFlow ?: return
        val down = JSONObject().put("kind", "down").put("x", x).put("y", y).put("button", "left")
        val up = JSONObject().put("kind", "up").put("x", x).put("y", y).put("button", "left")
        send(JSONObject().put("op", "auth_browser_input").put("id", flow.id).put("input", down))
        send(JSONObject().put("op", "auth_browser_input").put("id", flow.id).put("input", up))
    }

    fun sendAuthBrowserText(text: String, pressEnter: Boolean = true) {
        val flow = authFlow ?: return
        if (text.isNotEmpty()) {
            send(JSONObject().put("op", "auth_browser_input").put("id", flow.id).put("input", JSONObject().put("kind", "text").put("text", text)))
        }
        if (pressEnter) {
            send(JSONObject().put("op", "auth_browser_input").put("id", flow.id).put("input", JSONObject().put("kind", "key").put("key", "Enter").put("pressed", true)))
            send(JSONObject().put("op", "auth_browser_input").put("id", flow.id).put("input", JSONObject().put("kind", "key").put("key", "Enter").put("pressed", false)))
        }
    }

    fun setupServerBrowser() {
        if (link != Link.Online) return
        browserSetupRunning = true
        send(JSONObject().put("op", "browser_setup").put("id", UUID.randomUUID().toString()))
    }

    fun cancelAuth() {
        val flow = authFlow
        if (flow != null && flow.active) {
            send(JSONObject().put("op", "auth_cancel").put("id", flow.id))
        }
        authFlow = null
    }

    /** A file mentioned in chat: fetched over the same connection into Downloads/Relay. */
    fun download(path: String, cwd: String) {
        if (transfers.any { it.path == path && it.error.isEmpty() && !it.done }) return
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
    fun rename(session: Session, title: String) {
        if (!session.isLockedGate()) {
            if (session.locked) current?.let { store.setLockedSessionName(it.id, session.id, title) }
            meta(session) { put("title", title.trim()) }
            return
        }
        val server = current?.id ?: return
        store.setLockedSessionName(server, session.id, title)
        val name = lockListName(session.id)
        sessions = sessions.map { if (it.id == session.id) it.copy(title = name) else it }
        chats[session.id]?.title = name
    }
    fun moveTo(session: Session, folder: String) = meta(session) { put("folder", folder) }
    fun delete(session: Session) {
        if (session.locked) {
            notice = "Сессия защищена паролем — сначала сними пароль, чтобы удалить её."
            return
        }
        send(JSONObject().put("op", "delete_session").put("sid", session.id))
    }
    fun createFolder(name: String, session: Session? = null) =
        send(JSONObject().put("op", "session_folder").put("name", name.trim()).put("sid", session?.id ?: JSONObject.NULL).put("actor", actor))
    fun renameFolder(folder: Folder, name: String) = send(JSONObject().put("op", "session_folder").put("id", folder.id).put("name", name.trim()))
    fun saveFolderIcon(folder: Folder, icon: String) = send(
        JSONObject().put("op", "session_folder").put("id", folder.id).put("icon", icon)
    )
    fun deleteFolder(folder: Folder) = send(JSONObject().put("op", "session_folder").put("id", folder.id).put("delete", true))
    fun toggleFolder(folder: Folder) {
        collapsed = if (folder.id in collapsed) collapsed - folder.id else collapsed + folder.id
        store.collapsed = collapsed
    }

    override fun onCleared() {
        disconnect()
    }
}
