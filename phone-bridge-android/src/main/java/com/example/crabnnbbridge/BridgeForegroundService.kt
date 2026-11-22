package com.example.crabnnbbridge

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import kotlin.concurrent.thread

class BridgeForegroundService : Service() {
    companion object {
        private const val CHANNEL_ID = "crabnnb_bridge"
        private const val NOTIFICATION_ID = 1
        private const val ACTION_STOP = "com.example.crabnnbbridge.STOP"
    }

    private var bridgeServer: BridgeTcpServer? = null
    private var connectIqRelay: ConnectIqMobileSdkRelay? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())

        val relay = ConnectIqRelayFactory.create(applicationContext)
        connectIqRelay = relay
        relay.start()
        val server = BridgeTcpServer(
            relay = relay,
            port = BridgeTcpServer.DEFAULT_PORT,
            onListenerError = { error ->
                Log.e("CrabNNBBridge", "Local bridge listener failed", error)
            },
            onClientError = { error ->
                Log.w("CrabNNBBridge", "Closing bridge client", error)
            },
        )
        bridgeServer = server
        thread(name = "crabnnb-bridge-listener") { server.run() }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        bridgeServer?.close()
        bridgeServer = null
        connectIqRelay?.close()
        connectIqRelay = null
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "CRABNNB phone bridge",
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = "Keeps the Garmin phone bridge available in the background"
            }
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stopService = PendingIntent.getService(
            this,
            1,
            Intent(this, BridgeForegroundService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentTitle("CRABNNB phone bridge active")
            .setContentText("Listening on 127.0.0.1:${BridgeTcpServer.DEFAULT_PORT}")
            .setContentIntent(openApp)
            .setOngoing(true)
            .addAction(0, "Stop", stopService)
            .build()
    }
}
