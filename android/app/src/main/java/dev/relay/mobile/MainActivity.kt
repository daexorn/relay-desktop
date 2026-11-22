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
            androidx.compose.runtime.LaunchedEffect(vm) { vm.restoreLastServer() }
            DisposableEffect(vm) {
                val manager = getSystemService(android.net.ConnectivityManager::class.java)
                val callback = object : android.net.ConnectivityManager.NetworkCallback() {
                    private var previous = manager.activeNetwork
                    override fun onAvailable(network: android.net.Network) {
                        if (network != previous) runOnUiThread { vm.routeChanged() }
                        previous = network
                    }
                    override fun onLost(network: android.net.Network) {
                        if (network == previous) previous = null
                    }
                }
                manager.registerDefaultNetworkCallback(callback)
                onDispose { manager.unregisterNetworkCallback(callback) }
            }
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
