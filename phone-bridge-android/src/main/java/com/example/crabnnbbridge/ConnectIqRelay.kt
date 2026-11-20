package com.example.crabnnbbridge

import java.io.IOException

/** Adapter boundary for the Garmin Connect IQ Mobile SDK integration. */
internal interface ConnectIqRelay {
    @Throws(IOException::class)
    fun sendChallengeAndAwaitHeartbeat(iqAppId: String, challenge: ByteArray): ByteArray
}

/**
 * Compile-time placeholder until the Garmin SDK is installed and its callbacks
 * are wired. It deliberately fails closed instead of fabricating a heartbeat.
 */
internal class ConnectIqMobileSdkStub : ConnectIqRelay {
    override fun sendChallengeAndAwaitHeartbeat(
        iqAppId: String,
        challenge: ByteArray,
    ): ByteArray {
        throw IOException("Garmin Connect IQ Mobile SDK relay is not configured")
    }
}
