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
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.meshcore_ffi.FfiEvent
import uniffi.meshcore_ffi.FfiMeshNode
import java.io.File
import java.security.MessageDigest

data class Peer(val fp: String, val name: String, val verified: Boolean)

/**
 * Drives the REAL BLE transport on-device plus the whole app model the UI observes: the public
 * **Announce** broadcast (rate-limited), ownerless **Nearby** channels, discovered **peers**, and
 * per-peer end-to-end encrypted **DM** threads.
 *
 * Process-lifetime singleton ([get]) with its own main-thread scope — NOT tied to the Activity.
 * [chat.cockroach.ble.MeshForegroundService] keeps the process (and this ticker) alive so the mesh
 * keeps relaying screen-off; the Activity only observes/commands this instance.
 */
class BleController private constructor(context: Context) {

    private val context: Context = context.applicationContext
    private val prefs = this.context.getSharedPreferences("cockroach", Context.MODE_PRIVATE)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    val log = mutableStateListOf<String>()
    val ephId = mutableStateOf("")
    val running = mutableStateOf(false)

    /** User-chosen display name, sent on the wire. Editable at onboarding; applied at node start. */
    val displayName = mutableStateOf(prefs.getString("nick", Build.MODEL) ?: Build.MODEL)

    /** The rate-limited public broadcast feed. */
    val announce: SnapshotStateList<ChatMessage> get() = channel(ANNOUNCE)
    /** Seconds until Announce can post again (0 = ready). */
    val announceCooldown = mutableStateOf(0)

    /** Ownerless channels the user has joined (shown under Nearby), newest activity implied by list. */
    val channels = mutableStateListOf<String>()
    private val channelMessages = mutableStateMapOf<String, SnapshotStateList<ChatMessage>>()

    /** Per-channel lenient rate limit: seconds until a channel accepts another message (0 = ready). */
    val channelCooldown = mutableStateMapOf<String, Int>()
    private val channelSendTimes = HashMap<String, ArrayDeque<Long>>()

    /** Peers learned from announces, keyed by fingerprint hex. */
    val peers = mutableStateListOf<Peer>()
    /** Per-peer encrypted DM threads, keyed by fingerprint hex. */
    val dmThreads = mutableStateMapOf<String, SnapshotStateList<ChatMessage>>()

    /** Rolling relay counters, surfaced on the status screen. */
    val relayedCount = mutableStateOf(0)

    private var node: FfiMeshNode? = null
    private var transport: BleMeshTransport? = null
    private var ticking = false
    private var lastAnnounceMs = 0L

    /** Observable so the UI switches between onboarding and the mesh shell reactively. */
    val onboarded = mutableStateOf(prefs.getBoolean("onboarded", false))

    /** UI language ("en" / "hi"). `langChosen` gates the first-run language picker. */
    val langCode = mutableStateOf(prefs.getString("lang", "en") ?: "en")
    val langChosen = mutableStateOf(prefs.getBoolean("langChosen", false))

    fun setLang(code: String) {
        langCode.value = code
        prefs.edit().putString("lang", code).putBoolean("langChosen", true).apply()
        langChosen.value = true
    }

    fun setDisplayName(name: String) {
        val n = name.trim().ifBlank { Build.MODEL }
        displayName.value = n
        prefs.edit().putString("nick", n).putBoolean("onboarded", true).apply()
        onboarded.value = true
    }

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
        val secrets = KeyVault.loadOrCreate(context)
        val dbPath = File(context.filesDir, KeyVault.DB_NAME).absolutePath
        val n = FfiMeshNode.newPersistent(
            seed = secrets.seed.toULong(),
            nickname = displayName.value,
            transport = t,
            dbPath = dbPath,
            dbKey = secrets.dbKey,
        )
        node = n
        transport = t
        ephId.value = n.ephId()
        running.value = true

        // Subscribe to the broadcast feed + the static public channels, and restore their history.
        joinChannel(ANNOUNCE, silent = true)
        for (name in PUBLIC_CHANNELS) joinChannel(name, silent = true)
        for (ch in listOf(ANNOUNCE) + PUBLIC_CHANNELS.map { normalizeChannel(it) }) restoreHistory(ch, n)

        log.add("node up — eph ${n.ephId().take(8)}")
        t.start()
        if (!ticking) {
            ticking = true
            scope.launch(Dispatchers.Main) {
                var i = 0
                while (true) {
                    tickOnce()
                    updateCooldown()
                    if (i % 25 == 0) node?.announce()
                    i++
                    delay(120)
                }
            }
        }
    }

    private fun restoreHistory(ch: String, n: FfiMeshNode) {
        for (m in n.channelHistory(ch, 200u)) {
            val sender = if (m.mine) "you" else senderName(m.sender)
            channel(ch).add(ChatMessage(m.body, mine = m.mine, verified = true, sender = sender, timestampMs = m.timestampMs.toLong()))
        }
    }

    /** Tear the mesh down WITHOUT destroying data. */
    fun stop() {
        transport?.stop()
        transport = null
        node = null
        running.value = false
        peers.clear()
        dmThreads.clear()
        channelMessages.clear()
        channels.clear()
        log.add("mesh stopped")
    }

    /** Panic: cryptographically erase everything and reset to onboarding. */
    fun panicWipe() {
        runCatching { node?.panicWipe() }
        transport?.stop()
        transport = null
        node = null
        KeyVault.wipe(context)
        prefs.edit().clear().apply()
        displayName.value = Build.MODEL
        onboarded.value = false
        langChosen.value = false
        langCode.value = "en"
        channelMessages.clear()
        channels.clear()
        peers.clear()
        dmThreads.clear()
        log.clear()
        ephId.value = ""
        announceCooldown.value = 0
        relayedCount.value = 0
        running.value = false
    }

    // --- Announce (rate-limited broadcast) ------------------------------------------------------

    fun sendAnnounce(text: String): Boolean {
        val n = node ?: return false
        if (text.isBlank() || announceCooldown.value > 0) return false
        n.sendChannelMessage(ANNOUNCE, text)
        channel(ANNOUNCE).add(ChatMessage(text, mine = true, verified = true, sender = displayName.value, timestampMs = now()))
        lastAnnounceMs = now()
        announceCooldown.value = ANNOUNCE_COOLDOWN_S
        return true
    }

    private fun updateCooldown() {
        val t = now()
        if (lastAnnounceMs != 0L) {
            val remaining = (ANNOUNCE_COOLDOWN_S - (t - lastAnnounceMs) / 1000).toInt().coerceAtLeast(0)
            if (remaining != announceCooldown.value) announceCooldown.value = remaining
        }
        // Per-channel sliding-window cooldown.
        for ((name, times) in channelSendTimes) {
            while (times.isNotEmpty() && t - times.first() >= CHANNEL_WINDOW_MS) times.removeFirst()
            val remaining = if (times.size >= CHANNEL_BURST)
                ((times.first() + CHANNEL_WINDOW_MS - t + 999) / 1000).toInt().coerceAtLeast(0) else 0
            if ((channelCooldown[name] ?: 0) != remaining) channelCooldown[name] = remaining
        }
    }

    // --- Nearby channels ------------------------------------------------------------------------

    fun channel(name: String): SnapshotStateList<ChatMessage> =
        channelMessages.getOrPut(name) { mutableStateListOf() }

    /** The Nearby channel list (excludes the Announce feed). */
    val nearbyChannels: List<String> get() = channels.filter { it != ANNOUNCE }

    fun joinChannel(rawName: String, silent: Boolean = false) {
        val name = normalizeChannel(rawName)
        node?.joinChannel(name)
        channel(name) // ensure a message list exists
        if (name != ANNOUNCE && name !in channels) channels.add(name)
        if (!silent) log.add("joined $name")
    }

    /**
     * Send to a public channel under a lenient rate limit ([CHANNEL_BURST] messages per
     * [CHANNEL_WINDOW_MS]). Returns false (nothing sent) if the channel is currently cooling down.
     */
    fun sendChannel(rawName: String, text: String): Boolean {
        val n = node ?: return false
        if (text.isBlank()) return false
        val name = normalizeChannel(rawName)
        val times = channelSendTimes.getOrPut(name) { ArrayDeque() }
        val t = now()
        while (times.isNotEmpty() && t - times.first() >= CHANNEL_WINDOW_MS) times.removeFirst()
        if (times.size >= CHANNEL_BURST) return false
        n.sendChannelMessage(name, text)
        channel(name).add(ChatMessage(text, mine = true, verified = true, sender = displayName.value, timestampMs = t))
        times.addLast(t)
        return true
    }

    /** Seconds until [rawName] accepts another message (0 = ready). */
    fun channelCooldownFor(rawName: String): Int = channelCooldown[normalizeChannel(rawName)] ?: 0

    fun channelPreview(name: String): ChatMessage? = channel(name).lastOrNull()

    // --- DMs ------------------------------------------------------------------------------------

    fun sendDm(peerFp: String, text: String) {
        val n = node ?: return
        if (text.isBlank()) return
        n.sendDm(peerFp, text)
        thread(peerFp).add(ChatMessage(text, mine = true, verified = true, sender = "you", timestampMs = now()))
    }

    fun thread(fp: String): SnapshotStateList<ChatMessage> =
        dmThreads.getOrPut(fp) { mutableStateListOf() }

    val verifiedPeers: List<Peer> get() = peers.filter { it.verified }
    val unverifiedPeers: List<Peer> get() = peers.filter { !it.verified }
    /** Peers shown in the DM tab: verified peers plus anyone you already have a thread with. */
    val dmPeers: List<Peer> get() = peers.filter { it.verified || dmThreads.containsKey(it.fp) }

    // --- identity / verification ----------------------------------------------------------------

    fun myFingerprint(): String = node?.myFingerprint() ?: ""

    fun verify(peerFp: String) {
        node?.verifyPeer(peerFp)
        // Establish the encrypted session now so the other device also flips to verified immediately.
        node?.startDmSession(peerFp)
        refreshPeer(peerFp)
    }

    fun setPetname(peerFp: String, name: String) {
        node?.setPetname(peerFp, name)
        refreshPeer(peerFp)
    }

    /** Deterministic four-word safety number for a fingerprint — same key → same words on both phones. */
    fun safetyWords(fpHex: String): List<String> {
        if (fpHex.isBlank()) return listOf("—", "—", "—", "—")
        val h = MessageDigest.getInstance("SHA-256").digest(fpHex.lowercase().toByteArray())
        return (0 until 4).map { WORDLIST[(h[it].toInt() and 0xff) % WORDLIST.size] }
    }

    private fun refreshPeer(fp: String) {
        val n = node ?: return
        val name = n.peerPetname(fp) ?: peers.firstOrNull { it.fp == fp }?.name ?: fp.take(8)
        upsertPeer(fp, name, verified = n.peerVerified(fp))
    }

    private fun tickOnce() {
        val n = node ?: return
        n.tick()
        for (ev in n.pollEvents()) {
            when (ev) {
                is FfiEvent.Message -> {
                    relayedCount.value += 1
                    // ev.sender is the rotating ephemeral wire ID (hex) — resolve it to the peer's
                    // announced name / your petname (the name rides their Announce, not the message).
                    val msg = ChatMessage(ev.body, mine = false, verified = ev.verified, sender = senderName(ev.sender), timestampMs = ev.timestampMs.toLong())
                    channel(ev.channel).add(msg)
                }
                is FfiEvent.PeerAppeared -> {
                    ephToFp[ev.eph] = ev.fingerprint
                    val name = n.peerPetname(ev.fingerprint) ?: ev.petname ?: ev.eph.take(8)
                    upsertPeer(ev.fingerprint, name, verified = n.peerVerified(ev.fingerprint))
                }
                is FfiEvent.DirectMessage -> {
                    // Once an identity-bound DM exists, both sides treat the peer as verified (a
                    // deliberate UX: exchanging an E2E-bound DM is itself a trust signal).
                    upsertPeer(ev.sender, peerName(ev.sender), verified = true)
                    thread(ev.sender).add(ChatMessage(ev.text, mine = false, verified = true, sender = peerName(ev.sender), timestampMs = now()))
                }
                is FfiEvent.DmSession -> {
                    upsertPeer(ev.peer, peerName(ev.peer), verified = ev.verified)
                    log.add("DM session ${if (ev.verified) "verified" else "REJECTED"}: ${ev.peer.take(8)}")
                }
                is FfiEvent.PeerLost -> log.add("link lost")
            }
        }
    }

    /** eph wire ID (hex) → fingerprint (hex), learned from announces, so channel messages can be named. */
    private val ephToFp = HashMap<String, String>()

    private fun peerName(fp: String): String =
        peers.firstOrNull { it.fp == fp }?.name ?: fp.take(8)

    /** Friendly name for a channel-message sender given its ephemeral wire ID. */
    private fun senderName(eph: String): String =
        ephToFp[eph]?.let { peerName(it) } ?: eph.take(6)

    private fun upsertPeer(fp: String, name: String, verified: Boolean) {
        val idx = peers.indexOfFirst { it.fp == fp }
        if (idx >= 0) {
            val existing = peers[idx]
            peers[idx] = existing.copy(name = name, verified = existing.verified || verified)
        } else {
            peers.add(Peer(fp, name, verified))
        }
    }

    // Mirror the core's normalization exactly (lower-case, leading '#' implied) so the channel we
    // join/send matches the `channel` field on inbound messages. Chars like '+' are preserved.
    private fun normalizeChannel(name: String): String =
        "#" + name.trim().removePrefix("#").trim().lowercase()

    private fun now(): Long = System.currentTimeMillis()

    private fun onMain(block: () -> Unit) {
        scope.launch(Dispatchers.Main) { block() }
    }

    companion object {
        const val ANNOUNCE = "#announce"
        const val ANNOUNCE_COOLDOWN_S = 60

        // Lenient public-channel rate limit: at most CHANNEL_BURST messages per CHANNEL_WINDOW_MS.
        const val CHANNEL_BURST = 2
        const val CHANNEL_WINDOW_MS = 10_000L

        /** Static public channels for V1 (in-app channel creation is deferred). Scenario-tuned. */
        val PUBLIC_CHANNELS = listOf(
            "general", "alerts", "medics", "supplies", "lost+found", "exits",
        )

        // Compact, evocative wordlist for safety numbers (visual in-person key comparison).
        private val WORDLIST = listOf(
            "river", "anchor", "copper", "lantern", "willow", "ember", "signal", "harbor",
            "meadow", "cobalt", "cedar", "beacon", "pebble", "marble", "orchid", "cinder",
            "falcon", "amber", "quartz", "hollow", "thunder", "pigeon", "walnut", "saffron",
            "glacier", "raven", "bramble", "compass", "lark", "ivory", "maple", "onyx",
            "harvest", "kestrel", "brook", "flint", "otter", "birch", "coral", "dune",
            "ash", "grove", "heron", "juniper", "kelp", "linden", "moss", "nettle",
            "opal", "quill", "reed", "slate", "tundra", "umber", "vale", "wren",
            "yarrow", "zephyr", "basil", "clove", "delta", "fern", "gale", "haze",
        )

        @Volatile
        private var instance: BleController? = null

        fun get(context: Context): BleController =
            instance ?: synchronized(this) {
                instance ?: BleController(context).also { instance = it }
            }
    }
}
