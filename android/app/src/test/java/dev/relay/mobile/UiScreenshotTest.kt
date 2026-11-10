package dev.relay.mobile

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import android.graphics.Canvas
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.shadows.ShadowLooper
import java.io.File

/** Renders the real screens with fake server events and saves them for a look. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w400dp-h860dp-xxhdpi")
class UiScreenshotTest {
    @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()
    private val out = File("build/screens").apply { mkdirs() }

    private fun shot(name: String) {
        // Infinite animations (typing dots) never let Compose go idle: drive the clock by hand.
        rule.mainClock.autoAdvance = false
        repeat(8) {
            ShadowLooper.idleMainLooper()
            rule.mainClock.advanceTimeBy(400)
            Thread.sleep(120)
        }
        val view = rule.activity.window.decorView.rootView
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        view.draw(Canvas(bitmap))
        File(out, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    private fun connected(theme: String = "graphite"): RelayViewModel {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.chooseTheme(theme)
        vm.attach(ServerProfile(name = "Production", host = "2.27.202.166", cwd = "/opt/sbrp-client"))
        vm.handle(JSONObject().put("type", "hello").put("home", "/root").put("version", "2.1").put("protocol", 4))
        vm.handle(JSONObject().put("type", "catalog")
            .put("models", JSONArray(listOf(
                JSONObject().put("value", "default").put("displayName", "Default").put("description", "Opus 5.5"),
                JSONObject().put("value", "opus").put("displayName", "Opus 5.5"),
                JSONObject().put("value", "claude-fable-5-2").put("displayName", "Fable 5.2"),
                JSONObject().put("value", "sonnet").put("displayName", "Sonnet 5.5"),
                JSONObject().put("value", "haiku").put("displayName", "Haiku 5.5"))))
            .put("commands", JSONArray(listOf(
                JSONObject().put("name", "compact").put("description", "Сжать историю разговора").put("hint", "[что сохранить]"),
                JSONObject().put("name", "context").put("description", "Чем занят контекст"),
                JSONObject().put("name", "code-review").put("description", "Проверить изменения")))))
        val now = System.currentTimeMillis() / 1000.0
        fun s(id: String, title: String, ago: Double, pinned: Double = 0.0, folder: String = "") = JSONObject()
            .put("id", id).put("title", title).put("cwd", "/opt/sbrp-client").put("modified", now - ago).put("pinned", pinned).put("folder", folder)
        vm.handle(
            JSONObject().put("type", "sessions")
                .put("sessions", JSONArray(listOf(
                    s("a", "Формулы для расчёта урона", 60.0, pinned = now),
                    s("b", "Якорь и цепь у штурвала", 3000.0, folder = "f1"),
                    s("c", "Паруса и ветер", 90000.0, folder = "f1"),
                    s("d", "Новая кузня: печь на домне", 7000.0).put("running", true),
                    s("e", "Проверка обновления сервера", 100000.0),
                )))
                .put("folders", JSONArray(listOf(JSONObject().put("id", "f1").put("name", "Корабли"))))
        )
        return vm
    }

    @Test fun servers() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        rule.setContent { RelayApp(vm) }
        shot("01-servers-empty")
    }

    @Test fun sessions() {
        val vm = connected()
        rule.setContent { RelayApp(vm) }
        shot("03-sessions")
    }

    private fun chat(vm: RelayViewModel) {
        vm.open(vm.sessions.first { it.id == "a" })
        val messages = JSONArray(listOf(
            JSONObject().put("role", "user").put("text", "Посчитай средний урон и покажи формулы 🙂"),
            JSONObject().put("role", "assistant").put("text", "Сначала посмотрю, как сейчас считается урон."),
            JSONObject().put("role", "tool").put("tool", "Bash").put("tool_id", "t1").put("input", JSONObject().put("command", "grep -rn damage src/ | head")),
            JSONObject().put("role", "tool_result").put("tool_id", "t1").put("text", "src/Combat.kt:12: val damage = base * crit\nsrc/Combat.kt:40: damage -= armor"),
            JSONObject().put("role", "tool").put("tool", "Edit").put("tool_id", "t2").put("input", JSONObject().put("file_path", "/opt/sbrp-client/src/Combat.kt").put("old_string", "damage -= armor").put("new_string", "damage = maxOf(0f, damage - armor)")),
            JSONObject().put("role", "tool_result").put("tool_id", "t2").put("text", "The file has been updated."),
            JSONObject().put("role", "tool").put("tool", "Bash").put("tool_id", "t3").put("input", JSONObject().put("command", "./gradlew test").put("description", "Прогнать тесты")),
            JSONObject().put("role", "tool_result").put("tool_id", "t3").put("failed", true).put("text", "> Task :compileKotlin\n> Task :test FAILED\nCombatTest > armorNeverHeals() FAILED\n    expected: <0.0> but was: <-3.0>\n3 tests completed, 1 failed"),
            JSONObject().put("role", "assistant").put("text", """
                Средний урон за удар:

                §§\bar{D} = B \cdot \left(1 + p \cdot (k - 1)\right) - A§§

                где §p§ — шанс крита, §k§ — множитель, а §\text{броня} = A§. За §n§ ударов:
                \begin{equation}
                \sum_{i=1}^{n} D_i \approx n \bar{D}
                \end{equation}

                | Оружие | §B§ | §p§ |
                |---|---|---|
                | Меч | 12 | 0.1 |

                - **Важно:** броня вычитается после крита 🛡
                - Цена в магазине §5, а не формула.
            """.trimIndent().replace('§', '$')),
        ))
        vm.handle(JSONObject().put("type", "history").put("sid", "a").put("messages", messages))
    }

    @Test fun chatWithFormulas() {
        val vm = connected()
        chat(vm)
        rule.setContent { RelayApp(vm) }
        shot("04-chat")
        rule.onNodeWithText("Действия", substring = true).performClick()
        rule.mainClock.advanceTimeBy(500)
        shot("05-chat-actions")
    }

    @Test fun permissionAndLightTheme() {
        val vm = connected("sand")
        chat(vm)
        vm.handle(JSONObject().put("type", "permission").put("sid", "a").put("id", "p1").put("tool", "Bash").put("input", JSONObject().put("command", "./gradlew build")))
        rule.setContent { RelayApp(vm) }
        shot("06-chat-light-permission")
    }

    @Test fun settings() {
        val vm = connected("nord")
        vm.screen = Screen.Settings
        rule.setContent { RelayApp(vm) }
        shot("07-settings")
    }

    @Test fun slashCommandsAndQueue() {
        val vm = connected()
        vm.newChat()
        val c = vm.chat()!!
        c.draft = "Собери мод"; vm.submit()
        c.draft = "И проверь в игре"; vm.submit()
        c.draft = "/co"
        rule.setContent { RelayApp(vm) }
        shot("09-slash-queue")
        c.draft = "Почему упал сервер?"
        c.picked.add(Picked("latest.log", "text/plain", ByteArray(48_000), null))
        c.picked.add(Picked("sbrp-client-0.7.300.jar", "application/java-archive", ByteArray(2_400_000), null))
        rule.mainClock.advanceTimeBy(400)
        shot("10-attachments")
    }
}
