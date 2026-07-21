package chat.cockroach.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.meshcore_ffi.FfiMeshNode

/**
 * The always-on relay. Holds the [FfiMeshNode] + [BleMeshTransport] and keeps them alive as a
 * foreground service (type `connectedDevice`) so the mesh survives the app leaving the foreground —
 * the persistent notification is the honest "Mesh Active / you're carrying the network" surface.
 *
 * On real hardware this replaces the emulator's loopback. Not started by the emulator demo.
 */
class MeshForegroundService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private lateinit var node: FfiMeshNode
    private lateinit var transport: BleMeshTransport

    override fun onCreate() {
        super.onCreate()
        startForegroundNotified()

        transport = BleMeshTransport(
            context = this,
            onLinkUp = { link, mtu -> node.linkUp(link, mtu) },
            onLinkDown = { link -> node.linkDown(link) },
            onFrame = { link, frame -> node.receiveFrame(link, frame) },
        )
        // Seed derived from a persisted identity in a real build; fixed here for the scaffold.
        node = FfiMeshNode(seed = 1uL, nickname = "me", transport = transport)
        transport.start()

        scope.launch {
            while (true) {
                node.tick()
                node.pollEvents() // a real build forwards these to the UI via a bound interface
                delay(100)
            }
        }
    }

    override fun onDestroy() {
        transport.stop()
        node.panicWipe()
        scope.cancel()
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startForegroundNotified() {
        val channelId = "mesh"
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(channelId, "Mesh Active", NotificationManager.IMPORTANCE_LOW)
        )
        val notification: Notification = Notification.Builder(this, channelId)
            .setContentTitle("Cockroach Chat")
            .setContentText("Mesh active — carrying the network")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(1, notification)
        }
    }
}
