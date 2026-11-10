package dev.relay.mobile

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

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
) {
    fun json() = JSONObject().put("id", id).put("name", name).put("host", host).put("port", port)
        .put("user", user).put("cwd", cwd).put("fingerprint", fingerprint ?: JSONObject.NULL)
        .put("model", model).put("mode", mode).put("effort", effort)

    companion object {
        fun from(o: JSONObject) = ServerProfile(
            o.optString("id"), o.optString("name"), o.optString("host"), o.optInt("port", 22),
            o.optString("user", "root"), o.optString("cwd", "~"),
            o.optString("fingerprint").takeIf { it.isNotEmpty() && it != "null" },
            o.optString("model", "default"), o.optString("mode", "auto"), o.optString("effort", "default"),
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
)

/** A model or slash command exactly as the server's Claude Code lists it. */
data class ModelInfo(val value: String, val name: String, val description: String)
data class CommandInfo(val name: String, val description: String, val hint: String)

fun modelLabel(models: List<ModelInfo>, value: String): String = when {
    value == "default" -> "Модель: авто"
    else -> models.firstOrNull { it.value == value }?.name?.takeIf { it.isNotEmpty() }
        ?: value.replaceFirstChar { it.uppercase() }
}

/** `fable`, `Fable 5.2` or a full id → the value Claude Code accepts; null when unknown. */
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
)

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
}

/** Port of the desktop `reduce`: folds Claude's stream events into the message list. */
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
                        val text = e.optJSONObject("delta")?.optString("text", null) ?: return
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
                        ?: data.optString("result", "Claude не смог завершить задачу.")
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
            is JSONArray -> (0 until content.length()).mapNotNull { content.optJSONObject(it)?.optString("text", null) }.joinToString("\n")
            else -> ""
        }
    }
}

fun toolName(name: String) = when (name) {
    "Read" -> "Чтение файла"
    "Edit", "MultiEdit" -> "Изменение файла"
    "Write" -> "Создание файла"
    "Bash" -> "Команда на сервере"
    "Glob", "Grep" -> "Поиск по проекту"
    "WebSearch", "WebFetch" -> "Поиск в интернете"
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
