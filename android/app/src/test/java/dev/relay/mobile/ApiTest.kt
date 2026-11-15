package dev.relay.mobile

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class ApiTest {
    private val api = ApiProfile(baseUrl = "https://example.test/v1", model = "vendor/model", models = listOf("vendor/model"))
    private fun online(profile: ApiProfile = api): RelayViewModel {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "API test", host = "example", api = profile))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4))
        return vm
    }

    @Test fun oldProfilesMigrateAndPublicSettingsNeverContainAKey() {
        val old = ServerProfile.from(JSONObject().put("id", "old").put("host", "example"))
        assertFalse(old.api.enabled)
        val request = api.request("fixture-secret")
        assertEquals("fixture-secret", request.getString("key"))
        val public = ServerProfile(api = api).json().toString()
        assertFalse(public.contains("fixture-secret"))
        assertFalse(public.contains("\"key\""))
        assertEquals(api, ServerProfile.from(ServerProfile(api = api).json()).api)
    }

    @Test fun urlsWithEmbeddedCredentialsAreRejected() {
        for (url in listOf("file:///etc/passwd", "https://key@example.test/v1", "https://example.test/v1?key=secret", "https://example.test/v1#secret")) {
            assertNotNull(url, api.copy(baseUrl = url).error())
        }
        assertNull(api.error())
        assertEquals("Sonnet 5 5", modelLabel(listOf(ModelInfo("claude-sonnet-5-5","claude-sonnet-5-5","")),"claude-sonnet-5-5"))
    }

    @Test fun sourceSwitchKeepsTheSessionAndSendsTheApiModel() {
        val vm = online()
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }
        vm.newChat()
        val c = vm.chat()!!
        val sid = c.sid
        c.transcript.messages.add(Message("assistant", "Старый ответ Claude Code"))
        vm.setApiSource(c, true)
        c.draft = "Продолжить через API"
        vm.submit()
        val request = sent.last { it.optString("op") == "prompt" }
        assertEquals(sid, request.getString("sid"))
        assertEquals("api", request.getString("backend"))
        assertEquals("vendor/model", request.getString("model"))
        assertEquals(api.baseUrl, request.getJSONObject("api").getString("base_url"))
        assertTrue(c.transcript.messages.any { it.text == "Старый ответ Claude Code" })
        assertEquals("default", c.model)
        vm.setApiSource(c, false)
        assertTrue("Cannot switch source while the answer is running", c.api)
    }

    @Test fun missingConfigurationPreservesTheDraftAndAttachments() {
        val vm = online(ApiProfile())
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }
        vm.newChat()
        val c = vm.chat()!!
        vm.setApiSource(c, true)
        c.draft = "Не терять"
        c.picked.add(Picked("test.txt", "text/plain", byteArrayOf(1), null))
        vm.submit()
        assertEquals("Не терять", c.draft)
        assertEquals(1, c.picked.size)
        assertFalse(c.transcript.busy)
        assertTrue(sent.none { it.optString("op") == "prompt" })
    }

    @Test fun phoneDiscoversAnApiSessionFromAnotherDevice() {
        val vm = online()
        vm.handle(JSONObject().put("type", "sessions").put("sessions", JSONArray().put(
            JSONObject().put("id", "cross-device-api").put("title", "Общая история").put("cwd", "/root").put("backend", "api")
        )))
        vm.open(vm.sessions.single())
        val c = vm.chat()!!
        assertTrue(c.api)
        vm.handle(JSONObject().put("type", "run_state").put("sid", c.sid).put("busy", true).put("backend", "claude"))
        assertFalse(c.api)
    }

    @Test fun modelCheckDoesNotMakeAnInferenceRequestAndKeepsNativeCatalog() {
        val vm = online()
        val sent = mutableListOf<JSONObject>()
        vm.sentForTest = { sent.add(it) }
        vm.handle(JSONObject().put("type", "catalog").put("models", JSONArray().put(JSONObject().put("value", "opus").put("displayName", "Opus"))))
        vm.checkApi(vm.current!!, api, "fixture-secret")
        val request = sent.single()
        assertEquals("api_models", request.getString("op"))
        assertFalse(request.has("text"))
        vm.handle(JSONObject().put("type", "api_models").put("request", request.getString("request")).put("models", JSONArray().put(JSONObject().put("value", "vendor/next").put("displayName", "Next"))))
        assertEquals(listOf("vendor/next"), vm.apiCatalog().map { it.value })
        assertEquals(listOf("opus"), vm.models.map { it.value })
    }
}
