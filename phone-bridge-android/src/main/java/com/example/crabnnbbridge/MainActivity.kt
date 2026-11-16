package com.example.crabnnbbridge

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.ServerSocket
import java.net.Socket
import kotlin.concurrent.thread

// Skeleton relay: TCP (from crabnnb --bridge-addr) <-> Connect IQ Mobile SDK (watch).
// TODO: wire to the Connect IQ Mobile SDK:
//   val connectIq = ConnectIQ.getInstance(this)
//   connectIq.registerForApplicationMessages(IQ_APP_ID, listener)   // heartbeats from the watch
//   connectIq.sendMessage(IQ_APP_ID, challengeBytes)                // challenge to the watch
class MainActivity : AppCompatActivity() {

    companion object {
        const val IQ_APP_ID = "crabnnb-phone-bridge"
        const val PORT = 8765
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        thread { runBridge() }
    }

    private fun runBridge() {
        val server = ServerSocket(PORT)
        while (true) {
            val client: Socket = server.accept()
            thread { handle(client) }
        }
    }

    private fun handle(client: Socket) {
        val input = DataInputStream(client.getInputStream())
        val output = DataOutputStream(client.getOutputStream())
        while (true) {
            val len = input.readInt() // u32le frame length (crabnnb bridge framing)
            if (len < 0 || len > 4096) return
            val challenge = ByteArray(len)
            input.readFully(challenge)
            // TODO: forward challenge to the watch via Connect IQ SDK and await the heartbeat
            // val heartbeat = sendToWatchAndAwait(challenge)
            val heartbeat = ByteArray(0) // placeholder
            output.writeInt(heartbeat.size)
            output.write(heartbeat)
            output.flush()
        }
    }
}
