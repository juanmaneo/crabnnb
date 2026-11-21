package com.example.crabnnbbridge

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.EOFException
import java.io.IOException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class BridgeProtocolTest {
    @Test
    fun readsLittleEndianChallengeFrame() {
        val challenge = ByteArray(BridgeProtocol.CHALLENGE_SIZE) { it.toByte() }
        val input = DataInputStream(ByteArrayInputStream(frame(challenge)))

        assertArrayEquals(challenge, BridgeProtocol.readChallenge(input))
    }

    @Test
    fun readsFragmentedHeaderAndPayload() {
        val challenge = ByteArray(BridgeProtocol.CHALLENGE_SIZE) { (it * 3).toByte() }
        val fragmented = object : ByteArrayInputStream(frame(challenge)) {
            override fun read(buffer: ByteArray, offset: Int, length: Int): Int =
                super.read(buffer, offset, minOf(length, 1))
        }

        assertArrayEquals(challenge, BridgeProtocol.readChallenge(DataInputStream(fragmented)))
    }

    @Test
    fun rejectsUnexpectedAndUnsignedOversizedChallengeLengths() {
        val wrongSize = DataInputStream(ByteArrayInputStream(byteArrayOf(17, 0, 0, 0)))
        assertThrows(IOException::class.java) { BridgeProtocol.readChallenge(wrongSize) }

        val unsignedOversized = DataInputStream(
            ByteArrayInputStream(byteArrayOf(0xff.toByte(), 0xff.toByte(), 0xff.toByte(), 0xff.toByte())),
        )
        assertThrows(IOException::class.java) { BridgeProtocol.readChallenge(unsignedOversized) }
    }

    @Test
    fun rejectsTruncatedHeaderAndPayload() {
        val shortHeader = DataInputStream(ByteArrayInputStream(byteArrayOf(16, 0, 0)))
        assertThrows(EOFException::class.java) { BridgeProtocol.readChallenge(shortHeader) }

        val shortPayload = DataInputStream(ByteArrayInputStream(byteArrayOf(16, 0, 0, 0, 1, 2)))
        assertThrows(EOFException::class.java) { BridgeProtocol.readChallenge(shortPayload) }
    }

    @Test
    fun writesLittleEndianHeartbeatFrame() {
        val heartbeat = ByteArray(BridgeProtocol.HEARTBEAT_SIZE) { (it + 1).toByte() }
        val output = ByteArrayOutputStream()

        BridgeProtocol.writeHeartbeat(DataOutputStream(output), heartbeat)

        assertArrayEquals(frame(heartbeat), output.toByteArray())
    }

    @Test
    fun rejectsInvalidHeartbeatBeforeWritingAnything() {
        val output = ByteArrayOutputStream()

        assertThrows(IOException::class.java) {
            BridgeProtocol.writeHeartbeat(DataOutputStream(output), byteArrayOf(1, 2, 3))
        }
        assertEquals(0, output.size())
    }


    private fun frame(payload: ByteArray): ByteArray = byteArrayOf(
        payload.size.toByte(),
        (payload.size ushr 8).toByte(),
        (payload.size ushr 16).toByte(),
        (payload.size ushr 24).toByte(),
    ) + payload
}
