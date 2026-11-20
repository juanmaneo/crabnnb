package com.example.crabnnbbridge

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.EOFException
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import kotlin.concurrent.thread

// TCP framing is u32 little-endian length followed by payload, matching src/bridge.rs.
// The relay adapter remains fail-closed until the Connect IQ Mobile SDK is wired.
class MainActivity : AppCompatActivity() {

    companion object {
        const val IQ_APP_ID = "crabnnb-phone-bridge"
        const val PORT = 8765
        const val CHALLENGE_SIZE = 16
        const val HEARTBEAT_SIZE = 81
    }

    @Volatile
    private var server: ServerSocket? = null

    private val connectIqRelay: ConnectIqRelay = ConnectIqMobileSdkStub()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        thread(name = "crabnnb-bridge-listener") { runBridge() }
    }

    override fun onDestroy() {
        server?.close()
        super.onDestroy()
    }

    private fun runBridge() {
        try {
            ServerSocket().use { listener ->
                listener.reuseAddress = true
                listener.bind(InetSocketAddress(InetAddress.getByName("127.0.0.1"), PORT))
                server = listener
                while (!listener.isClosed) {
                    val client = try {
                        listener.accept()
                    } catch (e: IOException) {
                        if (listener.isClosed) break else throw e
                    }
                    handle(client)
                }
            }
        } catch (e: IOException) {
            if (!isFinishing) {
                android.util.Log.e("CrabNNBBridge", "Local bridge listener failed", e)
            }
        } finally {
            server = null
        }
    }

    private fun readFrame(input: DataInputStream): ByteArray {
        val len = input.readUnsignedByte() or
            (input.readUnsignedByte() shl 8) or
            (input.readUnsignedByte() shl 16) or
            (input.readUnsignedByte() shl 24)
        if (len != CHALLENGE_SIZE) {
            throw IOException("expected a $CHALLENGE_SIZE-byte challenge frame, got $len bytes")
        }
        val payload = ByteArray(len)
        input.readFully(payload)
        return payload
    }

    private fun writeFrame(output: DataOutputStream, payload: ByteArray) {
        val len = payload.size
        output.writeByte(len and 0xff)
        output.writeByte((len ushr 8) and 0xff)
        output.writeByte((len ushr 16) and 0xff)
        output.writeByte((len ushr 24) and 0xff)
        output.write(payload)
        output.flush()
    }

    private fun sendChallengeAndAwaitHeartbeat(challenge: ByteArray): ByteArray =
        connectIqRelay.sendChallengeAndAwaitHeartbeat(IQ_APP_ID, challenge)

    private fun handle(client: Socket) {
        client.use { socket ->
            try {
                val input = DataInputStream(socket.getInputStream())
                val output = DataOutputStream(socket.getOutputStream())
                while (!socket.isClosed) {
                    val challenge = try {
                        readFrame(input)
                    } catch (_: EOFException) {
                        return
                    }
                    val heartbeat = sendChallengeAndAwaitHeartbeat(challenge)
                    if (heartbeat.size != HEARTBEAT_SIZE) {
                        throw IOException("watch returned an invalid heartbeat frame")
                    }
                    writeFrame(output, heartbeat)
                }
            } catch (e: IOException) {
                android.util.Log.w("CrabNNBBridge", "Closing bridge client", e)
            }
        }
    }
}
