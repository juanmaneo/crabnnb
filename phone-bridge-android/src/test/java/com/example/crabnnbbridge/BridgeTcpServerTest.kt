package com.example.crabnnbbridge

import java.io.ByteArrayOutputStream
import java.io.Closeable
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.ServerSocket
import java.net.SocketAddress
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
                assertEquals("d72f71b21bf64c49a9ac3c6a670ea1f4", iqAppId)
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
    fun reportsClientIoFailureUnlessShutdownWasRequested() {
        val clientError = AtomicReference<IOException?>()
        val errorReported = CountDownLatch(1)
        val server = startServer(
            relay = object : ConnectIqRelay {
                override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray =
                    throw IOException("watch unavailable")
            },
            onClientError = {
                clientError.set(it)
                errorReported.countDown()
            },
        )

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                writeFrame(DataOutputStream(socket.getOutputStream()), ByteArray(BridgeProtocol.CHALLENGE_SIZE))
                assertEquals(-1, DataInputStream(socket.getInputStream()).read())
            }
            assertTrue(errorReported.await(2, TimeUnit.SECONDS))
            assertEquals("watch unavailable", clientError.get()?.message)
        } finally {
            server.stop()
        }
    }

    @Test
    fun closesAcceptedClientIfShutdownWinsBeforeHandling() {
        val service = AtomicReference<BridgeTcpServer?>()
        val server = startServer(
            relay = object : ConnectIqRelay {
                override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray =
                    error("relay must not be called after shutdown")
            },
            onClientAccepted = { service.get()?.close() },
        )
        service.set(server.service)

        try {
            Socket("127.0.0.1", server.port).use { socket ->
                socket.soTimeout = 2_000
                assertEquals(-1, DataInputStream(socket.getInputStream()).read())
            }
        } finally {
            server.stop()
        }
    }

    @Test
    fun exercisesDefaultListenerClientAndErrorCallbacks() {
        ServerSocket(0).use { occupied ->
            BridgeTcpServer(errorRelay(), port = occupied.localPort).run()
        }

        val port = AtomicInteger()
        val listening = CountDownLatch(1)
        val server = BridgeTcpServer(
            relay = object : ConnectIqRelay {
                override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray =
                    throw IOException("default client error callback")
            },
            port = 0,
            onListening = {
                port.set(it)
                listening.countDown()
            },
        )
        val worker = thread { server.run() }
        try {
            assertTrue(listening.await(2, TimeUnit.SECONDS))
            Socket("127.0.0.1", port.get()).use { socket ->
                socket.soTimeout = 2_000
                writeFrame(DataOutputStream(socket.getOutputStream()), ByteArray(BridgeProtocol.CHALLENGE_SIZE))
                assertEquals(-1, DataInputStream(socket.getInputStream()).read())
            }
        } finally {
            server.close()
            worker.join(2_000)
        }
        assertFalse("default-callback server did not stop", worker.isAlive)
    }

    @Test
    fun reportsBindFailureUnlessShutdownWasRequested() {
        ServerSocket(0).use { occupied ->
            val error = AtomicReference<IOException?>()
            BridgeTcpServer(
                relay = errorRelay(),
                port = occupied.localPort,
                onListenerError = { error.set(it) },
            ).run()
            assertTrue(error.get() is IOException)

            val suppressed = AtomicInteger()
            val stopped = BridgeTcpServer(
                relay = errorRelay(),
                port = occupied.localPort,
                onListenerError = { suppressed.incrementAndGet() },
            )
            stopped.close()
            stopped.run()
            assertEquals(0, suppressed.get())
        }
    }

    @Test
    fun reportsUnexpectedAcceptFailureAndStopsAfterCloseInListeningCallback() {
        val acceptError = AtomicReference<IOException?>()
        val brokenSocket = object : ServerSocket() {
            override fun bind(endpoint: SocketAddress?, backlog: Int) = Unit
            override fun getLocalPort(): Int = 12345
            override fun accept(): Socket = throw IOException("accept failed")
        }
        BridgeTcpServer(
            relay = errorRelay(),
            onListenerError = { acceptError.set(it) },
            serverSocketFactory = { brokenSocket },
        ).run()
        assertEquals("accept failed", acceptError.get()?.message)

        lateinit var service: BridgeTcpServer
        val listening = CountDownLatch(1)
        service = BridgeTcpServer(
            relay = errorRelay(),
            port = 0,
            onListening = {
                service.close()
                listening.countDown()
            },
        )
        val worker = thread { service.run() }
        assertTrue(listening.await(2, TimeUnit.SECONDS))
        worker.join(2_000)
        assertFalse("listener did not stop", worker.isAlive)
    }

    @Test
    fun preservesClientFailureWhenSocketCloseAlsoFails() {
        val accepted = CountDownLatch(1)
        val client = object : Socket() {
            override fun getInputStream(): InputStream = object : InputStream() {
                override fun read(): Int = throw IllegalStateException("client read failed")
            }

            override fun getOutputStream(): OutputStream = ByteArrayOutputStream()

            override fun close() = throw IOException("client close failed")
        }
        val fakeServer = object : ServerSocket() {
            override fun bind(endpoint: SocketAddress?, backlog: Int) = Unit
            override fun getLocalPort(): Int = 12346
            override fun accept(): Socket = client
        }
        val service = BridgeTcpServer(
            relay = errorRelay(),
            onClientAccepted = { accepted.countDown() },
            serverSocketFactory = { fakeServer },
        )
        val failure = AtomicReference<Throwable?>()
        val worker = thread {
            try {
                service.run()
            } catch (error: Throwable) {
                failure.set(error)
            }
        }

        assertTrue(accepted.await(2, TimeUnit.SECONDS))
        worker.join(2_000)
        assertFalse("server thread did not stop", worker.isAlive)
        assertTrue(failure.get() is IllegalStateException)
        assertEquals("client close failed", failure.get()?.suppressed?.singleOrNull()?.message)
    }

    @Test
    fun closeQuietlySuppressesIoFailure() {
        val service = BridgeTcpServer(errorRelay())
        service.closeQuietly(object : Closeable {
            override fun close() = throw IOException("close failed")
        })
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

    private fun errorRelay() = object : ConnectIqRelay {
        override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray =
            error("relay must not be called")
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
        onClientError: (IOException) -> Unit = {},
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
            onClientError = onClientError,
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
        val service: BridgeTcpServer,
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
