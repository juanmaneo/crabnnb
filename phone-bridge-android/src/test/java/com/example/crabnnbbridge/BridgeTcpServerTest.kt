package com.example.crabnnbbridge

import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException
import java.net.Socket
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BridgeTcpServerTest {
    @Test
    fun relaysAChallengeAndWritesTheHeartbeatFrame() {
        val challenge = ByteArray(BridgeProtocol.CHALLENGE_SIZE) { (it + 4).toByte() }
        val heartbeat = ByteArray(BridgeProtocol.HEARTBEAT_SIZE) { (it * 2).toByte() }
        val received = AtomicReference<ByteArray?>()
        val challengeReceived = CountDownLatch(1)
        val server = startServer(object : ConnectIqRelay {
            override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray {
                assertEquals("crabnnb-phone-bridge", iqAppId)
                received.set(challenge)
                challengeReceived.countDown()
                return heartbeat
            }
        })

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                val output = DataOutputStream(socket.getOutputStream())
                writeFrame(output, challenge)

                assertArrayEquals(heartbeat, readFrame(DataInputStream(socket.getInputStream())))
            }
            assertTrue(challengeReceived.await(2, TimeUnit.SECONDS))
            assertArrayEquals(challenge, received.get())
            assertNull(server.listenerError.get())
        } finally {
            server.stop()
        }
    }

    @Test
    fun unavailableWatchClosesClientWithoutFabricatingAHeartbeat() {
        val relay = ConnectIqMobileSdkRelay(noDevicesSdk(), timeoutMs = 500)
        relay.start()
        val server = startServer(relay)

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                writeFrame(DataOutputStream(socket.getOutputStream()), ByteArray(BridgeProtocol.CHALLENGE_SIZE))
                assertEquals(-1, DataInputStream(socket.getInputStream()).read())
            }
        } finally {
            server.stop()
            relay.close()
        }
    }

    @Test
    fun closingServerDisconnectsClientBlockedOnAChallenge() {
        val clientAccepted = CountDownLatch(1)
        val server = startServer(
            relay = object : ConnectIqRelay {
                override fun sendChallengeAndAwaitHeartbeat(
                    iqAppId: String,
                    challenge: ByteArray,
                ): ByteArray = error("relay must not be called without a challenge")
            },
            onClientAccepted = { clientAccepted.countDown() },
        )

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                assertTrue("server did not accept client", clientAccepted.await(2, TimeUnit.SECONDS))
                server.stop()
                assertEquals(-1, DataInputStream(socket.getInputStream()).read())
            }
        } finally {
            server.stop()
        }
    }

    @Test
    fun invalidRelayHeartbeatIsNotWrittenToClient() {
        val server = startServer(object : ConnectIqRelay {
            override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray) = byteArrayOf(1)
        })

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                writeFrame(DataOutputStream(socket.getOutputStream()), ByteArray(BridgeProtocol.CHALLENGE_SIZE))
                assertTrue(DataInputStream(socket.getInputStream()).read() == -1)
            }
        } finally {
            server.stop()
        }
    }

    @Test
    fun rejectsMalformedChallengeFrameWithoutCallingRelay() {
        val relayCalls = AtomicInteger()
        val server = startServer(object : ConnectIqRelay {
            override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray {
                relayCalls.incrementAndGet()
                return ByteArray(BridgeProtocol.HEARTBEAT_SIZE)
            }
        })

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                DataOutputStream(socket.getOutputStream()).apply {
                    write(byteArrayOf(17, 0, 0, 0))
                    flush()
                }
                assertTrue(DataInputStream(socket.getInputStream()).read() == -1)
            }
            assertEquals(0, relayCalls.get())
        } finally {
            server.stop()
        }
    }

    private fun noDevicesSdk() = object : ConnectIqSdk {
        override fun initialize(onReady: (IOException?) -> Unit) = onReady(null)
        override fun connectedDevices() = emptyList<ConnectIqDevice>()
        override fun registerForAppEvents(
            device: ConnectIqDevice,
            iqAppId: String,
            onMessage: (String, Long, Boolean, List<Any?>) -> Unit,
        ) = error("must not register without a connected watch")
        override fun sendMessage(
            device: ConnectIqDevice,
            iqAppId: String,
            message: ByteArray,
            onStatus: (String, Long, Boolean) -> Unit,
        ) = error("must not send without a connected watch")
        override fun unregisterForAppEvents(device: ConnectIqDevice, iqAppId: String) = Unit
        override fun shutdown() = Unit
    }

    private fun startServer(
        relay: ConnectIqRelay,
        onClientAccepted: () -> Unit = {},
    ): RunningServer {
        val started = CountDownLatch(1)
        val port = AtomicInteger()
        val listenerError = AtomicReference<IOException?>()
        val service = BridgeTcpServer(
            relay = relay,
            port = 0,
            onListening = {
                port.set(it)
                started.countDown()
            },
            onClientAccepted = onClientAccepted,
            onListenerError = {
                listenerError.set(it)
                started.countDown()
            },
        )
        val thread = thread(name = "bridge-server-test") { service.run() }
        assertTrue("server did not bind in time", started.await(2, TimeUnit.SECONDS))
        assertTrue("server failed to bind: ${listenerError.get()}", port.get() > 0)
        return RunningServer(service, thread, port.get(), listenerError)
    }

    private fun writeFrame(output: DataOutputStream, payload: ByteArray) {
        output.writeInt(Integer.reverseBytes(payload.size))
        output.write(payload)
        output.flush()
    }

    private fun readFrame(input: DataInputStream): ByteArray {
        val length = Integer.reverseBytes(input.readInt())
        if (length != BridgeProtocol.HEARTBEAT_SIZE) {
            throw IOException("unexpected test heartbeat length $length")
        }
        return ByteArray(length).also { input.readFully(it) }
    }

    private class RunningServer(
        private val service: BridgeTcpServer,
        private val thread: Thread,
        val port: Int,
        val listenerError: AtomicReference<IOException?>,
    ) {
        fun stop() {
            service.close()
            thread.join(2_000)
            assertFalse("server thread did not stop", thread.isAlive)
        }
    }
}
