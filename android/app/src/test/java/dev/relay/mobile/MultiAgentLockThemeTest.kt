package dev.relay.mobile

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class MultiAgentLockThemeTest {
    private fun onlineVm(defaultBackend: String = ""): RelayViewModel {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Prod", host = "10.0.0.1", defaultBackend = defaultBackend))
        val backends = JSONObject()
            .put("claude", JSONObject().put("installed", true).put("version", "2.1.12"))
            .put("codex", JSONObject().put("installed", false).put("version", ""))
            .put("agy", JSONObject().put("installed", true).put("version", "agy 2.0.4"))
            .put("api", JSONObject().put("installed", true).put("version", ""))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4).put("backends", backends))
        return vm
    }

    @Test fun allTwentyFourThemesSyncWithDesktopSpecs() {
        assertEquals(40, THEMES.size)
        val dark = THEMES.filter { !it.light }
        val light = THEMES.filter { it.light }
        assertEquals(30, dark.size)
        assertEquals(10, light.size)
        val expectedIds = listOf(
            "graphite", "midnight", "obsidian", "tokyo", "catppuccin", "gruvbox",
            "kanagawa", "everforest", "cyber", "aurora", "forest", "plum",
            "ember", "ocean", "nord", "rose", "dracula", "oled",
            "monokai", "ayu", "synthwave", "cobalt", "matrix", "vesper",
            "poimandres", "horizon", "zenburn", "sakura_night", "arctic_void", "copper",
            "paper", "sand", "mint", "latte", "solarized", "frost",
            "lavender", "peach", "sage", "sky_day",
        )
        assertEquals(expectedIds, THEMES.map { it.id })
        for (t in THEMES) {
            assertEquals(8, t.colors.size)
            val p = palette(t.id)
            assertEquals(t.light, p.light)
            assertEquals(t.light, isLight(t.id))
            assertTrue(p.elevated.alpha == 1f)
            assertTrue(p.subtleAccent.alpha in 0.1f..0.2f)
            assertTrue(p.borderSubtle.alpha > 0.5f)
        }
        val slash = builtinSlashCommands().map { it.name }
        assertTrue(slash.containsAll(listOf("usage", "cost", "status", "clear", "help")))

        val msgs = listOf(
            Message(role = "user", text = "ку", backend = "claude", model = "claude-sonnet-4-6", mode = "auto", actor = "maisdev"),
            Message(role = "assistant", text = "Привет!", backend = "claude", model = "claude-sonnet-4-6", mode = "auto", actor = "maisdev"),
            Message(role = "user", text = "проверь тесты", backend = "agy", model = "gemini-3.1-pro", mode = "plan", actor = "maisdev"),
            Message(role = "tool", tool = "run_command", backend = "agy"),
            Message(role = "assistant", text = "Готово", backend = "agy", model = "gemini-3.1-pro", mode = "plan", actor = "maisdev"),
        )
        val attrs = computeAttributions(msgs, "codex", busy = false)
        assertEquals("claude", attrs[1].backend)
        assertEquals("Claude Code", attrs[1].who)
        assertTrue(attrs[1].toWhom.contains("maisdev"))
        assertTrue(attrs[1].how.contains("Sonnet 4.6"))
        assertEquals("agy", attrs[4].backend)
        assertEquals("Antigravity (agy)", attrs[4].who)
        assertTrue(attrs[4].how.contains("Gemini 3.1 Pro"))
        assertTrue(attrs[4].how.contains("через 1 шаг"))
    }

    @Test fun codexAndAgyBackendSwitchingAndCatalogs() {
        val vm = onlineVm()
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }

        vm.newChat()
        val c = vm.chat()!!
        assertEquals("claude", c.activeBackend())

        // Switch to Codex: requests codex catalog and falls back to default Codex models until catalog arrives
        vm.setBackend(c, "codex")
        assertEquals("codex", c.activeBackend())
        assertFalse(c.api)
        assertTrue(sent.any { it.optString("op") == "catalog" && it.optString("backend") == "codex" })
        assertTrue(vm.catalogFor("codex").any { it.value == "gpt-5.4" })

        // Server delivers Codex catalog
        vm.handle(
            JSONObject()
                .put("type", "catalog")
                .put("backend", "codex")
                .put("models", JSONArray().put(JSONObject().put("value", "gpt-5.4-codex-max").put("displayName", "GPT-5.4 Codex Max")))
        )
        assertEquals(listOf("gpt-5.4-codex-max"), vm.codexModels.map { it.value })
        assertTrue(vm.models.isEmpty())

        c.updateSelectedModel("gpt-5.4-codex-max")
        c.draft = "Оптимизируй SQL-запрос"
        vm.submit()
        val codexPrompt = sent.last { it.optString("op") == "prompt" }
        assertEquals("codex", codexPrompt.getString("backend"))
        assertEquals("gpt-5.4-codex-max", codexPrompt.getString("model"))

        // Cannot switch backend while busy
        vm.setBackend(c, "agy")
        assertEquals("codex", c.activeBackend())

        // Finish run, then switch to Antigravity (agy)
        vm.handle(JSONObject().put("type", "run_state").put("sid", c.sid).put("busy", false))
        vm.setBackend(c, "agy")
        assertEquals("agy", c.activeBackend())
        assertTrue(sent.any { it.optString("op") == "catalog" && it.optString("backend") == "agy" })

        vm.handle(
            JSONObject()
                .put("type", "catalog")
                .put("backend", "agy")
                .put("models", JSONArray().put(JSONObject().put("value", "gemini-3.1-pro-preview").put("displayName", "Gemini 3.1 Pro")))
        )
        c.updateSelectedModel("gemini-3.1-pro-preview")
        c.draft = "Проверь тесты"
        vm.submit()
        val agyPrompt = sent.last { it.optString("op") == "prompt" }
        assertEquals("agy", agyPrompt.getString("backend"))
        assertEquals("gemini-3.1-pro-preview", agyPrompt.getString("model"))

        // ServerProfile round-trip preserves defaultBackend, codexModel, agyModel without leaking keys
        val saved = ServerProfile(name = "S", host = "h", defaultBackend = "agy", codexModel = "gpt-5.4", agyModel = "gemini-3.1-pro-preview")
        val restored = ServerProfile.from(saved.json())
        assertEquals("agy", restored.effectiveBackend())
        assertEquals("gpt-5.4", restored.codexModel)
        assertEquals("gemini-3.1-pro-preview", restored.agyModel)
    }

    @Test fun privateSessionLockUnlockAndRelockClearsSensitiveMessages() {
        val vm = onlineVm()
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }

        vm.handle(
            JSONObject().put("type", "sessions").put(
                "sessions",
                JSONArray().put(
                    JSONObject()
                        .put("id", "secret-1")
                        .put("title", "Секретный релиз")
                        .put("cwd", "/srv/app")
                        .put("locked", true)
                        .put("unlocked", false)
                )
            )
        )
        val s = vm.sessions.single()
        assertTrue(s.isLockedGate())

        // Deleting a locked session is blocked with a clear notice
        vm.delete(s)
        assertTrue(sent.none { it.optString("op") == "delete_session" })
        assertTrue(vm.notice.contains("паролем"))

        // Opening a locked session gates history and composer
        vm.open(s)
        val c = vm.chat()!!
        assertTrue(c.isLockedGate())
        assertFalse(vm.canSend(c))

        // Setting/changing lock sends session_lock op
        vm.lockSession("secret-1", password = "new-password", currentPassword = "old-password", remove = false)
        val lockReq = sent.last { it.optString("op") == "session_lock" }
        assertEquals("secret-1", lockReq.getString("sid"))
        assertEquals("new-password", lockReq.getString("password"))
        assertEquals("old-password", lockReq.getString("current_password"))
        assertFalse(lockReq.getBoolean("remove"))

        // Wrong password keeps session locked and sets lockError
        vm.unlockSession("secret-1", "wrong")
        val unlockReq = sent.last { it.optString("op") == "session_unlock" }
        assertEquals("wrong", unlockReq.getString("password"))
        vm.handle(JSONObject().put("type", "session_unlock_result").put("sid", "secret-1").put("ok", false).put("message", "Неверный пароль сессии."))
        assertTrue(c.isLockedGate())
        assertEquals("Неверный пароль сессии.", c.lockError)

        // Correct password unlocks and immediately fetches history
        vm.unlockSession("secret-1", "correct-horse")
        vm.handle(JSONObject().put("type", "session_unlock_result").put("sid", "secret-1").put("ok", true))
        assertFalse(c.isLockedGate())
        assertEquals("", c.lockError)
        assertTrue(sent.any { it.optString("op") == "history" && it.optString("sid") == "secret-1" })

        vm.handle(
            JSONObject().put("type", "history").put("sid", "secret-1").put(
                "messages",
                JSONArray().put(JSONObject().put("role", "assistant").put("text", "Конфиденциальные данные"))
            )
        )
        vm.handle(JSONObject().put("type", "permission").put("sid", "secret-1").put("id", "p1").put("tool", "Bash"))
        assertEquals(1, c.transcript.messages.size)
        assertEquals(1, c.permissions.size)

        // Relocking immediately wipes in-memory messages and pending permissions
        vm.relockSession("secret-1")
        assertTrue(sent.any { it.optString("op") == "session_relock" && it.optString("sid") == "secret-1" })
        vm.handle(JSONObject().put("type", "session_lock_changed").put("sid", "secret-1").put("locked", true).put("unlocked", false))
        assertTrue(c.isLockedGate())
        assertTrue(c.transcript.messages.isEmpty())
        assertTrue(c.permissions.isEmpty())
    }

    @Test fun agentInstallBackendsStatusAndServerOAuth() {
        val vm = onlineVm(defaultBackend = "codex")
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }

        assertEquals(true, vm.backendsStatus["claude"]?.installed)
        assertEquals(false, vm.backendsStatus["codex"]?.installed)
        assertEquals("agy 2.0.4", vm.backendsStatus["agy"]?.version)

        vm.installAgent("codex", "install")
        assertEquals(true, vm.installingAgents["codex"])
        val installReq = sent.last { it.optString("op") == "agent_install" }
        assertEquals("codex", installReq.getString("agent"))
        assertEquals("install", installReq.getString("action"))

        vm.handle(
            JSONObject()
                .put("type", "agent_install")
                .put("agent", "codex")
                .put("running", false)
                .put("ok", true)
                .put("installed", true)
                .put("version", "codex 0.29.0")
                .put("message", "OpenAI Codex CLI готов.")
                .put("output", "installed ok")
        )
        assertEquals(false, vm.installingAgents["codex"])
        assertEquals(true, vm.backendsStatus["codex"]?.installed)
        assertEquals("codex 0.29.0", vm.backendsStatus["codex"]?.version)
        assertEquals("installed ok", vm.agentLogs["codex"])

        // backends_status refresh updates versions cleanly
        vm.handle(
            JSONObject().put(
                "type",
                "backends_status",
            ).put(
                "backends",
                JSONObject()
                    .put("claude", JSONObject().put("installed", true).put("version", "2.1.15"))
                    .put("codex", JSONObject().put("installed", true).put("version", "0.30.0"))
                    .put("agy", JSONObject().put("installed", true).put("version", "agy 2.1.0"))
            )
        )
        assertEquals("0.30.0", vm.backendsStatus["codex"]?.version)
        assertEquals("agy 2.1.0", vm.backendsStatus["agy"]?.version)

        // Server OAuth flow & frame ACK
        vm.startAuth("agy")
        val startReq = sent.last { it.optString("op") == "auth_start" }
        val flowId = startReq.getString("id")
        assertEquals("agy", startReq.getString("provider"))

        vm.handle(
            JSONObject()
                .put("type", "auth_frame")
                .put("id", flowId)
                .put("frame", 7)
                .put("width", 1280)
                .put("height", 960)
                .put("data", "")
        )
        val ack = sent.last { it.optString("op") == "auth_browser_ack" }
        assertEquals(flowId, ack.getString("id"))
        assertEquals(7, ack.getInt("frame"))

        vm.submitAuthInput("oauth-code-123")
        val inputReq = sent.last { it.optString("op") == "auth_input" }
        assertEquals("oauth-code-123", inputReq.getString("code"))

        vm.cancelAuth()
        assertEquals("auth_cancel", sent.last().optString("op"))

        // Bridge.helper embeds CODEX_SOURCE, AGY_SOURCE, INSTALLER_SOURCE, and DEFAULT_BACKEND
        val helperText = Bridge.helper(apiDefault = false, defaultBackend = "agy") { name -> "# $name" }.toString(Charsets.UTF_8)
        assertTrue(helperText.contains("CODEX_SOURCE = \"# codex_engine.py\""))
        assertTrue(helperText.contains("AGY_SOURCE = \"# agy_engine.py\""))
        assertTrue(helperText.contains("INSTALLER_SOURCE = \"# agent_installer.py\""))
        assertTrue(helperText.contains("DEFAULT_BACKEND = \"agy\""))
    }

    @Test fun engineSignaturesAndTurnTelemetrySubagentAnalysis() {
        assertEquals(androidx.compose.ui.graphics.Color(0xFFEC8A5C), backendColor("claude", light = false))
        assertEquals(androidx.compose.ui.graphics.Color(0xFFCA6234), backendColor("claude", light = true))
        assertEquals(androidx.compose.ui.graphics.Color(0xFF3ED6A8), backendColor("codex", light = false))
        assertEquals(androidx.compose.ui.graphics.Color(0xFF169873), backendColor("codex", light = true))
        assertEquals(androidx.compose.ui.graphics.Color(0xFFA482FF), backendColor("agy", light = false))
        assertEquals(androidx.compose.ui.graphics.Color(0xFF6E4AE0), backendColor("agy", light = true))
        assertEquals(androidx.compose.ui.graphics.Color(0xFF5EA8FF), backendColor("api", light = false))
        assertEquals(androidx.compose.ui.graphics.Color(0xFF256CDA), backendColor("api", light = true))

        assertEquals("ПОИСК", classifyToolPhase("grep_search").label)
        assertEquals("ПРАВКА", classifyToolPhase("edit_file").label)
        assertEquals("КОМАНДА", classifyToolPhase("run_command").label)
        assertEquals("СУБАГЕНТ", classifyToolPhase("invoke_subagent").label)
        assertEquals("СУБАГЕНТ", classifyToolPhase("spawn_agent").label)
        assertEquals("СУБАГЕНТ", classifyToolPhase("mcp__relay_cluster__delegate").label)
        assertEquals("ЭТОТ ПК", classifyToolPhase("mcp__relay_desktop__screenshot").label)
        assertEquals("ШАГ", classifyToolPhase("TodoWrite").label)

        val msgs = listOf(
            ChatMessage(role = "user", text = "Проверь и обнови модуль"),
            ChatMessage(role = "tool", tool = "Grep", toolId = "1", input = JSONObject().put("pattern", "Combat")),
            ChatMessage(role = "tool_result", toolId = "1", text = "src/Combat.kt:12"),
            ChatMessage(
                role = "tool",
                tool = "invoke_subagent",
                toolId = "2",
                input = JSONObject().put("Role", "Аудитор безопасности").put("Prompt", "Проверь граничные случаи").put("Model", "pro"),
            ),
            ChatMessage(role = "tool_result", toolId = "2", text = "Уязвимостей нет"),
            ChatMessage(
                role = "tool",
                tool = "Edit",
                toolId = "3",
                input = JSONObject().put("file_path", "src/Combat.kt").put("old_string", "a").put("new_string", "b"),
            ),
            ChatMessage(role = "tool_result", toolId = "3", failed = true, text = "conflict"),
            ChatMessage(
                role = "tool",
                tool = "mcp__relay_cluster__spawn",
                toolId = "4",
                input = JSONObject().put("agent", "Тестировщик").put("task", "Прогон интеграционных тестов"),
            ),
            ChatMessage(
                role = "tool",
                tool = "Bash",
                toolId = "5",
                input = JSONObject().put("command", "./gradlew test").put("description", "Запуск модульных тестов"),
            ),
        )

        val tel = analyzeTurnTelemetry(msgs, busy = true, status = "Работает")
        assertEquals(5, tel.totalActions)
        assertEquals(3, tel.doneActions)
        assertEquals(2, tel.runningActions)
        assertEquals(1, tel.failedActions)
        assertEquals(1, tel.reads)
        assertEquals(1, tel.edits)
        assertEquals(1, tel.commands)
        assertEquals(2, tel.delegations)
        assertEquals(2, tel.subagents.size)
        assertEquals("Аудитор безопасности", tel.subagents[0].role)
        assertFalse(tel.subagents[0].running)
        assertEquals("Тестировщик", tel.subagents[1].role)
        assertTrue(tel.subagents[1].running)
        assertEquals(ToolPhase.Subagent, tel.activePhase)
    }
}
