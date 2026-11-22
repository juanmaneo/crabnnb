package com.example.crabnnbbridge

import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Adapter boundary for the Garmin Connect IQ Mobile SDK integration. */
internal interface ConnectIqRelay {
    @Throws(IOException::class)
    fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray
}

/** Opaque device handle; [id] is the Connect IQ device identifier. */
internal class ConnectIqDevice(val id: Long, val handle: Any)

/** Narrow SDK surface used by [ConnectIqMobileSdkRelay]; tests provide a fake implementation. */
internal interface ConnectIqSdk {
    fun initialize(onReady: (IOException?) -> Unit)

    @Throws(IOException::class)
    fun connectedDevices(): List<ConnectIqDevice>

    @Throws(IOException::class)
    fun registerForAppEvents(
        device: ConnectIqDevice,
        iqAppId: String,
        onMessage: (appId: String, deviceId: Long, success: Boolean, payload: List<Any?>) -> Unit,
    )

    @Throws(IOException::class)
    fun sendMessage(
        device: ConnectIqDevice,
        iqAppId: String,
        message: ByteArray,
        onStatus: (appId: String, deviceId: Long, success: Boolean) -> Unit,
    )

    @Throws(IOException::class)
    fun unregisterForAppEvents(device: ConnectIqDevice, iqAppId: String)

    fun shutdown()
}

/**
 * Relays one 16-byte challenge to the single connected watch and waits for its
 * 81-byte heartbeat. Every failure path throws; no heartbeat is ever fabricated.
 * [start] must run on the main thread because the Connect IQ SDK creates a Handler.
 */
internal class ConnectIqMobileSdkRelay(
    private val sdk: ConnectIqSdk,
    private val timeoutMs: Long = DEFAULT_TIMEOUT_MS,
) : ConnectIqRelay, AutoCloseable {
    companion object {
        const val DEFAULT_TIMEOUT_MS = 20_000L
    }

    private val started = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    private val initialized = CountDownLatch(1)

    @Volatile
    private var initializationError: IOException? = null

    @Volatile
    private var activeOutcome: RelayOutcome? = null


    fun start() {
        if (closed.get() || !started.compareAndSet(false, true)) return
        try {
            sdk.initialize { error ->
                initializationError = error
                initialized.countDown()
            }
        } catch (error: RuntimeException) {
            initializationError = IOException("Garmin Connect IQ SDK could not start", error)
            initialized.countDown()
        }
    }

    override fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray {
        if (iqAppId.isBlank()) throw IOException("Connect IQ app id must not be blank")
        if (challenge.size != BridgeProtocol.CHALLENGE_SIZE) {
            throw IOException("expected a ${BridgeProtocol.CHALLENGE_SIZE}-byte challenge")
        }
        if (!started.get()) throw IOException("Garmin Connect IQ SDK has not been started")
        awaitInitialized()

        val outcome = RelayOutcome()
        activeOutcome = outcome
        var device: ConnectIqDevice? = null
        try {
            val target = singleConnectedDevice()
            if (closed.get()) throw IOException("Garmin Connect IQ relay is closed")
            device = target
            sdk.registerForAppEvents(target, iqAppId) { appId, deviceId, success, payload ->
                if (appId.equals(iqAppId, ignoreCase = true) && deviceId == target.id) {
                    outcome.onReply(success, payload)
                }
            }
            sdk.sendMessage(target, iqAppId, challenge.copyOf()) { appId, deviceId, success ->
                if (appId.equals(iqAppId, ignoreCase = true) && deviceId == target.id) {
                    outcome.onSendStatus(success)
                }
            }
            return outcome.await(timeoutMs)
        } catch (error: IOException) {
            throw error
        } catch (error: RuntimeException) {
            throw IOException("Connect IQ challenge relay failed", error)
        } finally {
            activeOutcome = null
            val registeredDevice = device
            if (registeredDevice != null) unregisterQuietly(registeredDevice, iqAppId)
        }
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        initialized.countDown()
        activeOutcome?.cancel()
        sdk.shutdown()
    }

    private fun awaitInitialized() {
        try {
            if (!initialized.await(timeoutMs, TimeUnit.MILLISECONDS)) {
                throw IOException("timed out initializing Garmin Connect IQ SDK")
            }
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw IOException("interrupted while initializing Garmin Connect IQ SDK", error)
        }
        initializationError?.let { throw it }
        if (closed.get()) throw IOException("Garmin Connect IQ relay is closed")
    }

    private fun singleConnectedDevice(): ConnectIqDevice {
        val devices = sdk.connectedDevices()
        return when (devices.size) {
            0 -> throw IOException("no connected Garmin Connect IQ device")
            1 -> devices.single()
            else -> throw IOException("expected exactly one connected Garmin device, found ${devices.size}")
        }
    }

    /**
     * Cleanup runs even when the relay failed, but a cleanup failure must never
     * replace the response or the original error.
     */
    private fun unregisterQuietly(device: ConnectIqDevice, iqAppId: String) {
        try {
            sdk.unregisterForAppEvents(device, iqAppId)
        } catch (_: IOException) {
            // Best-effort; the SDK drops listeners on shutdown anyway.
        } catch (_: RuntimeException) {
            // Best-effort; see above.
        }
    }
}

/**
 * Collects the send status and watch reply, which the SDK may deliver in either
 * order. Success requires both a successful send and one valid heartbeat reply;
 * the first failure is terminal.
 */
private class RelayOutcome {
    private val done = CountDownLatch(1)
    private var sent = false
    private var replySeen = false
    private var heartbeat: ByteArray? = null
    private var failure: IOException? = null

    @Synchronized
    fun onSendStatus(success: Boolean) {
        if (finished()) return
        if (!success) {
            fail(IOException("Connect IQ failed to send the challenge to the watch"))
        } else {
            sent = true
            completeIfReady()
        }
    }

    @Synchronized
    fun cancel() {
        if (!finished()) fail(IOException("Garmin Connect IQ relay is closed"))
    }

    @Synchronized
    fun onReply(success: Boolean, payload: List<Any?>) {
        if (finished() || replySeen) return
        replySeen = true
        if (!success) {
            fail(IOException("watch reported a failed Connect IQ message"))
            return
        }
        try {
            heartbeat = decodeHeartbeat(payload)
            completeIfReady()
        } catch (error: IOException) {
            fail(error)
        }
    }

    fun await(timeoutMs: Long): ByteArray {
        try {
            if (!done.await(timeoutMs, TimeUnit.MILLISECONDS)) {
                throw IOException("timed out waiting for the watch heartbeat")
            }
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw IOException("interrupted while waiting for the watch heartbeat", error)
        }
        synchronized(this) {
            failure?.let { throw it }
            return heartbeat!!.copyOf()
        }
    }

    private fun finished(): Boolean = done.count == 0L

    private fun fail(error: IOException) {
        failure = error
        done.countDown()
    }

    private fun completeIfReady() {
        if (sent && heartbeat != null) done.countDown()
    }
}

/** Validates the SDK payload is exactly one 81-byte array; anything else is rejected. */
internal fun encodeConnectIqHex(bytes: ByteArray): String =
    bytes.joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }

private fun decodeHeartbeat(payload: List<Any?>): ByteArray {
    if (payload.size != 1) throw IOException("watch response must contain exactly one heartbeat value")
    val bytes = when (val value = payload.single()) {
        is ByteArray -> value.copyOf()
        is List<*> -> {
            if (value.any { it !is Byte }) throw IOException("watch response contains a non-byte value")
            ByteArray(value.size) { value[it] as Byte }
        }

        is String -> {
            if (value.length != BridgeProtocol.HEARTBEAT_SIZE * 2) {
                throw IOException("watch returned ${value.length / 2} heartbeat bytes; expected ${BridgeProtocol.HEARTBEAT_SIZE}")
            }
            try {
                ByteArray(BridgeProtocol.HEARTBEAT_SIZE) { index ->
                    value.substring(index * 2, index * 2 + 2).toInt(16).toByte()
                }
            } catch (error: NumberFormatException) {
                throw IOException("watch response is not valid hexadecimal", error)
            }
        }

        else -> throw IOException("watch response is neither hexadecimal text nor a byte array")
    }
    if (bytes.size != BridgeProtocol.HEARTBEAT_SIZE) {
        throw IOException("watch returned ${bytes.size} heartbeat bytes; expected ${BridgeProtocol.HEARTBEAT_SIZE}")
    }
    return bytes
}
