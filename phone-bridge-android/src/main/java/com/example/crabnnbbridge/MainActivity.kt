package com.example.crabnnbbridge

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import kotlin.concurrent.thread

class MainActivity : AppCompatActivity() {
    companion object {
        const val PORT = BridgeTcpServer.DEFAULT_PORT
    }

    private var bridgeServer: BridgeTcpServer? = null
    private var connectIqRelay: ConnectIqMobileSdkRelay? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val relay = ConnectIqMobileSdkRelay(applicationContext)
        connectIqRelay = relay
        relay.start()
        val server = BridgeTcpServer(
            relay = relay,
            port = PORT,
            onListenerError = { error ->
                if (!isFinishing) {
                    android.util.Log.e("CrabNNBBridge", "Local bridge listener failed", error)
                }
            },
            onClientError = { error ->
                android.util.Log.w("CrabNNBBridge", "Closing bridge client", error)
            },
        )
        bridgeServer = server
        thread(name = "crabnnb-bridge-listener") { server.run() }
    }

    override fun onDestroy() {
        bridgeServer?.close()
        bridgeServer = null
        connectIqRelay?.close()
        connectIqRelay = null
        super.onDestroy()
    }
}
