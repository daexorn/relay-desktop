package dev.relay.mobile

import org.junit.Assert.assertEquals
import org.junit.Test

class UserInfoTest {
    /** Regression: getPassword() used to call itself and overflow the stack on the first login. */
    @Test
    fun passwordComesBack() {
        assertEquals("секрет", Bridge.userInfo("секрет").password)
    }
}
