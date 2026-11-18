package dev.relay.mobile

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
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
}
