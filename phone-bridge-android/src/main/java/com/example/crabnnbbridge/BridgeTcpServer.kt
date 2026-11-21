package com.example.crabnnbbridge

import java.io.Closeable
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.EOFException
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

/** Loopback-only TCP server for the PC-to-phone half of the bridge. */
internal class BridgeTcpServer(
    private val relay: ConnectIqRelay,
    private val port: Int = DEFAULT_PORT,
    private val onListening: (Int) -> Unit = {},
    private val onClientAccepted: () -> Unit = {},
    private val onListenerError: (IOException) -> Unit = {},
    private val onClientError: (IOException) -> Unit = {},
    private val serverSocketFactory: () -> ServerSocket = { ServerSocket() },
) : AutoCloseable {
    companion object {
        const val DEFAULT_PORT = 8765
        const val IQ_APP_ID = "crabnnb-phone-bridge"
    }

    private val closeRequested = AtomicBoolean(false)

    @Volatile
    private var listener: ServerSocket? = null

    @Volatile
    private var activeClient: Socket? = null

    fun run() {
        try {
            serverSocketFactory().use { server ->
                server.reuseAddress = true
                server.bind(InetSocketAddress(InetAddress.getByName("127.0.0.1"), port))
                listener = server
                onListening(server.localPort)
                if (closeRequested.get()) {
                    server.close()
                }
                while (!server.isClosed) {
                    val client = try {
                        server.accept()
                    } catch (e: IOException) {
                        if (server.isClosed) break else throw e
                    }
                    activeClient = client
                    onClientAccepted()
                    if (closeRequested.get()) {
                        closeQuietly(client)
                        break
                    }
                    try {
                        handle(client)
                    } finally {
                        activeClient = null
                    }
                }
            }
        } catch (e: IOException) {
            if (!closeRequested.get()) {
                onListenerError(e)
            }
        } finally {
            listener = null
            closeQuietly(activeClient)
            activeClient = null
        }
    }

    override fun close() {
        closeRequested.set(true)
        closeQuietly(listener)
        closeQuietly(activeClient)
    }

    internal fun closeQuietly(closeable: Closeable?) {
        try {
            closeable?.close()
        } catch (_: IOException) {
            // Closing is best-effort during shutdown.
        }
    }

    private fun handle(client: Socket) {
        client.use { socket ->
            try {
                val input = DataInputStream(socket.getInputStream())
                val output = DataOutputStream(socket.getOutputStream())
                while (true) {
                    val challenge = try {
                        BridgeProtocol.readChallenge(input)
                    } catch (_: EOFException) {
                        return
                    }
                    val heartbeat = relay.sendChallengeAndAwaitHeartbeat(IQ_APP_ID, challenge)
                    BridgeProtocol.writeHeartbeat(output, heartbeat)
                }
            } catch (e: IOException) {
                if (!closeRequested.get()) {
                    onClientError(e)
                }
            }
        }
    }
}
