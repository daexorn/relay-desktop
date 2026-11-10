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

/** Messages sent mid-answer, live model switches, the catalog and running sessions. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class QueueTest {
    private fun online(): RelayViewModel {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Test", host = "example"))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("protocol", 4))
        vm.handle(JSONObject().put("type", "catalog")
            .put("models", JSONArray().put(JSONObject().put("value", "default").put("displayName", "Default"))
                .put(JSONObject().put("value", "claude-fable-5-2").put("displayName", "Fable 5.2")))
            .put("commands", JSONArray().put(JSONObject().put("name", "compact").put("description", "Сжать"))
                .put(JSONObject().put("name", "context"))))
        return vm
    }

    @Test fun catalogGivesModelsAndCommands() {
        val vm = online()
        assertEquals(listOf("default", "claude-fable-5-2"), vm.models.map { it.value })
        assertEquals("Fable 5.2", modelLabel(vm.models, "claude-fable-5-2"))
        assertEquals("claude-fable-5-2", resolveModel(vm.models, "fable"))
        assertEquals(listOf("compact", "context"), slashMatches("/c", vm.commands).map { it.name })
        assertTrue(slashMatches("/compact", vm.commands).isEmpty())
        assertTrue(slashMatches("/c x", vm.commands).isEmpty())
    }

    @Test fun messagesSentMidAnswerWaitTheirTurn() {
        val vm = online()
        vm.newChat()
        val c = vm.chat()!!
        c.draft = "первое"; vm.submit()
        assertTrue(c.transcript.busy)
        c.draft = "второе"; vm.submit()
        assertEquals(listOf("второе"), c.queued.toList())
        assertEquals(1, c.transcript.messages.count { it.role == "user" })
        // Claude finishes the first answer and moves on to the queued message.
        vm.handle(JSONObject().put("type", "claude").put("sid", c.sid).put("data", JSONObject().put("type", "result").put("result", "ок")))
        assertTrue(c.queued.isEmpty())
        assertEquals("второе", c.transcript.messages.last { it.role == "user" }.text)
        assertTrue(c.transcript.busy)
        vm.handle(JSONObject().put("type", "run_state").put("sid", c.sid).put("busy", false))
        assertFalse(c.transcript.busy)
    }

    @Test fun modelCommandWorksMidAnswerAndRunningShows() {
        val vm = online()
        vm.newChat()
        val c = vm.chat()!!
        c.draft = "задача"; vm.submit()
        c.draft = "/model fable"; vm.submit()
        assertEquals("claude-fable-5-2", c.model)
        assertTrue(c.queued.isEmpty())
        assertEquals("", c.draft)
        vm.handle(JSONObject().put("type", "run_state").put("sid", "other").put("busy", true))
        assertTrue("other" in vm.running)
    }
}
