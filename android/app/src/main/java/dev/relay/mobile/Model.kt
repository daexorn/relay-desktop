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
        ModelInfo("default", "По умолчанию (Codex)", "Модель профиля Codex CLI на сервере"),
        ModelInfo("gpt-5.4", "GPT-5.4", "Основная агентная модель OpenAI Codex"),
        ModelInfo("gpt-5.3-codex", "GPT-5.3 Codex", "Специализированная модель для кода"),
        ModelInfo("gpt-5.4-mini", "GPT-5.4 Mini", "Быстрая модель для простых задач"),
    )
    "agy" -> listOf(
        ModelInfo("default", "По умолчанию (AGY)", "Модель профиля Antigravity CLI на сервере"),
        ModelInfo("gemini-3.1-pro-preview", "Gemini 3.1 Pro", "Глубокое планирование и сложный код"),
        ModelInfo("gemini-3-flash-preview", "Gemini 3 Flash", "Быстрая агентная разработка"),
        ModelInfo("claude-sonnet-4-6", "Claude Sonnet 4.6 (AGY)", "Через Google Antigravity"),
    )
    "claude" -> listOf(
        ModelInfo("default", "Модель сервера", ""),
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

fun modelLabel(models: List<ModelInfo>, value: String): String = when {
    value == "default" || value.isBlank() -> "Модель: авто"
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

data class Folder(val id: String, val name: String)

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
)

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
                                messages.add(Message("assistant", block.optString("text")))
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
                            } else if (text.isNotEmpty() && text.trim() != "No response requested.") {
                                messages.add(Message("assistant", text))
                            }
                        }
                        "tool_use" -> {
                            val id = b.optString("id")
                            if (messages.takeLast(20).none { it.role == "tool" && it.toolId == id }) {
                                messages.add(Message("tool", tool = b.optString("name", "Действие"), input = b.optJSONObject("input") ?: JSONObject(), toolId = id))
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
                    data.optString("result").takeIf { it.isNotEmpty() }?.let { messages.add(Message("assistant", it)) }
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

fun toolName(name: String) = when (name) {
    "Read", "read_file" -> "Чтение файла"
    "Edit", "MultiEdit", "edit_file", "apply_patch", "file_change" -> "Изменение файла"
    "Write", "write_file" -> "Создание файла"
    "Bash", "shell", "command_execution", "run_command" -> "Команда на сервере"
    "SlashCommand" -> "Команда агента"
    "Glob", "Grep", "grep_search", "find_by_name" -> "Поиск по проекту"
    "WebSearch", "WebFetch", "web_search" -> "Поиск в интернете"
    "TodoWrite" -> "План работы"
    "Task", "Agent" -> "Помощник"
    else -> if (name.startsWith("mcp__relay_board__")) "Доска задач" else name.ifEmpty { "Действие" }
}

/** One line that says what a tool call does: the command, the file, the query. */
fun toolSummary(tool: String, input: JSONObject): String {
    for (key in listOf("command", "file_path", "path", "pattern", "query", "url", "description", "prompt")) {
        val v = input.optString(key)
        if (v.isNotEmpty()) return v.lineSequence().first().take(200)
    }
    return ""
}
