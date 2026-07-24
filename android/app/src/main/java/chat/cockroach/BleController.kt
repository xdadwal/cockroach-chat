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

data class Peer(
    val fp: String,
    val name: String,
    val verified: Boolean,
    /** True once an inbound verification control message (notice or ack) proves the peer's side
     *  of the encrypted channel works — the "confirmed both ends" badge state. */
    val confirmed: Boolean = false,
)

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
        // Reload verified contacts + their DM threads from the encrypted store, so they survive a
        // stop/restart (or process death) instead of vanishing until each peer re-announces.
        restoreContacts(n)

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
        // Authoritative reload: clear first so a restart doesn't duplicate the retained in-memory list.
        val list = channel(ch)
        list.clear()
        for (m in n.channelHistory(ch, 200u)) {
            val sender = if (m.mine) "you" else senderName(m.sender)
            list.add(ChatMessage(m.body, mine = m.mine, verified = true, sender = sender, timestampMs = m.timestampMs.toLong()))
        }
    }

    /** Reload persisted peers (with verified/petname) and each one's DM thread from the store.
     *  `confirmed` is derived, not stored: any inbound verification control row proves the
     *  peer's side of the channel worked. */
    private fun restoreContacts(n: FfiMeshNode) {
        peers.clear()
        for (p in n.listPeers()) {
            val name = n.peerPetname(p.fingerprint) ?: p.petname ?: p.fingerprint.take(8)
            val t = thread(p.fingerprint)
            t.clear()
            var confirmed = false
            for (dm in n.dmHistory(p.fingerprint, 200u)) {
                val kind = dm.kind.toInt()
                if (!dm.mine && kind != KIND_TEXT) confirmed = true
                val sender = if (kind != KIND_TEXT) name else if (dm.mine) "you" else name
                t.add(ChatMessage(dm.body, mine = dm.mine, verified = true, sender = sender, timestampMs = dm.timestampMs.toLong(), kind = kind))
            }
            upsertPeer(p.fingerprint, name, verified = p.verified, confirmed = confirmed)
        }
    }

    /** Tear the mesh down WITHOUT destroying data. */
    fun stop() {
        // Tear down only the radio and core node. Do NOT wipe the display state (contacts, channel
        // and DM history): it is persisted and reloaded on the next start. Clearing it here is what
        // made verified contacts and messages vanish on mesh-off. `running=false` switches the UI to
        // the mesh-off screen; the lists stay intact for when the mesh comes back.
        transport?.stop()
        transport = null
        node = null
        running.value = false
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
        // Show our own verify-notice in the thread right away (the core sends it as soon as the
        // session is ready; the peer's ack arriving is what flips the badge to "confirmed").
        thread(peerFp).add(ChatMessage("", mine = true, verified = true, sender = peerName(peerFp), timestampMs = now(), kind = KIND_NOTICE))
    }

    /** Undo a verification: badge resets, chat history and petname survive; re-scan to verify. */
    fun unverify(peerFp: String) {
        val n = node ?: return
        n.unverifyPeer(peerFp)
        // Rebuild the thread from the store (control rows are gone) and hard-reset the badge —
        // upsertPeer is deliberately monotonic, so downgrade explicitly.
        val name = peerName(peerFp)
        val t = thread(peerFp)
        t.clear()
        for (dm in n.dmHistory(peerFp, 200u)) {
            t.add(ChatMessage(dm.body, mine = dm.mine, verified = true, sender = if (dm.mine) "you" else name, timestampMs = dm.timestampMs.toLong(), kind = dm.kind.toInt()))
        }
        val idx = peers.indexOfFirst { it.fp == peerFp }
        if (idx >= 0) peers[idx] = peers[idx].copy(verified = false, confirmed = false)
        log.add("unverified ${peerFp.take(8)}")
    }

    /** Forget a contact entirely: peer record, DM thread, session state. */
    fun forget(peerFp: String) {
        node?.forgetPeer(peerFp)
        peers.removeAll { it.fp == peerFp }
        dmThreads.remove(peerFp)
        log.add("removed ${peerFp.take(8)}")
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
                    val kind = ev.kind.toInt()
                    val name = peerName(ev.sender)
                    // The badge follows the persisted verified flag (set only by a QR scan); an
                    // inbound control message (notice or ack) is live proof the peer's side of
                    // the encrypted channel works -> confirmed.
                    upsertPeer(ev.sender, name, verified = n.peerVerified(ev.sender), confirmed = kind == KIND_NOTICE || kind == KIND_ACK)
                    thread(ev.sender).add(ChatMessage(ev.text, mine = false, verified = true, sender = name, timestampMs = now(), kind = kind))
                }
                is FfiEvent.DmSession -> {
                    upsertPeer(ev.peer, peerName(ev.peer), verified = n.peerVerified(ev.peer))
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

    private fun upsertPeer(fp: String, name: String, verified: Boolean, confirmed: Boolean = false) {
        val idx = peers.indexOfFirst { it.fp == fp }
        if (idx >= 0) {
            val existing = peers[idx]
            peers[idx] = existing.copy(name = name, verified = existing.verified || verified, confirmed = existing.confirmed || confirmed)
        } else {
            peers.add(Peer(fp, name, verified, confirmed))
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

        /** DM kinds mirrored from the core: 0 text, 1 verify-notice, 2 verify-ack. */
        const val KIND_TEXT = 0
        const val KIND_NOTICE = 1
        const val KIND_ACK = 2

        // Lenient public-channel rate limit: at most CHANNEL_BURST messages per CHANNEL_WINDOW_MS.
        const val CHANNEL_BURST = 2
        const val CHANNEL_WINDOW_MS = 10_000L

        /** Static public channels for V1 (in-app channel creation is deferred). Scenario-tuned. */
        val PUBLIC_CHANNELS = listOf(
            "general", "alerts", "medics", "supplies", "lost+found", "exits", "meme",
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
