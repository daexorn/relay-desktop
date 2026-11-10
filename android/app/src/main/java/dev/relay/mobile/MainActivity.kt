package dev.relay.mobile

import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.DisposableEffect
import androidx.lifecycle.viewmodel.compose.viewModel

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        Crashes.install(this)
        super.onCreate(savedInstanceState)
        // Formula fonts load in the background, so the first message with LaTeX is quick.
        Thread { Formulas.init(applicationContext) }.start()
        setContent {
            val vm: RelayViewModel = viewModel()
            // Status bar icons follow the theme: dark icons on light themes.
            val light = isLight(vm.theme)
            DisposableEffect(light) {
                val style = if (light) SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT) else SystemBarStyle.dark(Color.TRANSPARENT)
                enableEdgeToEdge(style, style)
                onDispose {}
            }
            RelayApp(vm)
        }
    }
}
