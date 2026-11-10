package dev.relay.mobile

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Taps through the app like a person would; the screens must settle (no endless redraws). */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], qualifiers = "w400dp-h860dp-xxhdpi")
class UiFlowTest {
    @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()

    // Forms live in Dialog windows; a text field inside a Dialog never reports idle under
    // Robolectric, so they are not covered here.

    @Test fun themePickerChangesTheTheme() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.screen = Screen.Settings
        rule.setContent { RelayApp(vm) }
        rule.onNodeWithText("Дракула").performClick()
        rule.waitForIdle()
        assertEquals("dracula", vm.theme)
    }

    @Test fun typingInANewChatEnablesSendOnlyWhenOnline() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.attach(ServerProfile(name = "Test", host = "example"))
        vm.handle(org.json.JSONObject().put("type", "hello").put("home", "/root"))
        vm.newChat()
        rule.setContent { RelayApp(vm) }
        rule.onNodeWithText("Напиши задачу… или / для команд").performTextInput("Привет")
        rule.waitForIdle()
        assertEquals("Привет", vm.chat()?.draft)
        rule.onNodeWithText("С чего начнём?").assertExists()
    }
}
