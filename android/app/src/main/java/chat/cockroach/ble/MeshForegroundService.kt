package chat.cockroach.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.content.ContextCompat
import chat.cockroach.BleController

/**
 * The always-on relay. It does NOT own the mesh — the [BleController] singleton does — its job is to
 * keep the **process** alive and foregrounded (type `connectedDevice`) so the controller's ticker
 * and BLE scanning survive the app leaving the foreground / the screen locking. The persistent
 * notification is the honest "Mesh Active / you're carrying the network" surface.
 *
 * Hardware only (BLE); the emulator demo uses the in-app loopback instead.
 */
class MeshForegroundService : Service() {

    override fun onCreate() {
        super.onCreate()
        startForegroundNotified()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        return when (intent?.action) {
            ACTION_STOP -> {
                BleController.get(this).stop()
                stopSelf()
                START_NOT_STICKY
            }
            ACTION_PANIC -> {
                BleController.get(this).panicWipe()
                stopSelf()
                START_NOT_STICKY
            }
            else -> {
                // Bring the mesh up on the shared controller. Idempotent — safe on restart.
                BleController.get(this).startBle()
                START_STICKY
            }
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startForegroundNotified() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "Mesh Active", NotificationManager.IMPORTANCE_LOW)
        )
        val stopIntent = PendingIntent.getService(
            this,
            0,
            Intent(this, MeshForegroundService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification: Notification = Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Cockroach Chat")
            .setContentText("Mesh active — carrying the network")
            .setSmallIcon(chat.cockroach.R.drawable.ic_notification)
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, "Stop", stopIntent).build())
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIF_ID, notification)
        }
    }

    companion object {
        private const val CHANNEL_ID = "mesh"
        private const val NOTIF_ID = 1
        const val ACTION_STOP = "chat.cockroach.action.STOP"
        const val ACTION_PANIC = "chat.cockroach.action.PANIC"

        /** Start the mesh in a foreground service (survives backgrounding / screen-off). */
        fun start(context: Context) =
            ContextCompat.startForegroundService(context, intent(context, null))

        /** Stop relaying and tear the service down (data preserved). */
        fun stop(context: Context) =
            ContextCompat.startForegroundService(context, intent(context, ACTION_STOP))

        /** Panic-wipe everything, then tear the service down. */
        fun panic(context: Context) =
            ContextCompat.startForegroundService(context, intent(context, ACTION_PANIC))

        private fun intent(context: Context, action: String?): Intent =
            Intent(context, MeshForegroundService::class.java).also { if (action != null) it.action = action }
    }
}
