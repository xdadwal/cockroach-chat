package chat.cockroach

import androidx.compose.runtime.mutableStateListOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.meshcore_ffi.BleTransport
import uniffi.meshcore_ffi.FfiEvent
import uniffi.meshcore_ffi.FfiMeshNode
import java.util.concurrent.ConcurrentLinkedQueue

data class ChatMessage(val body: String, val mine: Boolean, val verified: Boolean)

/**
 * Runs TWO real [FfiMeshNode]s (backed by the Rust core) inside the app, wired to each other by an
 * in-memory loopback that stands in for BLE — which the emulator cannot provide. Every message you
 * see cross from one pane to the other has gone through the real packet codec, signing, dedup, and
 * relay engine. Swapping this loopback for the CoreBluetooth/Android-BLE transport is the only
 * remaining step for real hardware.
 *
 * Frames are delivered asynchronously (queued, drained on the ticker) so a node never calls its
 * peer's `receiveFrame` while holding its own lock — that would reentrantly deadlock.
 */
class MeshController(private val scope: CoroutineScope) {

    private data class Frame(val target: Int, val link: ULong, val bytes: ByteArray)

    private val queue = ConcurrentLinkedQueue<Frame>()
    private val nodes: List<FfiMeshNode>

    /** Chat log as seen on each phone. */
    val phoneA = mutableStateListOf<ChatMessage>()
    val phoneB = mutableStateListOf<ChatMessage>()
    val status = mutableStateListOf<String>()

    private val names = listOf("Ava", "Ben")
    private val logs get() = listOf(phoneA, phoneB)

    init {
        // Node i's transport enqueues frames for its peer (1 - i).
        nodes = (0..1).map { i ->
            val peer = 1 - i
            val transport = object : BleTransport {
                override fun send(link: ULong, frame: ByteArray) {
                    queue.add(Frame(peer, link, frame))
                }
            }
            FfiMeshNode(seed = (i + 1).toULong(), nickname = names[i], transport = transport)
        }
    }

    fun start() {
        // Bring the single shared link up on both ends (mtu 182 = iOS floor). Triggers announces.
        val link = 1UL
        nodes.forEach { it.linkUp(link, 182u) }
        status.add("link up — Ava ↔ Ben (loopback transport, no BLE)")
        scope.launch(Dispatchers.Main) {
            while (true) {
                pump()
                delay(60)
            }
        }
    }

    fun send(fromPhone: Int, text: String) {
        if (text.isBlank()) return
        nodes[fromPhone].sendChannelMessage("#general", text)
        logs[fromPhone].add(ChatMessage(text, mine = true, verified = true))
    }

    private fun pump() {
        // 1) deliver queued frames into the target node
        while (true) {
            val f = queue.poll() ?: break
            nodes[f.target].receiveFrame(f.link, f.bytes)
        }
        // 2) advance timers (releases jittered rebroadcasts)
        nodes.forEach { it.tick() }
        // 3) surface events per node
        for (i in nodes.indices) {
            for (ev in nodes[i].pollEvents()) {
                when (ev) {
                    is FfiEvent.Message ->
                        logs[i].add(ChatMessage(ev.body, mine = false, verified = ev.verified))
                    is FfiEvent.PeerAppeared ->
                        status.add("${names[i]} sees peer ${ev.petname ?: ev.eph.take(8)}")
                    is FfiEvent.PeerLost -> status.add("${names[i]} lost a link")
                }
            }
        }
    }
}
