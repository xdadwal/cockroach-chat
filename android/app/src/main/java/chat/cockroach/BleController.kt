package chat.cockroach

import android.content.Context
import android.os.Build
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.snapshots.SnapshotStateList
import chat.cockroach.ble.BleMeshTransport
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.meshcore_ffi.FfiEvent
import uniffi.meshcore_ffi.FfiMeshNode

data class Peer(val fp: String, val name: String, val verified: Boolean)

/**
 * Drives the REAL BLE transport on-device and now also the direct-message layer: discovered peers
 * are tracked (from signed announces) and each can hold an end-to-end encrypted DM thread.
 */
class BleController(private val context: Context, private val scope: CoroutineScope) {

    val log = mutableStateListOf<String>()
    val messages = mutableStateListOf<ChatMessage>() // #general channel
    val ephId = mutableStateOf("")
    val running = mutableStateOf(false)

    /** Peers learned from announces, keyed by fingerprint hex. */
    val peers = mutableStateListOf<Peer>()

    /** Per-peer encrypted DM threads, keyed by fingerprint hex. */
    val dmThreads = mutableStateMapOf<String, SnapshotStateList<ChatMessage>>()

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

    /** Send an end-to-end encrypted DM to a peer (by fingerprint hex). */
    fun sendDm(peerFp: String, text: String) {
        val n = node ?: return
        if (text.isBlank()) return
        n.sendDm(peerFp, text)
        thread(peerFp).add(ChatMessage(text, mine = true, verified = true))
    }

    fun thread(fp: String): SnapshotStateList<ChatMessage> =
        dmThreads.getOrPut(fp) { mutableStateListOf() }

    private fun tickOnce() {
        val n = node ?: return
        n.tick()
        for (ev in n.pollEvents()) {
            when (ev) {
                is FfiEvent.Message ->
                    messages.add(ChatMessage(ev.body, mine = false, verified = ev.verified))
                is FfiEvent.PeerAppeared ->
                    upsertPeer(ev.fingerprint, ev.petname ?: ev.eph.take(8), verified = false)
                is FfiEvent.DirectMessage -> {
                    upsertPeer(ev.sender, ev.sender.take(8), verified = true)
                    thread(ev.sender).add(ChatMessage(ev.text, mine = false, verified = true))
                }
                is FfiEvent.DmSession -> {
                    upsertPeer(ev.peer, peerName(ev.peer), verified = ev.verified)
                    log.add("DM session ${if (ev.verified) "verified" else "REJECTED"}: ${ev.peer.take(8)}")
                }
                is FfiEvent.PeerLost -> log.add("link lost")
            }
        }
    }

    private fun peerName(fp: String): String =
        peers.firstOrNull { it.fp == fp }?.name ?: fp.take(8)

    private fun upsertPeer(fp: String, name: String, verified: Boolean) {
        val idx = peers.indexOfFirst { it.fp == fp }
        if (idx >= 0) {
            val existing = peers[idx]
            peers[idx] = existing.copy(name = name, verified = existing.verified || verified)
        } else {
            peers.add(Peer(fp, name, verified))
        }
    }

    private fun onMain(block: () -> Unit) {
        scope.launch(Dispatchers.Main) { block() }
    }
}
