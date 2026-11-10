package dev.relay.mobile

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowLooper
import java.time.Duration

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class DemoTest {
    @Test fun demoWorksWithoutAServer() {
        val vm = RelayViewModel(ApplicationProvider.getApplicationContext())
        vm.startDemo()
        assertEquals(Link.Online, vm.link)
        assertEquals(5, vm.sessions.size)
        assertTrue("demo-forge" in vm.running)
        vm.open(vm.sessions.first { it.id == Demo.SID })
        ShadowLooper.idleMainLooper()
        val c = vm.chat()!!
        assertTrue(c.transcript.messages.any { it.role == "tool" })
        val answer = c.transcript.messages.last { it.role == "assistant" }.text
        assertEquals(listOf("/opt/sbrp-client/build/libs/sbrp-client-0.7.300.jar"), mentionedFiles(answer))
        c.draft = "Привет"; vm.submit()
        ShadowLooper.idleMainLooper(1, java.util.concurrent.TimeUnit.SECONDS)
        assertTrue(c.transcript.messages.last().text.contains("демо"))
        vm.download("/opt/sbrp-client/build/libs/sbrp-client-0.7.300.jar", c.cwd)
        ShadowLooper.idleMainLooper()
        assertTrue(vm.transfers.first().error.contains("демо"))
        vm.leaveDemo()
        assertEquals(Screen.Servers, vm.screen)
    }

    @Test fun pathsInTextBecomeDownloads() {
        assertEquals(listOf("/opt/x/build/libs/a.jar", "/root/logs/latest.log"),
            mentionedFiles("Готово: `/opt/x/build/libs/a.jar`, лог в /root/logs/latest.log. Исходник /opt/x/src/Main.kt и https://example.com/a.zip не нужны."))
    }
}
