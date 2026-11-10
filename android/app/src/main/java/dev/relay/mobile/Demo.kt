package dev.relay.mobile

import org.json.JSONArray
import org.json.JSONObject

/** A server that exists only on this phone: sessions, formulas, actions and a question, to look around. */
object Demo {
    const val SID = "demo-formulas"

    fun events(): List<JSONObject> {
        val now = System.currentTimeMillis() / 1000.0
        fun s(id: String, title: String, ago: Double, pinned: Double = 0.0, folder: String = "", running: Boolean = false) = JSONObject()
            .put("id", id).put("title", title).put("cwd", "/opt/sbrp-client").put("modified", now - ago).put("pinned", pinned).put("folder", folder).put("running", running)
        val tex = '$'
        val answer = """
            Средний урон за удар:

            $tex$tex\bar{D} = B \cdot \left(1 + p \cdot (k - 1)\right) - A$tex$tex

            где ${tex}p${tex} — шанс крита, ${tex}k${tex} — множитель, а ${tex}A${tex} — броня. Волны на воде:

            $tex$tex\vec P(x,z,t) = \begin{pmatrix} x + \sum_i \frac{Q_i}{k_i} D_{i,x}\cos\theta_i \\ \sum_i A_i \sin\theta_i \\ z + \sum_i \frac{Q_i}{k_i} D_{i,z}\cos\theta_i \end{pmatrix}, \quad \theta_i = k_i (\vec D_i \cdot \vec x) - \omega_i t$tex$tex

            Широкую формулу можно листать пальцем вбок.

            | Оружие | ${tex}B${tex} | ${tex}p${tex} |
            |---|---|---|
            | Меч | 12 | 0.1 |
            | Топор | 15 | 0.05 |

            Собрал мод: [sbrp-client-0.7.300.jar](/opt/sbrp-client/build/libs/sbrp-client-0.7.300.jar) — нажми «Скачать» под ответом 🙂
        """.trimIndent()
        val messages = JSONArray(listOf(
            JSONObject().put("role", "user").put("text", "Посчитай средний урон, покажи формулы и собери мод"),
            JSONObject().put("role", "assistant").put("text", "Сначала посмотрю, как сейчас считается урон."),
            JSONObject().put("role", "tool").put("tool", "Grep").put("tool_id", "t1").put("input", JSONObject().put("pattern", "damage").put("path", "src/")),
            JSONObject().put("role", "tool_result").put("tool_id", "t1").put("text", "src/Combat.kt:12: val damage = base * crit\nsrc/Combat.kt:40: damage -= armor"),
            JSONObject().put("role", "tool").put("tool", "Edit").put("tool_id", "t2").put("input", JSONObject().put("file_path", "/opt/sbrp-client/src/Combat.kt").put("old_string", "damage -= armor").put("new_string", "damage = maxOf(0f, damage - armor)")),
            JSONObject().put("role", "tool_result").put("tool_id", "t2").put("text", "The file has been updated."),
            JSONObject().put("role", "tool").put("tool", "Bash").put("tool_id", "t3").put("input", JSONObject().put("command", "./gradlew build").put("description", "Собрать мод")),
            JSONObject().put("role", "tool_result").put("tool_id", "t3").put("text", "> Task :compileKotlin\n> Task :jar\nBUILD SUCCESSFUL in 41s"),
            JSONObject().put("role", "assistant").put("text", answer),
        ))
        return listOf(
            JSONObject().put("type", "hello").put("home", "/root").put("version", "2.1 (Claude Code) · демо").put("protocol", 4),
            JSONObject().put("type", "catalog")
                .put("models", JSONArray(listOf(
                    JSONObject().put("value", "default").put("displayName", "Default").put("description", "Opus 5.5"),
                    JSONObject().put("value", "opus").put("displayName", "Opus 5.5"),
                    JSONObject().put("value", "claude-fable-5-2").put("displayName", "Fable 5.2"),
                    JSONObject().put("value", "sonnet").put("displayName", "Sonnet 5.5"),
                    JSONObject().put("value", "haiku").put("displayName", "Haiku 5.5"))))
                .put("commands", JSONArray(listOf(
                    JSONObject().put("name", "compact").put("description", "Сжать историю разговора").put("hint", "[что сохранить]"),
                    JSONObject().put("name", "context").put("description", "Чем занят контекст"),
                    JSONObject().put("name", "code-review").put("description", "Проверить изменения")))),
            JSONObject().put("type", "sessions")
                .put("sessions", JSONArray(listOf(
                    s(SID, "Формулы для расчёта урона", 60.0, pinned = now),
                    s("demo-anchor", "Якорь и цепь у штурвала", 3000.0, folder = "f1"),
                    s("demo-sails", "Паруса и ветер", 90000.0, folder = "f1"),
                    s("demo-forge", "Новая кузня: печь на домне", 600.0, running = true),
                    s("demo-update", "Проверка обновления сервера", 100000.0),
                )))
                .put("folders", JSONArray(listOf(JSONObject().put("id", "f1").put("name", "Корабли")))),
            JSONObject().put("type", "history").put("sid", SID).put("messages", messages),
        )
    }

    fun reply(text: String) = "Это демо-режим: сервера нет, поэтому Claude не отвечает по-настоящему. Ты написал: «${text.take(200)}». " +
        "Подключи свой сервер на первом экране, и всё будет работать так же."
}
