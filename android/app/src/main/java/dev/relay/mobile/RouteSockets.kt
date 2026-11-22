package dev.relay.mobile

import android.content.Context
import android.net.ConnectivityManager
import com.jcraft.jsch.SocketFactory
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket

/** Resolve and connect on the same default network, including a system VPN. */
internal class RouteSockets(context: Context) : SocketFactory {
    private val manager = context.getSystemService(ConnectivityManager::class.java)
    override fun createSocket(host: String, port: Int): Socket {
        val network = manager.activeNetwork ?: throw IOException("На телефоне нет активной сети")
        val addresses = network.getAllByName(host)
        var failure: IOException? = null
        val until = android.os.SystemClock.elapsedRealtime() + 15000L
        for ((i, addr) in addresses.withIndex()) {
            val left = until - android.os.SystemClock.elapsedRealtime()
            if (left <= 0) break
            val socket = network.socketFactory.createSocket()
            try {
                socket.connect(InetSocketAddress(addr, port), (left / (addresses.size-i)).coerceAtLeast(1L).toInt())
                return socket
            } catch (e: IOException) {
                failure = e
                socket.close()
            }
        }
        throw IOException("Сеть не подключилась к $host:$port. Проверь доступ к SSH через текущую сеть или VPN.", failure)
    }

    override fun getInputStream(socket: Socket) = socket.getInputStream()
    override fun getOutputStream(socket: Socket) = socket.getOutputStream()
}
