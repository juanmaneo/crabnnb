package com.example.crabnnbbridge

import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectIqRelayTest {
    @Test
    fun returnsHeartbeatWhenSendSucceedsAndWatchReplies() {
        val heartbeat = heartbeat()
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(payload = listOf(heartbeat.copyOf()))
        })

        val response = relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())

        assertArrayEquals(heartbeat, response)
        assertArrayEquals(challenge(), sdk.sent.single())
        assertEquals(1, sdk.unregistered.get())
    }

    @Test
    fun acceptsReplyDeliveredBeforeSendStatus() {
        val heartbeat = heartbeat()
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverReply(payload = listOf(heartbeat.copyOf()))
            sdk.deliverSendStatus(true)
        })

        assertArrayEquals(heartbeat, relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
    }

    @Test
    fun acceptsHeartbeatDeliveredAsListOfBytes() {
        val heartbeat = heartbeat()
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(payload = listOf(heartbeat.toList()))
        })

        assertArrayEquals(heartbeat, relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
    }

    @Test
    fun failsWhenNoWatchIsConnected() {
        val sdk = FakeConnectIqSdk(devices = emptyList())

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun failsWhenMoreThanOneWatchIsConnected() {
        val sdk = FakeConnectIqSdk(devices = listOf(WATCH, ConnectIqDevice(WATCH_ID + 1, "other")))

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun failsWhenSdkInitializationFails() {
        val sdk = FakeConnectIqSdk(initError = IOException("sdk missing"))

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun failsWhenRelayWasNeverStarted() {
        val sdk = FakeConnectIqSdk()

        assertThrows(IOException::class.java) {
            ConnectIqMobileSdkRelay(sdk, timeoutMs = 1_000).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun failsWhenChallengeHasWrongLength() {
        val sdk = FakeConnectIqSdk()

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, ByteArray(BridgeProtocol.CHALLENGE_SIZE + 1))
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun failsWhenSendStatusReportsFailure() {
        val sdk = FakeConnectIqSdk(onSend = { it.deliverSendStatus(false) })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertEquals(1, sdk.unregistered.get())
    }

    @Test
    fun failsWhenWatchReportsMessageFailure() {
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(success = false, payload = listOf(heartbeat()))
        })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
    }

    @Test
    fun rejectsHeartbeatWithWrongLength() {
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(payload = listOf(ByteArray(BridgeProtocol.HEARTBEAT_SIZE - 1)))
        })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
    }

    @Test
    fun rejectsPayloadWithMultipleElements() {
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(payload = listOf(heartbeat(), heartbeat()))
        })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
    }

    @Test
    fun rejectsListPayloadWithNonByteElement() {
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(payload = listOf(heartbeat().toList().toMutableList<Any?>().apply { this[0] = 1 }))
        })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
    }

    @Test
    fun rejectsNonByteArrayPayload() {
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(payload = listOf("not bytes"))
        })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
    }

    @Test
    fun timesOutWhenWatchNeverReplies() {
        val sdk = FakeConnectIqSdk(onSend = { it.deliverSendStatus(true) })

        assertThrows(IOException::class.java) {
            relay(sdk, timeoutMs = 50).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertEquals(1, sdk.unregistered.get())
    }

    @Test
    fun ignoresRepliesFromOtherDevicesOrApps() {
        val sdk = FakeConnectIqSdk(onSend = { sdk ->
            sdk.deliverSendStatus(true)
            sdk.deliverReply(deviceId = WATCH_ID + 1, payload = listOf(heartbeat()))
            sdk.deliverReply(appId = "someone-else", payload = listOf(heartbeat()))
        })

        assertThrows(IOException::class.java) {
            relay(sdk, timeoutMs = 50).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
    }

    @Test
    fun sdkRuntimeExceptionBecomesIoException() {
        val sdk = FakeConnectIqSdk(onSend = { throw IllegalStateException("boom") })

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertEquals(1, sdk.unregistered.get())
    }

    @Test
    fun cleanupFailureDoesNotReplaceHeartbeat() {
        val heartbeat = heartbeat()
        val sdk = FakeConnectIqSdk(
            onSend = { sdk ->
                sdk.deliverSendStatus(true)
                sdk.deliverReply(payload = listOf(heartbeat.copyOf()))
            },
            unregisterFailure = IOException("unregister failed"),
        )

        assertArrayEquals(heartbeat, relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
    }

    @Test
    fun closingRelayCancelsAnInFlightWait() {
        val sdk = FakeConnectIqSdk(onSend = { it.deliverSendStatus(true) })
        val relay = relay(sdk, timeoutMs = 10_000)
        val result = AtomicReference<Throwable?>()
        val finished = CountDownLatch(1)
        val worker = thread {
            try {
                relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
            } catch (error: Throwable) {
                result.set(error)
            } finally {
                finished.countDown()
            }
        }

        assertTrue(sdk.sendEntered.await(1, TimeUnit.SECONDS))
        relay.close()

        assertTrue("relay did not cancel its outstanding wait", finished.await(1, TimeUnit.SECONDS))
        assertTrue(result.get() is IOException)
        worker.join(1_000)
    }

    @Test
    fun closedRelayRejectsChallenges() {
        val sdk = FakeConnectIqSdk()
        val relay = relay(sdk)
        relay.close()

        assertThrows(IOException::class.java) {
            relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertEquals(1, sdk.shutdowns.get())
        assertTrue(sdk.sent.isEmpty())
    }

    private fun relay(sdk: FakeConnectIqSdk, timeoutMs: Long = 1_000): ConnectIqMobileSdkRelay =
        ConnectIqMobileSdkRelay(sdk, timeoutMs).also { it.start() }

    private fun challenge() = ByteArray(BridgeProtocol.CHALLENGE_SIZE) { (it + 1).toByte() }

    private fun heartbeat() = ByteArray(BridgeProtocol.HEARTBEAT_SIZE) { (it * 3).toByte() }

    private companion object {
        const val APP_ID = "crabnnb-phone-bridge"
        const val WATCH_ID = 42L
        val WATCH = ConnectIqDevice(WATCH_ID, "watch")
    }

    private class FakeConnectIqSdk(
        private val initError: IOException? = null,
        private val devices: List<ConnectIqDevice> = listOf(WATCH),
        private val onSend: (FakeConnectIqSdk) -> Unit = {},
        private val unregisterFailure: IOException? = null,
    ) : ConnectIqSdk {
        val sent = mutableListOf<ByteArray>()
        val unregistered = AtomicInteger()
        val shutdowns = AtomicInteger()
        val sendEntered = CountDownLatch(1)
        private var messageListener: ((String, Long, Boolean, List<Any?>) -> Unit)? = null
        private var statusListener: ((String, Long, Boolean) -> Unit)? = null

        override fun initialize(onReady: (IOException?) -> Unit) = onReady(initError)

        override fun connectedDevices(): List<ConnectIqDevice> = devices

        override fun registerForAppEvents(
            device: ConnectIqDevice,
            iqAppId: String,
            onMessage: (String, Long, Boolean, List<Any?>) -> Unit,
        ) {
            messageListener = onMessage
        }

        override fun sendMessage(
            device: ConnectIqDevice,
            iqAppId: String,
            message: ByteArray,
            onStatus: (String, Long, Boolean) -> Unit,
        ) {
            sent += message
            statusListener = onStatus
            sendEntered.countDown()
            onSend(this)
        }

        override fun unregisterForAppEvents(device: ConnectIqDevice, iqAppId: String) {
            unregistered.incrementAndGet()
            messageListener = null
            unregisterFailure?.let { throw it }
        }

        override fun shutdown() {
            shutdowns.incrementAndGet()
        }

        fun deliverReply(
            success: Boolean = true,
            payload: List<Any?>,
            deviceId: Long = WATCH_ID,
            appId: String = APP_ID,
        ) {
            messageListener?.invoke(appId, deviceId, success, payload) ?: error("no reply listener registered")
        }

        fun deliverSendStatus(success: Boolean, deviceId: Long = WATCH_ID, appId: String = APP_ID) {
            statusListener?.invoke(appId, deviceId, success) ?: error("no send listener registered")
        }
    }
}
