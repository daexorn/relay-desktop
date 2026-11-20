package dev.relay.mobile

import android.content.Context
import androidx.compose.ui.graphics.ImageBitmap
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

val BACKENDS = listOf("claude", "codex", "agy", "api")
val CLI_AGENTS = listOf("claude", "codex", "agy")

fun backendLabel(b: String): String = when (b) {
    "codex" -> "OpenAI Codex"
    "agy" -> "Antigravity (agy)"
    "api" -> "Пользовательский API"
    else -> "Claude Code"
}

fun backendShortLabel(b: String): String = when (b) {
    "codex" -> "Codex"
    "agy" -> "AGY"
    "api" -> "API"
    else -> "Claude Code"
}

fun defaultBackendModels(backend: String): List<ModelInfo> = when (backend) {
    "codex" -> listOf(
        ModelInfo("gpt-5.4", "GPT-5.4", "Основная агентная модель OpenAI Codex"),
        ModelInfo("gpt-5.3-codex", "GPT-5.3 Codex", "Специализированная модель для кода"),
        ModelInfo("gpt-5.4-mini", "GPT-5.4 Mini", "Быстрая модель для простых задач"),
    )
    "agy" -> listOf(
        ModelInfo("gemini-4-carbon-medium", "Gemini 4 Carbon (Medium)", "Средняя глубина"),
        ModelInfo("gemini-4-carbon-high", "Gemini 4 Carbon (High)", "Высокая глубина"),
        ModelInfo("gemini-4-carbon-low", "Gemini 4 Carbon (Low)", "Низкая глубина"),
    )
    "claude" -> listOf(
        ModelInfo("fable", "Fable", ""),
        ModelInfo("opus", "Opus", ""),
        ModelInfo("sonnet", "Sonnet", ""),
        ModelInfo("haiku", "Haiku", ""),
    )
    else -> emptyList()
}

data class ServerProfile(
    val id: String = UUID.randomUUID().toString(),
    val name: String = "",
    val host: String = "",
    val port: Int = 22,
    val user: String = "root",
    val cwd: String = "~",
    val fingerprint: String? = null,
    val model: String = "default",
    val mode: String = "auto",
    val effort: String = "default",
    val api: ApiProfile = ApiProfile(),
    val defaultBackend: String = "",
    val codexModel: String = "default",
    val agyModel: String = "default",
) {
    fun effectiveBackend(): String =
        if (defaultBackend in BACKENDS) defaultBackend
        else if (api.enabled) "api"
        else "claude"

    fun json(): JSONObject = JSONObject()
        .put("id", id)
        .put("name", name)
        .put("host", host)
        .put("port", port)
        .put("user", user)
        .put("cwd", cwd)
        .put("fingerprint", fingerprint ?: JSONObject.NULL)
        .put("model", model)
        .put("mode", mode)
        .put("effort", effort)
        .put("api", api.json())
        .put("default_backend", defaultBackend)
        .put("codex_model", codexModel)
        .put("agy_model", agyModel)

    companion object {
        fun from(o: JSONObject) = ServerProfile(
            id = o.optString("id"),
            name = o.optString("name"),
            host = o.optString("host"),
            port = o.optInt("port", 22),
            user = o.optString("user", "root"),
            cwd = o.optString("cwd", "~"),
            fingerprint = o.optString("fingerprint").takeIf { it.isNotEmpty() && it != "null" },
            model = o.optString("model", "default"),
            mode = o.optString("mode", "auto"),
            effort = o.optString("effort", "default"),
            api = ApiProfile.from(o.optJSONObject("api")),
            defaultBackend = o.optString("default_backend", ""),
            codexModel = o.optString("codex_model", "default").ifBlank { "default" },
            agyModel = o.optString("agy_model", "default").ifBlank { "default" },
        )
    }
}

data class ApiProfile(
    val enabled: Boolean = false,
    val baseUrl: String = "",
    val model: String = "",
    val maxTokens: Int = 8192,
    val maxSteps: Int = 80,
    val models: List<String> = emptyList(),
) {
    fun json() = JSONObject()
        .put("enabled", enabled)
        .put("base_url", baseUrl)
        .put("model", model)
        .put("max_tokens", maxTokens)
        .put("max_steps", maxSteps)
        .put("models", JSONArray(models))

    fun request(key: String, selectedModel: String = model) =
        json().put("key", key).put("model", selectedModel.ifBlank { model })

    fun error(): String? {
        val uri = runCatching { java.net.URI(baseUrl.trim()) }.getOrNull()
        if (uri == null || uri.scheme !in listOf("http", "https") || uri.host.isNullOrBlank() || uri.userInfo != null || uri.query != null || uri.fragment != null) {
            return "Укажи базовый URL API, например https://tooken.club/v1."
        }
        if (model.isBlank() || model.length > 200 || model.any(Char::isISOControl)) return "Укажи идентификатор модели API."
        return null
    }

    companion object {
        fun from(o: JSONObject?) = if (o == null) ApiProfile() else ApiProfile(
            o.optBoolean("enabled"),
            o.optString("base_url"),
            o.optString("model"),
            o.optInt("max_tokens", 8192).coerceIn(256, 65536),
            o.optInt("max_steps", 80).coerceIn(1, 256),
            o.optJSONArray("models")?.let { a -> (0 until a.length().coerceAtMost(500)).map { a.optString(it) }.filter(String::isNotBlank) }.orEmpty(),
        )
    }
}

data class Session(
    val id: String,
    val title: String,
    val cwd: String,
    val modified: Double,
    val pinned: Double = 0.0,
    val folder: String = "",
    val autoTitle: String = "",
    val running: Boolean = false,
    val backend: String = "",
    val locked: Boolean = false,
    val unlocked: Boolean = true,
) {
    fun isLockedGate(): Boolean = locked && !unlocked
}

data class BackendStatus(
    val id: String,
    val label: String,
    val installed: Boolean = false,
    val version: String = "",
    val running: Boolean = false,
)

data class AuthFlowState(
    val id: String,
    val provider: String,
    val state: String = "starting",
    val message: String = "",
    val frame: ImageBitmap? = null,
    val frameWidth: Int = 1280,
    val frameHeight: Int = 960,
) {
    val active: Boolean get() = state in setOf("starting", "browser", "verifying", "input_error")
}

/** A model or slash command reported by a server CLI engine or API. */
data class ModelInfo(val value: String, val name: String, val description: String)
data class CommandInfo(val name: String, val description: String, val hint: String)

fun explicitModel(value: String) = value.isNotBlank() && value.trim().lowercase() !in setOf("default", "auto")

fun concreteModel(value: String, models: List<ModelInfo>, backend: String): String = when {
    explicitModel(value) -> value.trim()
    backend == "api" -> ""
    backend == "claude" && models.any { it.value == "sonnet" } -> "sonnet"
    else -> models.firstOrNull { explicitModel(it.value) }?.value ?: if (backend == "claude") "sonnet" else ""
}

fun concreteEffort(value: String) = if (value in setOf("", "default", "auto")) "medium" else value

fun lockedSessionTitle(modified: Double): String {
    if (!modified.isFinite() || modified <= 0) return "Сессия под паролем"
    val day = java.text.SimpleDateFormat("dd.MM 'в' HH:mm", java.util.Locale("ru"))
    return "Сессия от ${day.format(java.util.Date((modified * 1000).toLong()))}"
}

fun modelLabel(models: List<ModelInfo>, value: String): String = when {
    !explicitModel(value) -> "Выбрать модель"
    else -> models.firstOrNull { it.value == value }?.name?.takeIf { it.isNotEmpty() && it != value }
        ?: value.removePrefix("claude-").replace('-', ' ').replaceFirstChar { it.uppercase() }.let { if (it.length > 28) it.take(27) + "…" else it }
}

/** `fable`, `Fable 5.2` or a full id → the value the engine accepts; null when unknown. */
fun resolveModel(models: List<ModelInfo>, typed: String): String? {
    val t = typed.lowercase()
    if (models.isEmpty()) return typed
    return (models.firstOrNull { it.value.lowercase() == t || it.name.lowercase() == t }
        ?: models.firstOrNull { it.name.lowercase().startsWith(t) || it.value.lowercase().startsWith(t) })?.value
}

/** Commands for the `/` popup while the draft is one word starting with `/`. */
fun slashMatches(draft: String, commands: List<CommandInfo>): List<CommandInfo> {
    if (!draft.startsWith("/") || draft.any { it.isWhitespace() }) return emptyList()
    val t = draft.drop(1).lowercase()
    val found = commands.filter { it.name.lowercase().startsWith(t) } + commands.filter { !it.name.lowercase().startsWith(t) && it.name.lowercase().contains(t) }
    return if (found.size == 1 && found[0].name == t) emptyList() else found
}

fun builtinSlashCommands(): List<CommandInfo> = listOf(
    CommandInfo("usage", "Лимиты и токены выбранного агента", ""),
    CommandInfo("reset-limits", "Проверить доступность разового сброса", ""),
    CommandInfo("cost", "Статистика токенов и вызовов инструментов сессии", ""),
    CommandInfo("status", "Состояние сервера, установленных CLI и текущей сессии", ""),
    CommandInfo("clear", "Начать новую чистую сессию в этой папке", ""),
    CommandInfo("compact", "Сжать историю длинного разговора (Claude Code)", ""),
    CommandInfo("model", "Сменить модель агента", "<название>"),
    CommandInfo("mode", "Режим подтверждений: auto, manual, edits, plan", "<режим>"),
    CommandInfo("effort", "Глубина рассуждений: auto, low, medium, high, max", "<уровень>"),
    CommandInfo("stop", "Остановить текущий ответ агента", ""),
    CommandInfo("help", "Показать список всех быстрых команд Relay", ""),
)

data class Folder(val id: String, val name: String, val icon: String = "")

data class Message(
    val role: String,
    var text: String = "",
    val tool: String = "",
    val input: JSONObject = JSONObject(),
    val toolId: String = "",
    val failed: Boolean = false,
    val images: Int = 0,
    /** Names of files sent with this message from the phone. */
    val files: List<String> = emptyList(),
    var backend: String = "",
    var model: String = "",
    var actor: String = "",
    var mode: String = "",
    var effort: String = "",
    var steer: Boolean = false,
    var agentName: String = "",
)

data class MessageAttribution(
    val who: String = "",
    val toWhom: String = "",
    val how: String = "",
    val backend: String = "claude",
)

fun prettyModelTitle(raw: String): String {
    val s = raw.trim()
    if (s.isEmpty() || s == "default") return ""
    val low = s.lowercase()
    return when {
        "sonnet-4-6" in low || "sonnet-4.6" in low -> "Sonnet 4.6"
        "opus-4-6" in low || "opus-4.6" in low -> "Opus 4.6"
        "haiku-4-5" in low || "haiku-4.5" in low -> "Haiku 4.5"
        "gpt-5.4" in low && "mini" in low -> "GPT-5.4 Mini"
        "gpt-5.4" in low -> "GPT-5.4"
        "gpt-5.3" in low -> "GPT-5.3 Codex"
        "gemini-3.1-pro" in low -> "Gemini 3.1 Pro"
        "gemini-3-flash" in low -> "Gemini 3 Flash"
        else -> s.removePrefix("claude-").replace('-', ' ').take(24)
    }
}

fun computeAttributions(messages: List<Message>, fallbackBackend: String, busy: Boolean): List<MessageAttribution> {
    val out = MutableList(messages.size) { MessageAttribution(backend = fallbackBackend) }
    var lastUserIdx: Int? = null
    var curBackend = fallbackBackend.ifBlank { "claude" }
    var curModel = ""
    var curMode = ""
    var curEffort = ""
    var curActor = ""
    var curAgent = ""
    var toolsSinceUser = 0

    for ((idx, m) in messages.withIndex()) {
        if (m.backend.isNotBlank()) curBackend = m.backend
        if (m.model.isNotBlank()) curModel = m.model
        if (m.mode.isNotBlank()) curMode = m.mode
        if (m.effort.isNotBlank()) curEffort = m.effort
        if (m.actor.isNotBlank()) curActor = m.actor
        if (m.agentName.isNotBlank()) curAgent = m.agentName

        when (m.role) {
            "user" -> {
                toolsSinceUser = 0
                val who = curActor.ifBlank { "Вы" }
                val target = curAgent.ifBlank { backendShortLabel(curBackend) }
                val nextAssistant = ((idx + 1) until messages.size).firstOrNull { messages[it].role == "assistant" && messages[it].text.isNotBlank() }
                val nextUserBefore = ((idx + 1) until messages.size).any { k ->
                    messages[k].role == "user" && (nextAssistant == null || k < nextAssistant)
                }
                val howParts = mutableListOf<String>()
                if (m.steer) howParts += "по ходу ответа"
                prettyModelTitle(curModel).takeIf { it.isNotEmpty() }?.let { howParts += it }
                when {
                    nextUserBefore && !m.steer -> howParts += "без отдельного ответа"
                    nextAssistant == null && busy && idx + 1 == messages.size -> howParts += "ждёт ответа…"
                }
                out[idx] = MessageAttribution(who = who, toWhom = target, how = howParts.joinToString(" · "), backend = curBackend)
                lastUserIdx = idx
            }
            "tool" -> toolsSinceUser++
            "assistant" -> {
                val who = curAgent.ifBlank { backendLabel(curBackend) }
                val toWhom = lastUserIdx?.let { uIdx ->
                    val um = messages[uIdx]
                    val uWho = um.actor.ifBlank { curActor.ifBlank { "Вам" } }
                    val preview = um.text.lineSequence().firstOrNull { it.isNotBlank() && !it.trimStart().startsWith("<relay-") }?.trim()?.take(22).orEmpty()
                    if (preview.isEmpty()) uWho else "$uWho · на «$preview»"
                }.orEmpty()
                val howParts = mutableListOf<String>()
                prettyModelTitle(curModel).takeIf { it.isNotEmpty() }?.let { howParts += it }
                when (curMode) {
                    "auto" -> howParts += "Авто"
                    "plan" -> howParts += "План"
                    "acceptEdits" -> howParts += "Правки"
                }
                if (toolsSinceUser > 0) howParts += "через $toolsSinceUser ${stepsWord(toolsSinceUser)}"
                else howParts += "прямой ответ"
                out[idx] = MessageAttribution(who = who, toWhom = toWhom, how = howParts.joinToString(" · "), backend = curBackend)
            }
        }
    }
    return out
}

/** A photo or file picked on the phone, waiting to go with the next message. */
class Picked(val name: String, val mime: String, val bytes: ByteArray, val preview: ImageBitmap?) {
    /** The server helper takes PNG, JPEG, WebP and GIF up to 4 MB as pictures the model sees; the rest as files. */
    val image get() = mime in setOf("image/png", "image/jpeg", "image/webp", "image/gif") && bytes.size <= 4 * 1024 * 1024
}

data class Permission(val id: String, val tool: String, val input: JSONObject)

/** Servers, theme and passwords on this phone. Passwords go to Android Keystore-backed storage. */
class Store(context: Context) {
    private val prefs = context.getSharedPreferences("relay", Context.MODE_PRIVATE)
    // Created on first use: Keystore problems then only affect saved passwords, not startup.
    private val secrets by lazy {
        EncryptedSharedPreferences.create(
            context,
            "relay-secrets",
            MasterKey.Builder(context).setKeyScheme(MasterKey.KeyScheme.AES256_GCM).build(),
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    var servers: List<ServerProfile>
        get() = runCatching {
            val a = JSONArray(prefs.getString("servers", "[]"))
            (0 until a.length()).map { ServerProfile.from(a.getJSONObject(it)) }
        }.getOrDefault(emptyList())
        set(value) {
            prefs.edit().putString("servers", JSONArray(value.map { it.json() }).toString()).apply()
        }

    var theme: String
        get() = prefs.getString("theme", "graphite") ?: "graphite"
        set(value) = prefs.edit().putString("theme", value).apply()

    var actor: String
        get() = prefs.getString("actor", "") ?: ""
        set(value) = prefs.edit().putString("actor", value).apply()

    var collapsed: Set<String>
        get() = prefs.getStringSet("collapsed", emptySet()) ?: emptySet()
        set(value) = prefs.edit().putStringSet("collapsed", value).apply()

    fun password(id: String): String? = runCatching { secrets.getString(id, null) }.getOrNull()
    fun setPassword(id: String, value: String?) {
        runCatching { secrets.edit().apply { if (value == null) remove(id) else putString(id, value) }.apply() }
    }
    fun apiKey(id: String): String? = runCatching { secrets.getString("api:$id", null) }.getOrNull()
    fun setApiKey(id: String, value: String): Result<Unit> = runCatching {
        check(secrets.edit().apply { if (value.isBlank()) remove("api:$id") else putString("api:$id", value) }.commit()) { "Не удалось сохранить API-ключ" }
    }
    fun apiSource(server: String, sid: String): Boolean? =
        if (prefs.contains("api:$server/$sid")) prefs.getBoolean("api:$server/$sid", false) else null
    fun setApiSource(server: String, sid: String, enabled: Boolean) {
        prefs.edit().putBoolean("api:$server/$sid", enabled).apply()
    }
    fun lockedSessionName(server: String, sid: String): String? = prefs.getString("locked-name:$server/$sid", null)

    fun setLockedSessionName(server: String, sid: String, name: String) {
        prefs.edit().apply {
            if (name.isBlank()) remove("locked-name:$server/$sid")
            else putString("locked-name:$server/$sid", name.trim().take(120))
        }.apply()
    }
    fun sessionBackend(server: String, sid: String): String? {
        val stored = prefs.getString("backend:$server/$sid", null)
        if (stored != null && stored in BACKENDS) return stored
        return apiSource(server, sid)?.let { if (it) "api" else "claude" }
    }
    fun setSessionBackend(server: String, sid: String, backend: String) {
        if (backend !in BACKENDS) return
        prefs.edit()
            .putString("backend:$server/$sid", backend)
            .putBoolean("api:$server/$sid", backend == "api")
            .apply()
    }
}

/** Port of the desktop `reduce`: folds stream events into the message list. */
class Transcript {
    val messages = mutableListOf<Message>()
    private val blocks = HashMap<Int, Int>()
    var userAt = 0
    var status = ""
    var error = ""
    var busy = false
    var turnBackend = "claude"
    var turnModel = ""
    var turnMode = ""
    var turnEffort = ""
    var turnActor = ""

    private fun stamp(m: Message, data: JSONObject) {
        val b = data.optString("relay_backend").ifBlank { turnBackend }
        if (m.backend.isBlank() && b.isNotBlank()) m.backend = b
        val mdl = data.optString("relay_model")
            .ifBlank { data.optJSONObject("message")?.optString("model").orEmpty() }
            .ifBlank { turnModel }
        if (m.model.isBlank() && mdl.isNotBlank()) m.model = mdl
        val md = data.optString("relay_mode").ifBlank { turnMode }
        if (m.mode.isBlank() && md.isNotBlank()) m.mode = md
        val ef = data.optString("relay_effort").ifBlank { turnEffort }
        if (m.effort.isBlank() && ef.isNotBlank()) m.effort = ef
        val ac = data.optString("relay_actor").ifBlank { turnActor }
        if (m.actor.isBlank() && ac.isNotBlank()) m.actor = ac
        val ag = data.optString("relay_agent_name")
        if (m.agentName.isBlank() && ag.isNotBlank()) m.agentName = ag
    }

    fun reduce(data: JSONObject) {
        when (data.optString("type")) {
            "stream_event" -> {
                val e = data.optJSONObject("event") ?: return
                val index = e.optInt("index")
                when (e.optString("type")) {
                    "message_start" -> blocks.clear()
                    "content_block_start" -> {
                        val block = e.optJSONObject("content_block") ?: return
                        when (block.optString("type")) {
                            "text" -> {
                                val msg = Message("assistant", block.optString("text"))
                                stamp(msg, data)
                                messages.add(msg)
                                blocks[index] = messages.size - 1
                            }
                            "thinking" -> status = "Думает над ответом"
                        }
                    }
                    "content_block_delta" -> {
                        val delta = e.optJSONObject("delta") ?: return
                        if (!delta.has("text") || delta.isNull("text")) return
                        val text = delta.optString("text")
                        val at = blocks[index] ?: return
                        messages[at].text += text
                        status = "Пишет ответ"
                    }
                }
            }
            "assistant" -> {
                val content = data.optJSONObject("message")?.optJSONArray("content") ?: return
                for (i in 0 until content.length()) {
                    val b = content.optJSONObject(i) ?: continue
                    when (b.optString("type")) {
                        "text" -> {
                            val text = b.optString("text")
                            // Content indices differ from stream indices: match the streamed text.
                            val streamed = blocks.entries.filter { text.startsWith(messages[it.value].text) }.minByOrNull { it.key }
                            if (streamed != null) {
                                blocks.remove(streamed.key)
                                messages[streamed.value].text = text
                                stamp(messages[streamed.value], data)
                            } else if (text.isNotEmpty() && text.trim() != "No response requested.") {
                                val msg = Message("assistant", text)
                                stamp(msg, data)
                                messages.add(msg)
                            }
                        }
                        "tool_use" -> {
                            val id = b.optString("id")
                            if (messages.takeLast(20).none { it.role == "tool" && it.toolId == id }) {
                                val msg = Message("tool", tool = b.optString("name", "Действие"), input = b.optJSONObject("input") ?: JSONObject(), toolId = id)
                                stamp(msg, data)
                                messages.add(msg)
                            }
                            status = toolName(b.optString("name"))
                        }
                    }
                }
            }
            "user" -> {
                val content = data.optJSONObject("message")?.optJSONArray("content") ?: return
                for (i in 0 until content.length()) {
                    val b = content.optJSONObject(i) ?: continue
                    if (b.optString("type") == "tool_result") {
                        messages.add(Message("tool_result", resultText(b.opt("content")).take(24000), toolId = b.optString("tool_use_id"), failed = b.optBoolean("is_error")))
                    }
                }
            }
            "result" -> {
                busy = false
                status = ""
                if (data.optBoolean("is_error")) {
                    error = data.optJSONArray("errors")?.let { a -> (0 until a.length()).joinToString("\n") { a.optString(it) } }
                        ?: data.optString("result", "Агент не смог завершить задачу.")
                } else if (messages.drop(userAt + 1).none { it.role == "assistant" && it.text.isNotEmpty() }) {
                    data.optString("result").takeIf { it.isNotEmpty() }?.let {
                        val msg = Message("assistant", it)
                        stamp(msg, data)
                        messages.add(msg)
                    }
                }
            }
        }
    }

    fun loadHistory(rows: JSONArray) {
        messages.clear()
        blocks.clear()
        for (i in 0 until rows.length()) {
            val r = rows.optJSONObject(i) ?: continue
            messages.add(
                Message(
                    role = r.optString("role"),
                    text = r.optString("text"),
                    tool = r.optString("tool"),
                    input = r.optJSONObject("input") ?: JSONObject(),
                    toolId = r.optString("tool_id"),
                    failed = r.optBoolean("failed"),
                    images = r.optJSONArray("images")?.length() ?: 0,
                    backend = r.optString("backend"),
                    model = r.optString("model"),
                    actor = r.optString("actor"),
                    mode = r.optString("mode"),
                    effort = r.optString("effort"),
                    steer = r.optBoolean("steer", false),
                    agentName = r.optString("agent_name"),
                )
            )
        }
    }

    companion object {
        fun resultText(content: Any?): String = when (content) {
            is String -> content
            is JSONArray -> (0 until content.length()).mapNotNull { idx ->
                content.optJSONObject(idx)?.takeIf { it.has("text") && !it.isNull("text") }?.optString("text")
            }.joinToString("\n")
            else -> ""
        }
    }
}

typealias ChatMessage = Message

enum class ToolPhase(val label: String) {
    Research("ПОИСК"),
    Edit("ПРАВКА"),
    Terminal("КОМАНДА"),
    Subagent("СУБАГЕНТ"),
    Desktop("ЭТОТ ПК"),
    Other("ШАГ"),
}

data class SubagentTrace(
    val id: String,
    val role: String,
    val summary: String,
    val model: String = "",
    /** 0 = running, 1 = completed, 2 = failed */
    val state: Int = 1,
) {
    val running: Boolean get() = state == 0
    val failed: Boolean get() = state == 2
}

data class TurnTelemetry(
    val totalActions: Int = 0,
    val doneActions: Int = 0,
    val runningActions: Int = 0,
    val failedActions: Int = 0,
    val reads: Int = 0,
    val edits: Int = 0,
    val commands: Int = 0,
    val delegations: Int = 0,
    val desktopOps: Int = 0,
    val activeHeadline: String = "",
    val activeTarget: String = "",
    val activePhase: ToolPhase = ToolPhase.Other,
    val subagents: List<SubagentTrace> = emptyList(),
)

fun classifyToolPhase(name: String): ToolPhase {
    val n = name.trim()
    return when {
        n in setOf(
            "Task", "Agent", "invoke_subagent", "spawn_agent",
            "define_subagent", "send_message", "swarm", "run_workflow",
        ) || n.startsWith("mcp__relay_cluster__") -> ToolPhase.Subagent
        n.startsWith("mcp__relay_desktop__") -> ToolPhase.Desktop
        n in setOf(
            "Read", "read_file", "Glob", "Grep", "grep_search", "find_by_name",
            "WebSearch", "WebFetch", "web_search", "list_dir", "view_file_outline", "view_code_item",
        ) -> ToolPhase.Research
        n in setOf(
            "Edit", "MultiEdit", "Write", "edit_file", "write_file",
            "apply_patch", "file_change", "NotebookEdit",
        ) -> ToolPhase.Edit
        n in setOf("Bash", "shell", "command_execution", "run_command", "SlashCommand") -> ToolPhase.Terminal
        else -> ToolPhase.Other
    }
}

fun extractSubagentTrace(call: Message, result: Message?, runningTurn: Boolean): SubagentTrace {
    val inp = call.input
    val rawRole = listOf("Role", "role", "subagent_type", "TypeName", "agent_type", "agent", "name")
        .firstNotNullOfOrNull { k -> inp.optString(k).trim().takeIf { it.isNotEmpty() } }
        ?: if (call.tool.startsWith("mcp__relay_cluster__")) {
            "Кластер · " + call.tool.removePrefix("mcp__relay_cluster__").replace('_', ' ')
        } else {
            "Помощник"
        }
    val summary = listOf("description", "Prompt", "prompt", "task", "brief", "Message", "message")
        .firstNotNullOfOrNull { k -> inp.optString(k).trim().takeIf { it.isNotEmpty() } }
        ?.lineSequence()?.firstOrNull()?.take(160).orEmpty()
    val model = listOf("Model", "model")
        .firstNotNullOfOrNull { k -> inp.optString(k).trim().takeIf { it.isNotEmpty() && it != "inherit" } }
        .orEmpty()
    val state = when {
        result?.failed == true -> 2
        result == null && runningTurn -> 0
        else -> 1
    }
    return SubagentTrace(
        id = call.toolId.ifEmpty { rawRole },
        role = rawRole,
        summary = summary,
        model = model,
        state = state,
    )
}

fun extractToolTarget(call: Message): String {
    val i = call.input
    if (classifyToolPhase(call.tool) == ToolPhase.Subagent) {
        val trace = extractSubagentTrace(call, null, false)
        return listOf(trace.role, trace.summary).filter { it.isNotEmpty() }.joinToString(" · ").take(140)
    }
    for (key in listOf("file_path", "path", "command", "CommandLine", "pattern", "query", "url")) {
        val v = i.optString(key).trim()
        if (v.isNotEmpty()) return v.lineSequence().first().take(140)
    }
    return toolSummary(call.tool, i)
}

fun analyzeTurnTelemetry(messages: List<ChatMessage>, busy: Boolean, status: String = ""): TurnTelemetry {
    val lastUser = messages.indexOfLast { it.role == "user" }
    val scope = if (lastUser >= 0) messages.subList(lastUser + 1, messages.size) else messages
    val resultsById = HashMap<String, ChatMessage>()
    for (m in scope) {
        if (m.role == "tool_result" && m.toolId.isNotEmpty()) {
            resultsById[m.toolId] = m
        }
    }
    var total = 0
    var done = 0
    var running = 0
    var failed = 0
    var reads = 0
    var edits = 0
    var commands = 0
    var delegations = 0
    var desktopOps = 0
    val subagents = mutableListOf<SubagentTrace>()
    var activeCall: ChatMessage? = null

    val calls = scope.filter { it.role == "tool" }
    for (call in calls) {
        total++
        val phase = classifyToolPhase(call.tool)
        when (phase) {
            ToolPhase.Research -> reads++
            ToolPhase.Edit -> edits++
            ToolPhase.Terminal -> commands++
            ToolPhase.Subagent -> delegations++
            ToolPhase.Desktop -> desktopOps++
            ToolPhase.Other -> {}
        }
        val res = if (call.toolId.isNotEmpty()) resultsById[call.toolId] else null
        when {
            res != null -> {
                done++
                if (res.failed) failed++
            }
            busy -> {
                running++
                if (activeCall == null) activeCall = call
            }
            else -> done++
        }
        if (phase == ToolPhase.Subagent) {
            subagents.add(extractSubagentTrace(call, res, busy))
        }
    }
    if (activeCall == null) activeCall = calls.lastOrNull()

    val activePhase = activeCall?.let { classifyToolPhase(it.tool) } ?: ToolPhase.Other
    val activeHeadline = when {
        busy && running > 0 && activeCall != null ->
            activeCall.input.optString("description").trim().ifEmpty { toolName(activeCall.tool) }
        busy && status.isNotBlank() -> status
        activeCall != null ->
            activeCall.input.optString("description").trim().ifEmpty { toolName(activeCall.tool) }
        busy -> "Анализирует контекст…"
        status.isNotBlank() -> status
        else -> "Готов"
    }
    val activeTarget = if (activeCall != null && (running > 0 || !busy)) extractToolTarget(activeCall) else ""

    return TurnTelemetry(
        totalActions = total,
        doneActions = done,
        runningActions = running,
        failedActions = failed,
        reads = reads,
        edits = edits,
        commands = commands,
        delegations = delegations,
        desktopOps = desktopOps,
        activeHeadline = activeHeadline,
        activeTarget = activeTarget,
        activePhase = activePhase,
        subagents = subagents,
    )
}

fun toolName(name: String) = when (name) {
    "Read", "read_file" -> "Чтение файла"
    "Edit", "MultiEdit", "edit_file", "apply_patch", "file_change" -> "Изменение файла"
    "Write", "write_file" -> "Создание файла"
    "Bash", "shell", "command_execution", "run_command" -> "Команда на сервере"
    "SlashCommand" -> "Команда агента"
    "Glob", "Grep", "grep_search", "find_by_name" -> "Поиск по проекту"
    "WebSearch", "WebFetch", "web_search" -> "Поиск в интернете"
    "TodoWrite" -> "План работы"
    "Task", "Agent", "invoke_subagent", "spawn_agent" -> "Помощник-субагент"
    "swarm", "run_workflow" -> "Рой агентов"
    else -> when {
        name.startsWith("mcp__relay_cluster__") -> "Кластер субагентов"
        name.startsWith("mcp__relay_desktop__") -> "Действие на ПК"
        name.startsWith("mcp__relay_board__") -> "Доска задач"
        else -> name.ifEmpty { "Действие" }
    }
}

/** One line that says what a tool call does: the command, the file, the query. */
fun toolSummary(tool: String, input: JSONObject): String {
    for (key in listOf("command", "CommandLine", "file_path", "path", "pattern", "query", "url", "description", "Prompt", "prompt", "Role", "subagent_type")) {
        val v = input.optString(key)
        if (v.isNotEmpty()) return v.lineSequence().first().take(200)
    }
    return ""
}
