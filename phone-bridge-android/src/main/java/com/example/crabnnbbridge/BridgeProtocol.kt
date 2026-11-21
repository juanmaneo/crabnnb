package com.example.crabnnbbridge

import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException

/** TCP wire framing shared with the Rust bridge transport. */
internal object BridgeProtocol {
    const val CHALLENGE_SIZE = 16
    const val HEARTBEAT_SIZE = 81

    @Throws(IOException::class)
    fun readChallenge(input: DataInputStream): ByteArray {
        val length = readFrameLength(input)
        if (length != CHALLENGE_SIZE.toLong()) {
            throw IOException("expected a $CHALLENGE_SIZE-byte challenge frame, got $length bytes")
        }
        val challenge = ByteArray(CHALLENGE_SIZE)
        input.readFully(challenge)
        return challenge
    }

    @Throws(IOException::class)
    fun writeHeartbeat(output: DataOutputStream, heartbeat: ByteArray) {
        if (heartbeat.size != HEARTBEAT_SIZE) {
            throw IOException("watch returned an invalid heartbeat frame")
        }
        writeFrame(output, heartbeat)
    }

    private fun readFrameLength(input: DataInputStream): Long {
        return input.readUnsignedByte().toLong() or
            (input.readUnsignedByte().toLong() shl 8) or
            (input.readUnsignedByte().toLong() shl 16) or
            (input.readUnsignedByte().toLong() shl 24)
    }

    private fun writeFrame(output: DataOutputStream, payload: ByteArray) {
        val length = payload.size
        output.writeByte(length and 0xff)
        output.writeByte((length ushr 8) and 0xff)
        output.writeByte((length ushr 16) and 0xff)
        output.writeByte((length ushr 24) and 0xff)
        output.write(payload)
        output.flush()
    }
}
