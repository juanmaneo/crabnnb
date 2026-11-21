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
            sdk.deliverReply(success = false, payload = emptyList())
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
    fun rejectsBlankAppIdWithoutUsingSdk() {
        val sdk = FakeConnectIqSdk()

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat("  ", challenge())
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun startIsIdempotentAndCloseBeforeStartPreventsInitialization() {
        val sdk = FakeConnectIqSdk()
        val relay = ConnectIqMobileSdkRelay(sdk)
        relay.start()
        relay.start()
        assertEquals(1, sdk.initializations.get())

        val closedSdk = FakeConnectIqSdk()
        val closedRelay = ConnectIqMobileSdkRelay(closedSdk)
        closedRelay.close()
        closedRelay.start()
        assertEquals(0, closedSdk.initializations.get())
    }

    @Test
    fun initializationRuntimeFailureIsReportedAsIoFailure() {
        val sdk = FakeConnectIqSdk(initializeFailure = IllegalStateException("init blew up"))

        assertThrows(IOException::class.java) {
            relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertTrue(sdk.sent.isEmpty())
    }

    @Test
    fun timesOutOrInterruptsWhileWaitingForSdkInitialization() {
        val timeoutSdk = FakeConnectIqSdk(initializeWithoutCallback = true)
        assertThrows(IOException::class.java) {
            relay(timeoutSdk, timeoutMs = 20).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }

        val interruptedSdk = FakeConnectIqSdk(initializeWithoutCallback = true)
        val relay = relay(interruptedSdk, timeoutMs = 10_000)
        val failure = AtomicReference<Throwable?>()
        val finished = CountDownLatch(1)
        val worker = thread {
            try {
                relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
            } catch (error: Throwable) {
                failure.set(error)
            } finally {
                finished.countDown()
            }
        }
        assertTrue(interruptedSdk.initializeEntered.await(1, TimeUnit.SECONDS))
        worker.interrupt()
        assertTrue(finished.await(1, TimeUnit.SECONDS))
        assertTrue(failure.get() is IOException)
        worker.join(1_000)
    }

    @Test
    fun ignoresMismatchedCallbacksAndDuplicateReplies() {
        val heartbeat = heartbeat()
        val sdk = FakeConnectIqSdk(onSend = { fake ->
            fake.deliverSendStatus(true, deviceId = WATCH_ID + 1)
            fake.deliverSendStatus(true, appId = "another-app")
            fake.deliverReply(payload = listOf(heartbeat), deviceId = WATCH_ID + 1)
            fake.deliverReply(payload = listOf(heartbeat), appId = "another-app")
            fake.deliverSendStatus(true)
            fake.deliverReply(payload = listOf(heartbeat.copyOf()))
            fake.deliverReply(success = false, payload = emptyList())
            fake.deliverSendStatus(false)
        })

        assertArrayEquals(heartbeat, relay(sdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
    }

    @Test
    fun failedSdkDeviceLookupIsWrappedAndCleanupRuntimeFailureIsIgnored() {
        val lookupSdk = FakeConnectIqSdk(deviceLookupFailure = IllegalStateException("lookup failed"))
        assertThrows(IOException::class.java) {
            relay(lookupSdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }

        val heartbeat = heartbeat()
        val cleanupSdk = FakeConnectIqSdk(
            onSend = { fake ->
                fake.deliverSendStatus(true)
                fake.deliverReply(payload = listOf(heartbeat.copyOf()))
            },
            unregisterRuntimeFailure = IllegalStateException("cleanup failed"),
        )
        assertArrayEquals(heartbeat, relay(cleanupSdk).sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
    }

    @Test
    fun interruptingAnOutstandingReplyWaitFailsAndRestoresInterruptFlag() {
        val sdk = FakeConnectIqSdk(onSend = { it.deliverSendStatus(true) })
        val relay = relay(sdk, timeoutMs = 10_000)
        val failure = AtomicReference<Throwable?>()
        val interruptRestored = AtomicReference(false)
        val finished = CountDownLatch(1)
        val worker = thread {
            try {
                relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
            } catch (error: Throwable) {
                failure.set(error)
                interruptRestored.set(Thread.currentThread().isInterrupted)
            } finally {
                finished.countDown()
            }
        }

        assertTrue(sdk.sendEntered.await(1, TimeUnit.SECONDS))
        worker.interrupt()
        assertTrue(finished.await(1, TimeUnit.SECONDS))
        assertTrue(failure.get() is IOException)
        assertTrue(interruptRestored.get())
        worker.join(1_000)
    }

    @Test
    fun closingAfterCompletedExchangeDoesNotChangeResult() {
        val heartbeat = heartbeat()
        val sdk = FakeConnectIqSdk(onSend = { fake ->
            fake.deliverSendStatus(true)
            fake.deliverReply(payload = listOf(heartbeat.copyOf()))
        })
        val relay = relay(sdk)

        assertArrayEquals(heartbeat, relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
        relay.close()
        relay.close()
        assertEquals(1, sdk.shutdowns.get())
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

    @Test
    fun closeDuringDeviceLookupCancelsBeforeRegisteringOrSending() {
        val relayRef = AtomicReference<ConnectIqMobileSdkRelay?>()
        val sdk = FakeConnectIqSdk(onDeviceLookup = { relayRef.get()?.close() })
        val relay = relay(sdk)
        relayRef.set(relay)

        assertThrows(IOException::class.java) {
            relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge())
        }
        assertTrue(sdk.sent.isEmpty())
        assertEquals(0, sdk.unregistered.get())
        assertEquals(1, sdk.shutdowns.get())
    }

    @Test
    fun closingAfterOutcomeCompletesDoesNotTurnSuccessIntoFailure() {
        val heartbeat = heartbeat()
        val relayRef = AtomicReference<ConnectIqMobileSdkRelay?>()
        val sdk = FakeConnectIqSdk(onSend = { fake ->
            fake.deliverSendStatus(true)
            fake.deliverReply(payload = listOf(heartbeat.copyOf()))
            relayRef.get()?.close()
        })
        val relay = relay(sdk)
        relayRef.set(relay)

        assertArrayEquals(heartbeat, relay.sendChallengeAndAwaitHeartbeat(APP_ID, challenge()))
        assertEquals(1, sdk.shutdowns.get())
    }

    @Test
    fun exposesTheOpaqueDeviceIdentifierAndHandle() {
        assertEquals(WATCH_ID, WATCH.id)
        assertEquals("watch", WATCH.handle)
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
        private val initializeFailure: RuntimeException? = null,
        private val initializeWithoutCallback: Boolean = false,
        private val deviceLookupFailure: RuntimeException? = null,
        private val unregisterRuntimeFailure: RuntimeException? = null,
        private val onDeviceLookup: () -> Unit = {},
    ) : ConnectIqSdk {
        val sent = mutableListOf<ByteArray>()
        val unregistered = AtomicInteger()
        val shutdowns = AtomicInteger()
        val sendEntered = CountDownLatch(1)
        val initializeEntered = CountDownLatch(1)
        val initializations = AtomicInteger()
        private var messageListener: ((String, Long, Boolean, List<Any?>) -> Unit)? = null
        private var statusListener: ((String, Long, Boolean) -> Unit)? = null

        override fun initialize(onReady: (IOException?) -> Unit) {
            initializations.incrementAndGet()
            initializeEntered.countDown()
            initializeFailure?.let { throw it }
            if (!initializeWithoutCallback) onReady(initError)
        }

        override fun connectedDevices(): List<ConnectIqDevice> {
            deviceLookupFailure?.let { throw it }
            onDeviceLookup()
            return devices
        }

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
            unregisterRuntimeFailure?.let { throw it }
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
