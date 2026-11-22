package com.example.crabnnbbridge

import android.content.Context
import android.util.Log
import com.garmin.android.connectiq.ConnectIQ
import com.garmin.android.connectiq.IQApp
import com.garmin.android.connectiq.IQDevice
import com.garmin.android.connectiq.exception.InvalidStateException
import com.garmin.android.connectiq.exception.ServiceUnavailableException
import java.io.IOException

/** Android/Garmin SDK construction boundary kept separate from JVM relay logic. */
internal object ConnectIqRelayFactory {
    fun create(context: Context): ConnectIqMobileSdkRelay =
        ConnectIqMobileSdkRelay(GarminConnectIqSdk(context.applicationContext))
}

/** Production adapter over Garmin's Connect IQ Mobile SDK 2.4.0 (WIRELESS mode). */
private class GarminConnectIqSdk(private val appContext: Context) : ConnectIqSdk {
    private val connectIq: ConnectIQ = ConnectIQ.getInstance(appContext, ConnectIQ.IQConnectType.WIRELESS)

    @Volatile
    private var ready = false

    @Volatile
    private var initializationStarted = false

    @Volatile
    private var shuttingDown = false

    override fun initialize(onReady: (IOException?) -> Unit) {
        initializationStarted = true
        shuttingDown = false
        connectIq.initialize(appContext, false, object : ConnectIQ.ConnectIQListener {
            override fun onSdkReady() {
                if (shuttingDown) return
                ready = true
                onReady(null)
            }

            override fun onInitializeError(status: ConnectIQ.IQSdkErrorStatus) {
                onReady(IOException("Garmin Connect IQ SDK initialization failed: $status"))
            }

            override fun onSdkShutDown() {
                ready = false
            }
        })
    }

    override fun connectedDevices(): List<ConnectIqDevice> {
        requireReady()
        return sdkCall {
            connectIq.connectedDevices
                .filter { connectIq.getDeviceStatus(it) == IQDevice.IQDeviceStatus.CONNECTED }
                .map { ConnectIqDevice(it.deviceIdentifier, it) }
        }
    }

    override fun registerForAppEvents(
        device: ConnectIqDevice,
        iqAppId: String,
        onMessage: (appId: String, deviceId: Long, success: Boolean, payload: List<Any?>) -> Unit,
    ) {
        requireReady()
        sdkCall {
            connectIq.registerForAppEvents(
                nativeDevice(device),
                IQApp(iqAppId),
                object : ConnectIQ.IQApplicationEventListener {
                    override fun onMessageReceived(
                        receivedDevice: IQDevice,
                        app: IQApp,
                        payload: List<Any>,
                        status: ConnectIQ.IQMessageStatus,
                    ) {
                        Log.i(
                            "CrabNNBBridge",
                            "watch reply status=$status app=${app.applicationId} device=${receivedDevice.deviceIdentifier} payload=${
                                describePayload(
                                    payload
                                )
                            }",
                        )
                        onMessage(
                            app.applicationId,
                            receivedDevice.deviceIdentifier,
                            status == ConnectIQ.IQMessageStatus.SUCCESS,
                            payload,
                        )
                    }
                },
            )
        }
    }

    override fun sendMessage(
        device: ConnectIqDevice,
        iqAppId: String,
        message: ByteArray,
        onStatus: (appId: String, deviceId: Long, success: Boolean) -> Unit,
    ) {
        requireReady()
        sdkCall {
            connectIq.sendMessage(
                nativeDevice(device),
                IQApp(iqAppId),
                encodeConnectIqHex(message),
                object : ConnectIQ.IQSendMessageListener {
                    override fun onMessageStatus(
                        sentDevice: IQDevice,
                        sentApp: IQApp,
                        status: ConnectIQ.IQMessageStatus,
                    ) {
                        Log.i(
                            "CrabNNBBridge",
                            "challenge send status=$status app=${sentApp.applicationId} device=${sentDevice.deviceIdentifier}",
                        )
                        onStatus(
                            sentApp.applicationId,
                            sentDevice.deviceIdentifier,
                            status == ConnectIQ.IQMessageStatus.SUCCESS,
                        )
                    }
                },
            )
        }
    }

    override fun unregisterForAppEvents(device: ConnectIqDevice, iqAppId: String) {
        sdkCall { connectIq.unregisterForApplicationEvents(nativeDevice(device), IQApp(iqAppId)) }
    }

    override fun shutdown() {
        if (!initializationStarted) return
        shuttingDown = true
        ready = false
        initializationStarted = false
        try {
            connectIq.shutdown(appContext)
        } catch (_: InvalidStateException) {
            // Best-effort during Activity teardown, including failed initialization.
        } catch (_: RuntimeException) {
            // Garmin SDK cleanup must not crash Activity teardown.
        }
    }

    private fun requireReady() {
        if (!ready) throw IOException("Garmin Connect IQ SDK is not ready")
    }

    // Logs only the shape of the reply, never its contents.
    private fun describePayload(payload: List<Any>): String =
        payload.joinToString(prefix = "[", postfix = "]") { value ->
            when (value) {
                is String -> "String(${value.length})"
                is ByteArray -> "ByteArray(${value.size})"
                is List<*> -> "List(${value.size})"
                else -> value.javaClass.simpleName
            }
        }

    private fun nativeDevice(device: ConnectIqDevice): IQDevice =
        device.handle as? IQDevice ?: throw IOException("invalid Connect IQ device handle")

    private inline fun <T> sdkCall(block: () -> T): T {
        try {
            return block()
        } catch (error: InvalidStateException) {
            throw IOException("Garmin Connect IQ SDK is in an invalid state", error)
        } catch (error: ServiceUnavailableException) {
            throw IOException("Garmin Connect Mobile is unavailable", error)
        }
    }
}
