package dev.relay.mobile

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], qualifiers = "w400dp-h860dp-xxhdpi")
class UiNewFeaturesTest {
    @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()

    @Test fun usageRequestsFollowInstalledAgentsAndReconnect() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(host = "test"))
        val requests = mutableListOf<JSONObject>()
        vm.sentForTest = { requests.add(it) }
        val backends = JSONObject().put("claude", JSONObject().put("installed", true))
            .put("agy", JSONObject().put("installed", true)).put("codex", JSONObject().put("installed", false))
        val hello = JSONObject().put("type", "hello").put("backends", backends)
        vm.handle(hello)
        vm.requestUsage()
        fun agents() = requests.filter { it.optString("op") == "usage" }.map { it.getString("backend") }
        assertEquals(listOf("claude", "agy"), agents())
        requests.clear()
        vm.handle(hello)
        assertEquals(listOf("claude", "agy"), agents())
        val rows = JSONArray().put(JSONObject().put("id", "seven_day").put("resets_at", 1900000000))
            .put(JSONObject().put("id", "five_hour").put("utilization", 0))
        vm.handle(JSONObject().put("type", "usage").put("backend", "agy").put("windows", rows))
        assertEquals(1, vm.usage["agy"]!!.windows.size)
        assertEquals(null, vm.usage["agy"]!!.windows.single().percent)
        vm.handle(JSONObject().put("type", "usage").put("windows", rows))
        assertEquals(0.0, vm.usage["claude"]!!.windows.last().percent)
    }

    @Test fun onlyStableNewerApksAreOffered() {
        assertEquals(true, Updates.newer("0.7.10", "0.7.9"))
        assertEquals(false, Updates.newer("0.7.5", "0.7.5"))
        assertEquals(false, Updates.newer("0.7.4", "0.7.5"))
        assertEquals(false, Updates.newer("0.7.6-beta", "0.7.5"))
    }

    @Test fun repeatedTestPermissionIsLimitedToRecognizedTests() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        val args = JSONObject().put("command", "cargo test")
        val request = JSONObject().put("sid", "chat").put("tool", "run_local_command").put("input", args)
        assertEquals(true, vm.mayRememberTest(request))
        args.put("command", "cargo test && whoami")
        assertEquals(false, vm.mayRememberTest(request))
        request.put("tool", "minecraft_test").put("input", JSONObject().put("mods", JSONArray().put(JSONObject().put("server_path", "/tmp/test.jar"))))
        assertEquals(false, vm.mayRememberTest(request))
    }

    @Test fun reconnectRestoresVerifiedSessionUntilExplicitRelock() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(host = "test", fingerprint = "trusted-pin"))
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }
        val hello = JSONObject().put("type", "hello").put("home", "/tmp")
        vm.handle(hello)
        vm.newChat()
        val sid = vm.chat()!!.sid
        val locked = JSONObject().put("type", "sessions").put("sessions", JSONArray().put(
            JSONObject().put("id", sid).put("title", "Закрыто").put("cwd", "/tmp").put("locked", true)))
        vm.unlockSession(sid, "fixture")
        vm.handle(JSONObject().put("type", "session_unlock_result").put("sid", sid).put("ok", true))
        sent.clear()
        vm.handle(hello)
        vm.handle(locked)
        vm.handle(locked)
        assertEquals(listOf("fixture"), sent.filter { it.optString("op") == "session_unlock" }.map { it.getString("password") })
        vm.relockSession(sid)
        sent.clear()
        vm.handle(hello)
        vm.handle(locked)
        assertEquals(false, sent.any { it.optString("op") == "session_unlock" })
    }

    @Test fun temporaryPasswordWorksWhenReopeningSameServerAndClearsOnHostChange() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        val server = ServerProfile(host = "first", rememberPassword = false)
        vm.saveServer(server, "fixture", false)
        assertEquals("fixture", vm.savedPassword(server))
        val moved = server.copy(host = "second")
        vm.saveServer(moved, null, false)
        assertEquals(null, vm.savedPassword(moved))
    }

    @Test @Config(sdk = [34], qualifiers = "w320dp-h480dp-xhdpi")
    fun narrowChatOpensPhoneTestsAndKeepsComposerAccessible() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "PC", host = "localhost"))
        vm.handle(JSONObject().put("type","hello").put("home","/tmp").put("protocol",4))
        vm.newChat()
        rule.setContent { RelayApp(vm) }
        rule.onAllNodesWithContentDescription("Поле сообщения")[0].assertExists()
        rule.onNodeWithText("Лимиты").performClick()
        rule.onNodeWithText("Лимиты агентов").assertExists()
        rule.onAllNodesWithContentDescription("Закрыть лимиты")[0].performClick()
        rule.onAllNodesWithContentDescription("Тесты на ПК")[0].performClick()
        rule.onNodeWithText("Связать с ПК").assertExists()
    }

    @Test fun oneClickAgentInstallBannerAndLockedSessionShield() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }
        vm.attach(ServerProfile(name = "Prod", host = "10.0.0.1", defaultBackend = "codex"))
        val backends = JSONObject()
            .put("claude", JSONObject().put("installed", true).put("version", "2.1.12"))
            .put("codex", JSONObject().put("installed", false).put("version", ""))
            .put("agy", JSONObject().put("installed", true).put("version", "agy 2.0.4"))
            .put("api", JSONObject().put("installed", true).put("version", ""))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4).put("backends", backends))
        vm.newChat()
        val c = vm.chat()!!

        rule.setContent { RelayApp(vm) }
        rule.onNodeWithText("Установить Codex").performClick()
        rule.waitForIdle()
        val installReq = sent.last { it.optString("op") == "agent_install" }
        assertEquals("codex", installReq.getString("agent"))

        vm.handle(
            JSONObject()
                .put("type", "agent_install")
                .put("agent", "codex")
                .put("running", false)
                .put("ok", true)
                .put("installed", true)
                .put("version", "codex 0.29.0")
                .put("message", "OpenAI Codex CLI готов (codex 0.29.0).")
        )
        rule.waitForIdle()
        assertEquals(true, vm.backendsStatus["codex"]?.installed)

        vm.handle(JSONObject().put("type", "session_locked").put("sid", c.sid))
        rule.waitForIdle()
        rule.onNodeWithText("Сессия защищена паролем").assertExists()
        rule.onNodeWithText("Разблокировать").assertExists()
    }

    @Test fun liveAgentTelemetryPodPipelineAndAgentRadarSheet() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Cluster", host = "10.0.0.2", defaultBackend = "agy"))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4))
        val now = System.currentTimeMillis() / 1000.0
        vm.handle(
            JSONObject().put("type", "sessions").put(
                "sessions",
                JSONArray(
                    listOf(
                        JSONObject().put("id", "s-live").put("title", "Оптимизация рендера").put("cwd", "/srv/engine").put("modified", now - 20).put("running", true).put("backend", "agy"),
                        JSONObject().put("id", "s-codex").put("title", "Миграция БД").put("cwd", "/srv/db").put("modified", now - 180).put("running", true).put("backend", "codex"),
                    )
                ),
            )
        )

        rule.setContent { RelayApp(vm) }
        rule.waitForIdle()

        // Active Agents Banner is shown on SessionsScreen when >=1 session is running
        rule.onNodeWithText("2 агента активны").assertExists()
        rule.onNodeWithText("Радар").performClick()
        rule.waitForIdle()

        // AgentRadarSheet ("Радар агентов") opens and displays active topology nodes
        rule.onNodeWithText("Радар агентов").assertExists()
        rule.onNodeWithText("Выполняется в engine · открыть →").performClick()
        rule.waitForIdle()

        // 1-tap jump opens the selected session in ChatScreen
        assertEquals(Screen.Chat, vm.screen)
        assertEquals("s-live", vm.selected)

        // Feed live pipeline events with a subagent + running command and ensure Compose settles cleanly!
        val chat = vm.chat()!!
        vm.handle(
            JSONObject().put("type", "history").put("sid", "s-live").put(
                "messages",
                JSONArray(
                    listOf(
                        JSONObject().put("role", "user").put("text", "Ускорь шейдеры"),
                        JSONObject().put("role", "tool").put("tool", "invoke_subagent").put("tool_id", "sub1")
                            .put("input", JSONObject().put("Role", "Шейдер-аудитор").put("Prompt", "Проверь матричные проекции")),
                        JSONObject().put("role", "tool_result").put("tool_id", "sub1").put("text", "Найдено 2 узких места"),
                        JSONObject().put("role", "tool").put("tool", "run_command").put("tool_id", "cmd1")
                            .put("input", JSONObject().put("CommandLine", "./gradlew benchmark").put("description", "Замер FPS")),
                    )
                ),
            )
        )
        rule.waitForIdle()
        assertEquals(true, chat.transcript.busy)
        rule.onNodeWithText("Конвейер агента", substring = true).assertExists()
        rule.onAllNodesWithContentDescription("Радар агентов")[0].performClick()
        rule.waitForIdle()
        rule.onNodeWithText("Радар агентов").assertExists()
        rule.onNodeWithText("Закрыть").performClick()
        rule.waitForIdle()
    }
}
