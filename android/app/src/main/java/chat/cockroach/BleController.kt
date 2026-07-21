package chat.cockroach

import android.content.Context
import android.os.Build
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import chat.cockroach.ble.BleMeshTransport
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.meshcore_ffi.FfiEvent
import uniffi.meshcore_ffi.FfiMeshNode

/**
 * Drives the REAL BLE transport on-device. With one phone this validates the stack starts
 * (advertising + scanning + GATT server, permissions, no crash); with a second phone running the
 * app, peers discover each other and messages flow over Bluetooth with no internet.
 */
class BleController(private val context: Context, private val scope: CoroutineScope) {

    val log = mutableStateListOf<String>()
    val messages = mutableStateListOf<ChatMessage>()
    val ephId = mutableStateOf("")
    val running = mutableStateOf(false)

    private var node: FfiMeshNode? = null
    private var transport: BleMeshTransport? = null
    private var ticking = false

    fun startBle() {
        if (node != null) {
            log.add("already running")
            return
        }
        val t = BleMeshTransport(
            context = context,
            onLinkUp = { link, mtu -> node?.linkUp(link, mtu) },
            onLinkDown = { link -> node?.linkDown(link) },
            onFrame = { link, frame -> node?.receiveFrame(link, frame) },
            onStatus = { s -> onMain { log.add(s) } },
        )
        val n = FfiMeshNode(
            seed = System.currentTimeMillis().toULong(),
            nickname = Build.MODEL,
            transport = t,
        )
        node = n
        transport = t
        ephId.value = n.ephId()
        running.value = true
        log.add("node up — eph ${n.ephId().take(8)} (${Build.MODEL})")
        t.start()
        if (!ticking) {
            ticking = true
            scope.launch(Dispatchers.Main) {
                var i = 0
                while (true) {
                    tickOnce()
                    // Re-announce our identity every ~3s so peers learn our key even if the
                    // announce sent at link-up was dropped during GATT setup.
                    if (i % 25 == 0) node?.announce()
                    i++
                    delay(120)
                }
            }
        }
    }

    fun send(text: String) {
        val n = node ?: return
        if (text.isBlank()) return
        n.sendChannelMessage("#general", text)
        messages.add(ChatMessage(text, mine = true, verified = true))
    }

    private fun tickOnce() {
        val n = node ?: return
        n.tick()
        for (ev in n.pollEvents()) {
            when (ev) {
                is FfiEvent.Message ->
                    messages.add(ChatMessage(ev.body, mine = false, verified = ev.verified))
                is FfiEvent.PeerAppeared ->
                    log.add("peer identity: ${ev.petname ?: ev.eph.take(8)}")
                is FfiEvent.PeerLost -> log.add("link lost")
            }
        }
    }

    private fun onMain(block: () -> Unit) {
        scope.launch(Dispatchers.Main) { block() }
    }
}
