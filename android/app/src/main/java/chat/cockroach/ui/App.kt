@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package chat.cockroach.ui

import android.Manifest
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Place
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Icon
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.graphics.Color
import chat.cockroach.BleController
import chat.cockroach.ChatMessage
import chat.cockroach.Peer
import chat.cockroach.ble.MeshForegroundService
import kotlinx.coroutines.delay
import kotlin.math.min
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeEncoder
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions

// --- small helpers ----------------------------------------------------------------------------

private fun shortId(eph: String): String =
    if (eph.length < 6) eph.uppercase() else eph.take(6).uppercase().chunked(2).joinToString("·")

internal fun Modifier.bottomHairline() = drawBehind {
    drawLine(SolidColor(CcInk.copy(alpha = 0.1f)), Offset(0f, size.height), Offset(size.width, size.height), 1f)
}

@Composable
private fun QrImage(content: String, modifier: Modifier = Modifier, px: Int = 512) {
    val bmp = remember(content) {
        runCatching { BarcodeEncoder().encodeBitmap(content, BarcodeFormat.QR_CODE, px, px) }.getOrNull()
    }
    if (bmp != null) Image(bmp.asImageBitmap(), "fingerprint QR", modifier)
}

// --- navigation ---------------------------------------------------------------------------------

private enum class NavTab(val label: String) { Feed("Feed"), Me("Me") }

private sealed interface Route {
    data class Channel(val name: String) : Route
    data class Dm(val fp: String) : Route
    data class Verify(val peerFp: String?, val scan: Boolean = false) : Route
    data object Status : Route
    data object Credits : Route
}

@Composable
fun CockroachApp(ble: BleController) {
    val hi = ble.langCode.value == "hi"
    CompositionLocalProvider(
        LocalStrings provides stringsFor(ble.langCode.value),
        LocalFontScale provides if (hi) 1.22f else 1f,
    ) {
        when {
            !ble.langChosen.value -> LanguageChoiceScreen(ble)
            !ble.onboarded.value -> OnboardingNameScreen(ble)
            !ble.running.value -> MeshOffScreen(ble)
            else -> MeshShell(ble)
        }
    }
}

@Composable
private fun LanguageChoiceScreen(ble: BleController) {
    val s = LocalStrings.current
    Column(Modifier.fillMaxSize().background(CcBase).padding(horizontal = 26.dp)) {
        Spacer(Modifier.weight(1f))
        CcText("Cockroach\nChat.", 40, FontWeight.Black, CcInk, upper = true, letterSpacing = (-1.0), lineHeightMul = 0.95)
        Spacer(Modifier.height(28.dp))
        CcText(s.langTitle, 24, FontWeight.ExtraBold, CcInk, letterSpacing = (-0.3))
        Spacer(Modifier.height(10.dp))
        CcText(s.langSubtitle, 14, FontWeight.Medium, CcInkMute(0.6f), lineHeightMul = 1.5)
        Spacer(Modifier.height(26.dp))
        Lang.entries.forEach { lang ->
            val on = ble.langCode.value == lang.code
            Row(
                Modifier.fillMaxWidth().padding(bottom = 12.dp).clip(RoundedCornerShape(13.dp))
                    .background(if (on) CcAmber.copy(alpha = 0.16f) else CcRaised)
                    .border(1.dp, if (on) CcAmber.copy(alpha = 0.5f) else CcInkMute(0.12f), RoundedCornerShape(13.dp))
                    .clickable { ble.langCode.value = lang.code } // live preview; committed on Continue
                    .padding(horizontal = 18.dp, vertical = 17.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                CcText(lang.display, 17, FontWeight.Bold, if (on) CcAmberText else CcInk, modifier = Modifier.weight(1f))
                if (on) Icon(Icons.Filled.Check, null, tint = CcAmberText, modifier = Modifier.size(20.dp))
            }
        }
        Spacer(Modifier.weight(1f))
        CcPrimaryButton(s.langContinue, { ble.setLang(ble.langCode.value) }, Modifier.fillMaxWidth())
        Spacer(Modifier.height(26.dp))
    }
}

@Composable
private fun MeshShell(ble: BleController) {
    var tab by remember { mutableStateOf(NavTab.Feed) }
    // Hoisted here (not inside FeedScreen) so the Announce/Nearby/Verified selection survives while a
    // chat detail screen is on top — otherwise back always lands on Announce.
    var feedTab by remember { mutableStateOf(FeedTab.Announce) }
    val stack = remember { mutableStateListOf<Route>() }
    val top = stack.lastOrNull()
    fun push(r: Route) { stack.add(r) }
    fun pop() { if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex) }
    BackHandler(enabled = stack.isNotEmpty()) { pop() }

    Column(Modifier.fillMaxSize().background(CcBase)) {
        Box(Modifier.weight(1f)) {
            when (val r = top) {
                is Route.Channel -> ChannelScreen(ble, r.name, ::pop)
                is Route.Dm -> DmScreen(ble, r.fp, ::pop)
                is Route.Verify -> VerifyFlow(ble, r.peerFp, r.scan, onDone = { fp -> pop(); if (fp != null) push(Route.Dm(fp)) }, onCancel = ::pop)
                Route.Status -> StatusScreen(ble, ::pop)
                Route.Credits -> CreditsScreen(::pop)
                null -> when (tab) {
                    NavTab.Feed -> FeedScreen(ble, feedTab, { feedTab = it }, onOpenChannel = { push(Route.Channel(it)) }, onOpenDm = { push(Route.Dm(it)) }, onStatus = { push(Route.Status) })
                    NavTab.Me -> IdentityScreen(
                        ble,
                        onShowQr = { push(Route.Verify(null, scan = false)) },
                        onScanQr = { push(Route.Verify(null, scan = true)) },
                        onCredits = { push(Route.Credits) },
                    )
                }
            }
        }
        if (top == null) BottomBar(tab) { tab = it }
    }
}

@Composable
private fun BottomBar(current: NavTab, onSelect: (NavTab) -> Unit) {
    val s = LocalStrings.current
    Row(Modifier.fillMaxWidth().background(CcNav).drawBehind {
        drawLine(SolidColor(CcInk.copy(alpha = 0.1f)), Offset(0f, 0f), Offset(size.width, 0f), 1f)
    }.padding(vertical = 7.dp)) {
        val items = listOf(
            Triple(NavTab.Feed, Icons.Filled.Home, s.navFeed),
            Triple(NavTab.Me, Icons.Filled.AccountCircle, s.navMe),
        )
        items.forEach { (t, icon, label) ->
            val on = t == current
            Column(
                Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).clickable { onSelect(t) }.padding(vertical = 4.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Icon(icon, label, tint = if (on) CcAmberText else CcInkMute(0.5f), modifier = Modifier.size(22.dp))
                Spacer(Modifier.height(3.dp))
                CcText(label, 10, FontWeight.SemiBold, if (on) CcAmberText else CcInkMute(0.5f), mono = true, upper = true)
            }
        }
    }
}

@Composable
private fun AppHeader(ble: BleController, onStatus: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 16.dp, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp),
    ) {
        CcText("Cockroach", 17, FontWeight.Black, CcInk, upper = true, letterSpacing = (-0.3))
        Spacer(Modifier.weight(1f))
        MeshChip(MeshState.Live, Modifier.clickable(onClick = onStatus))
        ShortIdChip(shortId(ble.ephId.value))
    }
}

// --- onboarding ---------------------------------------------------------------------------------

@Composable
private fun OnboardingNameScreen(ble: BleController) {
    val s = LocalStrings.current
    var name by remember { mutableStateOf(ble.displayName.value) }
    Column(Modifier.fillMaxSize().background(CcBase).padding(horizontal = 26.dp)) {
        Spacer(Modifier.weight(1f))
        CcText("Cockroach\nChat.", 44, FontWeight.Black, CcInk, upper = true, letterSpacing = (-1.0), lineHeightMul = 0.95)
        Spacer(Modifier.height(16.dp))
        CcText(s.onbTagline, 15, FontWeight.Medium, CcInkMute(0.62f), lineHeightMul = 1.5)
        Spacer(Modifier.height(30.dp))
        SectionLabel(s.onbNameLabel)
        Spacer(Modifier.height(9.dp))
        NameField(name, { if (it.length <= 24) name = it }, s.onbNamePlaceholder)
        Spacer(Modifier.height(14.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            Icon(Icons.Filled.Warning, null, tint = CcWarning, modifier = Modifier.size(15.dp).padding(top = 1.dp))
            CcText(s.onbNameWarn, 12, FontWeight.Medium, CcInkMute(0.55f), lineHeightMul = 1.45)
        }
        Spacer(Modifier.weight(1f))
        CcPrimaryButton(s.onbEnter, { ble.setDisplayName(name) }, Modifier.fillMaxWidth())
        Spacer(Modifier.height(26.dp))
    }
}

@Composable
private fun NameField(value: String, onChange: (String) -> Unit, placeholder: String, accent: Color = CcAmber) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(CcRaised)
            .border(1.dp, accent.copy(alpha = 0.5f), RoundedCornerShape(13.dp)).padding(horizontal = 16.dp, vertical = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (value.isEmpty()) CcText(placeholder, 17, FontWeight.SemiBold, CcInkMute(0.4f))
            androidx.compose.foundation.text.BasicTextField(
                value = value, onValueChange = onChange, singleLine = true,
                textStyle = androidx.compose.ui.text.TextStyle(fontFamily = Archivo, fontWeight = FontWeight.SemiBold, fontSize = 17.sp, color = CcInk),
                cursorBrush = SolidColor(accent),
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = ImeAction.Done),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        CcText("${value.length} / 24", 11, FontWeight.SemiBold, CcInkMute(0.4f), mono = true)
    }
}

// --- mesh off (start) ---------------------------------------------------------------------------

@Composable
private fun MeshOffScreen(ble: BleController) {
    val s = LocalStrings.current
    val context = LocalContext.current
    val perms = remember {
        buildList {
            if (Build.VERSION.SDK_INT >= 31) { add(Manifest.permission.BLUETOOTH_SCAN); add(Manifest.permission.BLUETOOTH_CONNECT); add(Manifest.permission.BLUETOOTH_ADVERTISE) }
            if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
        }.toTypedArray()
    }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        val granted = result.filterKeys { it != Manifest.permission.POST_NOTIFICATIONS }.all { it.value }
        if (granted) MeshForegroundService.start(context) else ble.log.add("BLE permissions denied")
    }
    Column(Modifier.fillMaxSize().background(CcBase)) {
        Row(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 18.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            CcText("Cockroach", 18, FontWeight.Black, CcInk, upper = true, letterSpacing = (-0.4))
            Spacer(Modifier.weight(1f))
            MeshChip(MeshState.Off)
        }
        Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 34.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            Box(Modifier.size(132.dp).dashedBorder(CcInkMute(0.22f), 66.dp, 2.dp), contentAlignment = Alignment.Center) {
                Box(Modifier.size(88.dp).clip(RoundedCornerShape(50)).background(CcAmber), contentAlignment = Alignment.Center) {
                    BroadcastGlyph(CcOnAmber, 40.dp)
                }
            }
            Spacer(Modifier.height(30.dp))
            CcText(s.offTitle, 24, FontWeight.ExtraBold, CcInk, align = androidx.compose.ui.text.style.TextAlign.Center, letterSpacing = (-0.3))
            Spacer(Modifier.height(12.dp))
            CcText(s.offBody, 14, FontWeight.Medium, CcInkMute(0.6f), align = androidx.compose.ui.text.style.TextAlign.Center, lineHeightMul = 1.55)
            Spacer(Modifier.height(26.dp))
            SectionLabel(s.offHint, CcInkMute(0.42f))
        }
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 22.dp)) {
            CcPrimaryButton(s.offStart, { launcher.launch(perms) }, Modifier.fillMaxWidth())
        }
    }
}

// --- FEED (Announce / Nearby / Verified) --------------------------------------------------------

@Composable
private fun FeedScreen(ble: BleController, tab: FeedTab, onTab: (FeedTab) -> Unit, onOpenChannel: (String) -> Unit, onOpenDm: (String) -> Unit, onStatus: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        AppHeader(ble, onStatus)
        FeedTabs(tab, onTab)
        when (tab) {
            FeedTab.Announce -> AnnounceTab(ble)
            FeedTab.Nearby -> NearbyTab(ble, onOpenChannel)
            FeedTab.Verified -> VerifiedTab(ble, onOpenDm)
        }
    }
}

@Composable
private fun ColumnScope.AnnounceTab(ble: BleController) {
    val s = LocalStrings.current
    PublicBanner()
    MessageList(ble.announce, Modifier.weight(1f), pad = 14, gap = 12)
    var draft by remember { mutableStateOf("") }
    val cooldown = ble.announceCooldown.value
    Column(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 14.dp, vertical = 10.dp)) {
        if (cooldown > 0) {
            CooldownCard(cooldown, BleController.ANNOUNCE_COOLDOWN_S, s.coolTitle, s.coolAnnounceSub.format(cooldown))
        } else {
            Composer(draft, { draft = it }, { if (ble.sendAnnounce(draft)) draft = "" }, s.composerAnnounce, accent = CcPublic)
            Spacer(Modifier.height(7.dp))
            CcText(s.announceRate, 10, FontWeight.Medium, CcInkMute(0.4f), mono = true, align = androidx.compose.ui.text.style.TextAlign.Center, modifier = Modifier.fillMaxWidth())
        }
    }
}

@Composable
private fun CooldownCard(remaining: Int, total: Int, title: String, subtitle: String) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(CcRaised)
            .border(1.dp, CcInkMute(0.1f), RoundedCornerShape(13.dp)).padding(15.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(34.dp), contentAlignment = Alignment.Center) {
            val frac = (remaining / total.toFloat()).coerceIn(0f, 1f)
            Canvas(Modifier.size(34.dp)) {
                val sw = 3.5.dp.toPx()
                drawArc(CcAmber.copy(alpha = 0.12f), 0f, 360f, false, style = Stroke(sw), topLeft = Offset(sw / 2, sw / 2), size = androidx.compose.ui.geometry.Size(size.width - sw, size.height - sw))
                drawArc(CcAmber, -90f, 360f * frac, false, style = Stroke(sw, cap = androidx.compose.ui.graphics.StrokeCap.Round), topLeft = Offset(sw / 2, sw / 2), size = androidx.compose.ui.geometry.Size(size.width - sw, size.height - sw))
            }
            CcText("$remaining", 11, FontWeight.Bold, CcAmberText, mono = true)
        }
        Column {
            CcText(title, 13, FontWeight.ExtraBold, CcInk)
            CcText(subtitle, 11, FontWeight.Medium, CcInkMute(0.55f), lineHeightMul = 1.3)
        }
    }
}

@Composable
private fun ColumnScope.NearbyTab(ble: BleController, onOpen: (String) -> Unit) {
    val s = LocalStrings.current
    Column(Modifier.weight(1f).fillMaxWidth()) {
        SectionLabel("${s.publicChannelsLabel} · ${ble.nearbyChannels.size}", modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 8.dp))
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 12.dp)) {
            items(ble.nearbyChannels) { name -> ChannelRow(ble, name, onOpen) }
        }
        Row(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp)) {
            BroadcastGlyph(CcPublicText, 14.dp)
            CcText(s.channelsFooter, 11, FontWeight.Medium, CcInkMute(0.5f), lineHeightMul = 1.35)
        }
    }
}

@Composable
private fun ChannelRow(ble: BleController, name: String, onOpen: (String) -> Unit) {
    val s = LocalStrings.current
    val last = ble.channelPreview(name)
    Row(
        Modifier.fillMaxWidth().clickable { onOpen(name) }.bottomHairline().padding(vertical = 13.dp, horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(13.dp),
    ) {
        Box(Modifier.size(42.dp).clip(RoundedCornerShape(12.dp)).background(CcPublic.copy(alpha = 0.14f)).border(1.dp, CcPublic.copy(alpha = 0.35f), RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
            CcText("#", 17, FontWeight.ExtraBold, CcPublicText)
        }
        Column(Modifier.weight(1f)) {
            CcText(s.channelLabel(name), 15, FontWeight.ExtraBold, CcInk)
            CcText(last?.let { "${it.sender.ifBlank { s.someone }}: ${it.body}" } ?: s.channelQuiet, 12, FontWeight.Medium, CcInkMute(0.5f), maxLines = 1)
        }
    }
}

@Composable
private fun ColumnScope.VerifiedTab(ble: BleController, onOpenDm: (String) -> Unit) {
    val s = LocalStrings.current
    Row(Modifier.fillMaxWidth().background(CcVerified.copy(alpha = 0.08f)).border(1.dp, CcVerified.copy(alpha = 0.22f)).padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp)) {
        ShieldBadge(true, 14.dp)
        CcText(s.dmTabBanner, 11, FontWeight.SemiBold, CcVerifiedText)
    }
    val verified = ble.dmPeers
    LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp)) {
        items(verified) { p -> PeerListRow(ble, p, trailing = "DM", onClick = { onOpenDm(p.fp) }) }
        if (verified.isEmpty()) {
            item {
                Box(Modifier.fillMaxWidth().padding(16.dp).dashedBorder(CcInkMute(0.16f), 13.dp).padding(16.dp), contentAlignment = Alignment.Center) {
                    CcText(s.dmEmpty, 12, FontWeight.SemiBold, CcInkMute(0.5f), align = androidx.compose.ui.text.style.TextAlign.Center, lineHeightMul = 1.5)
                }
            }
        }
    }
}

// --- channel view -------------------------------------------------------------------------------

@Composable
private fun ChannelScreen(ble: BleController, name: String, onBack: () -> Unit) {
    val s = LocalStrings.current
    LaunchedEffect(name) { ble.joinChannel(name, silent = true) }
    var draft by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().background(CcBase)) {
        Row(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 14.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            BackIcon(onBack)
            Column(Modifier.weight(1f)) {
                CcText("#${s.channelLabel(name)}", 18, FontWeight.Black, CcInk, letterSpacing = (-0.2))
                CcText(s.channelPublicOwnerless, 10, FontWeight.SemiBold, CcInkMute(0.45f), mono = true)
            }
        }
        PublicBanner()
        MessageList(ble.channel(name), Modifier.weight(1f), pad = 12, gap = 11)
        val cd = ble.channelCooldownFor(name)
        Box(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 12.dp, vertical = 10.dp)) {
            if (cd > 0) {
                CooldownCard(cd, (BleController.CHANNEL_WINDOW_MS / 1000).toInt(), s.slowTitle, s.slowSub.format(cd))
            } else {
                Composer(draft, { draft = it }, { if (ble.sendChannel(name, draft)) draft = "" }, s.composerChannel.format("#${s.channelLabel(name)}"))
            }
        }
    }
}

// --- peer row (used by the DM list) -------------------------------------------------------------

@Composable
private fun PeerListRow(ble: BleController, p: Peer, trailing: String, onClick: () -> Unit) {
    val s = LocalStrings.current
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).bottomHairline().padding(vertical = 11.dp, horizontal = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(13.dp),
    ) {
        if (p.verified) {
            Box(Modifier.size(40.dp).clip(RoundedCornerShape(12.dp)).background(CcVerified.copy(alpha = 0.16f)).border(1.dp, CcVerified.copy(alpha = 0.45f), RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
                CcText(p.name.take(1).uppercase(), 16, FontWeight.ExtraBold, CcVerifiedText)
            }
        } else {
            Box(Modifier.size(40.dp).dashedBorder(CcInkMute(0.28f), 12.dp), contentAlignment = Alignment.Center) {
                CcText("?", 14, FontWeight.Bold, CcInkMute(0.5f), mono = true)
            }
        }
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                if (p.verified) {
                    CcText(p.name, 15, FontWeight.ExtraBold, CcInk)
                    ShieldBadge(true, 13.dp)
                } else {
                    CcText("\"${p.name}\"", 13, FontWeight.SemiBold, CcInkMute(0.72f), mono = true)
                }
            }
            CcText(if (p.verified) s.peerYourPetname else s.peerClaims, 11, FontWeight.Medium, CcInkMute(0.45f), mono = true)
        }
        if (p.verified) {
            Box(Modifier.clip(RoundedCornerShape(9.dp)).background(CcAmber.copy(alpha = 0.16f)).border(1.dp, CcAmber.copy(alpha = 0.4f), RoundedCornerShape(9.dp)).padding(horizontal = 13.dp, vertical = 8.dp)) {
                CcText(trailing, 11, FontWeight.Bold, CcAmberText, upper = true)
            }
        } else {
            Box(Modifier.dashedBorder(CcUnverified.copy(alpha = 0.5f), 9.dp).padding(horizontal = 12.dp, vertical = 8.dp)) {
                CcText(trailing, 11, FontWeight.Bold, CcUnverifiedText, upper = true)
            }
        }
    }
}

// --- DM -----------------------------------------------------------------------------------------

@Composable
private fun DmScreen(ble: BleController, fp: String, onBack: () -> Unit) {
    val s = LocalStrings.current
    val peer = ble.peers.firstOrNull { it.fp == fp }
    val name = peer?.name ?: fp.take(8)
    var draft by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().background(CcBase)) {
        Row(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 14.dp, vertical = 11.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(11.dp)) {
            BackIcon(onBack)
            Box(Modifier.size(36.dp).clip(RoundedCornerShape(11.dp)).background(CcVerified.copy(alpha = 0.16f)).border(1.dp, CcVerified.copy(alpha = 0.45f), RoundedCornerShape(11.dp)), contentAlignment = Alignment.Center) {
                CcText(name.take(1).uppercase(), 14, FontWeight.ExtraBold, CcVerifiedText)
            }
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    CcText(name, 16, FontWeight.ExtraBold, CcInk)
                    if (peer?.verified == true) ShieldBadge(true, 13.dp)
                }
                CcText(if (peer?.verified == true) s.dmVerifiedSubtitle else s.dmNotVerified, 10, FontWeight.Medium, if (peer?.verified == true) CcVerifiedText else CcUnverifiedText, mono = true)
            }
        }
        E2EBanner()
        MessageList(ble.thread(fp), Modifier.weight(1f), pad = 12, gap = 10)
        Box(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 12.dp, vertical = 10.dp)) {
            Composer(draft, { draft = it }, { if (draft.isNotBlank()) { ble.sendDm(fp, draft); draft = "" } }, s.composerDm, leading = {
                Icon(Icons.Filled.Lock, null, tint = CcVerified, modifier = Modifier.size(15.dp))
            })
        }
    }
}

// --- verification flow (E1 / E3 / E4) -----------------------------------------------------------

private enum class VStep { Show, Match, Mismatch }

@Composable
private fun VerifyFlow(ble: BleController, peerFp: String?, startScan: Boolean, onDone: (String?) -> Unit, onCancel: () -> Unit) {
    val s = LocalStrings.current
    var step by remember { mutableStateOf(VStep.Show) }
    var matchedFp by remember { mutableStateOf<String?>(null) }
    var petname by remember { mutableStateOf("") }
    val myFp = remember { ble.myFingerprint() }

    val scanLauncher = rememberLauncherForActivityResult(ScanContract()) { result ->
        val scanned = result.contents?.trim()?.lowercase()
        if (scanned == null) return@rememberLauncherForActivityResult
        val target = peerFp?.lowercase()
        val known = ble.peers.firstOrNull { it.fp.equals(scanned, ignoreCase = true) }?.fp
        when {
            target != null && scanned == target -> { matchedFp = target; petname = ble.peers.firstOrNull { it.fp == target }?.name ?: ""; step = VStep.Match }
            target == null && known != null -> { matchedFp = known; petname = ble.peers.firstOrNull { it.fp == known }?.name ?: ""; step = VStep.Match }
            else -> step = VStep.Mismatch
        }
    }
    fun launchScan() = scanLauncher.launch(
        ScanOptions()
            .setCaptureActivity(chat.cockroach.PortraitCaptureActivity::class.java)
            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
            .setPrompt(s.scanPrompt)
            .setBeepEnabled(false)
            .setOrientationLocked(true),
    )
    // "Scan a QR" entry point: open the camera immediately.
    LaunchedEffect(Unit) { if (startScan) launchScan() }

    Column(Modifier.fillMaxSize().background(CcBase).padding(horizontal = 24.dp)) {
        Row(Modifier.fillMaxWidth().padding(vertical = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            BackIcon(onCancel)
            CcText(if (step == VStep.Mismatch) s.verifyMismatchTitle else s.verifyTitle, 15, FontWeight.ExtraBold, if (step == VStep.Mismatch) CcDestructiveText else CcInk, upper = true, letterSpacing = 0.2)
        }
        when (step) {
            VStep.Show -> Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                Spacer(Modifier.weight(1f))
                CcText(s.verifyShowHeading, 22, FontWeight.ExtraBold, CcInk)
                Spacer(Modifier.height(10.dp))
                CcText(s.verifyShowBody, 13, FontWeight.Medium, CcInkMute(0.55f), align = androidx.compose.ui.text.style.TextAlign.Center, lineHeightMul = 1.5)
                Spacer(Modifier.height(24.dp))
                Box(Modifier.clip(RoundedCornerShape(18.dp)).background(CcInk).padding(20.dp), contentAlignment = Alignment.Center) {
                    QrImage(myFp, Modifier.size(200.dp))
                }
                Spacer(Modifier.height(18.dp))
                SectionLabel(s.safetyNumber, CcInkMute(0.42f))
                Spacer(Modifier.height(9.dp))
                SafetyNumber(ble.safetyWords(myFp))
                Spacer(Modifier.weight(1f))
                CcSecondaryButton(s.scanTheirs, ::launchScan, Modifier.fillMaxWidth())
                Spacer(Modifier.height(22.dp))
            }
            VStep.Match -> Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                Spacer(Modifier.weight(1f))
                Box(Modifier.size(96.dp).clip(RoundedCornerShape(50)).background(CcVerified.copy(alpha = 0.16f)).border(1.dp, CcVerified.copy(alpha = 0.5f), RoundedCornerShape(50)), contentAlignment = Alignment.Center) {
                    Icon(Icons.Filled.Check, null, tint = CcVerified, modifier = Modifier.size(46.dp))
                }
                Spacer(Modifier.height(24.dp))
                CcText(s.matchTitle, 26, FontWeight.Black, CcInk, letterSpacing = (-0.3))
                Spacer(Modifier.height(12.dp))
                CcText(s.matchBody, 14, FontWeight.Medium, CcInkMute(0.6f), align = androidx.compose.ui.text.style.TextAlign.Center, lineHeightMul = 1.5)
                Spacer(Modifier.height(26.dp))
                SectionLabel(s.petnameLabel, modifier = Modifier.fillMaxWidth())
                Spacer(Modifier.height(9.dp))
                NameField(petname, { petname = it }, s.petnamePlaceholder, accent = CcVerified)
                Spacer(Modifier.weight(1f))
                CcPrimaryButton(s.saveOpenDm, {
                    matchedFp?.let { fp -> ble.verify(fp); if (petname.isNotBlank()) ble.setPetname(fp, petname) }
                    onDone(matchedFp)
                }, Modifier.fillMaxWidth())
                Spacer(Modifier.height(22.dp))
            }
            VStep.Mismatch -> Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                Spacer(Modifier.weight(1f))
                Box(Modifier.size(96.dp).clip(RoundedCornerShape(50)).background(CcDestructive.copy(alpha = 0.14f)).border(1.dp, CcDestructive.copy(alpha = 0.5f), RoundedCornerShape(50)), contentAlignment = Alignment.Center) {
                    Icon(Icons.Filled.Warning, null, tint = CcDestructiveText, modifier = Modifier.size(42.dp))
                }
                Spacer(Modifier.height(24.dp))
                CcText("${s.verifyMismatchTitle}.", 25, FontWeight.Black, CcInk, letterSpacing = (-0.3))
                Spacer(Modifier.height(12.dp))
                CcText(s.mismatchBody, 14, FontWeight.Medium, CcInkMute(0.62f), align = androidx.compose.ui.text.style.TextAlign.Center, lineHeightMul = 1.55)
                Spacer(Modifier.weight(1f))
                CcSecondaryButton(s.tryScanAgain, { step = VStep.Show }, Modifier.fillMaxWidth())
                Spacer(Modifier.height(14.dp))
                Box(Modifier.fillMaxWidth().clickable(onClick = onCancel).padding(6.dp), contentAlignment = Alignment.Center) {
                    CcText(s.cancelUnverified, 12, FontWeight.Bold, CcDestructiveText, upper = true)
                }
                Spacer(Modifier.height(22.dp))
            }
        }
    }
}

// --- identity (ME) + panic entry ----------------------------------------------------------------

@Composable
private fun IdentityScreen(ble: BleController, onShowQr: () -> Unit, onScanQr: () -> Unit, onCredits: () -> Unit) {
    val s = LocalStrings.current
    Column(Modifier.fillMaxSize().background(CcBase)) {
        Column(Modifier.fillMaxWidth().bottomHairline().padding(start = 18.dp, top = 15.dp, end = 18.dp, bottom = 12.dp)) {
            CcText(s.identityTitle, 24, FontWeight.Black, CcInk, letterSpacing = (-0.4))
            CcText("${ble.displayName.value} · ${shortId(ble.ephId.value)}", 11, FontWeight.SemiBold, CcInkMute(0.5f), mono = true)
        }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            item {
                IdentityAction(s.showMyQr, s.showMyQrSub, onClick = onShowQr) {
                    Canvas(Modifier.size(20.dp)) {
                        val u = size.minDimension / 5f
                        drawRect(CcInk, Offset(0f, 0f), androidx.compose.ui.geometry.Size(u * 2, u * 2))
                        drawRect(CcInk, Offset(u * 3, 0f), androidx.compose.ui.geometry.Size(u * 2, u * 2))
                        drawRect(CcInk, Offset(0f, u * 3), androidx.compose.ui.geometry.Size(u * 2, u * 2))
                        drawRect(CcInk, Offset(u * 3, u * 3), androidx.compose.ui.geometry.Size(u * 1.4f, u * 1.4f))
                    }
                }
            }
            item { IdentityAction(s.scanQr, s.scanQrSub, onClick = onScanQr) { ScanGlyph(CcInk, 20.dp) } }
            // Language switch.
            item {
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(CcRaised).border(1.dp, CcInkMute(0.1f), RoundedCornerShape(13.dp)).padding(15.dp)) {
                    SectionLabel(s.languageLabel, CcInkMute(0.5f))
                    Spacer(Modifier.height(10.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Lang.entries.forEach { lang ->
                            val on = ble.langCode.value == lang.code
                            Box(
                                Modifier.weight(1f).clip(RoundedCornerShape(9.dp))
                                    .background(if (on) CcAmber.copy(alpha = 0.16f) else CcElevated)
                                    .border(1.dp, if (on) CcAmber.copy(alpha = 0.45f) else CcInkMute(0.12f), RoundedCornerShape(9.dp))
                                    .clickable { ble.setLang(lang.code) }
                                    .padding(vertical = 11.dp),
                                contentAlignment = Alignment.Center,
                            ) { CcText(lang.display, 14, FontWeight.Bold, if (on) CcAmberText else CcInk) }
                        }
                    }
                }
            }
            item { Spacer(Modifier.height(10.dp)); SectionLabel(s.dangerZone, CcDestructiveText, Modifier.padding(start = 2.dp, bottom = 8.dp)) }
            item {
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(CcDestructive.copy(alpha = 0.07f)).border(1.dp, CcDestructive.copy(alpha = 0.4f), RoundedCornerShape(14.dp)).padding(18.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                        Icon(Icons.Filled.Delete, null, tint = CcDestructiveText, modifier = Modifier.size(18.dp))
                        CcText(s.panicWipe, 15, FontWeight.ExtraBold, CcDestructiveText)
                    }
                    Spacer(Modifier.height(8.dp))
                    CcText(s.panicBody, 12, FontWeight.Medium, CcInkMute(0.62f), lineHeightMul = 1.5)
                    Spacer(Modifier.height(14.dp))
                    HoldBarToWipe(onWiped = { ble.panicWipe() })
                }
            }
            // Footer: quiet by design — it should be findable, not compete with the actions above.
            item {
                Spacer(Modifier.height(22.dp))
                Column(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(11.dp)).clickable(onClick = onCredits)
                        .padding(vertical = 12.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    CcText(s.creditsFooter, 13, FontWeight.Bold, CcInkMute(0.7f))
                    Spacer(Modifier.height(2.dp))
                    CcText(s.creditsFooterSub, 11, FontWeight.Medium, CcInkMute(0.4f))
                }
                Spacer(Modifier.height(6.dp))
            }
        }
    }
}

@Composable
private fun IdentityAction(title: String, sub: String, onClick: () -> Unit, icon: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(CcRaised).border(1.dp, CcInkMute(0.1f), RoundedCornerShape(13.dp)).clickable(onClick = onClick).padding(15.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(13.dp)) {
        Box(Modifier.size(20.dp), contentAlignment = Alignment.Center) { icon() }
        Column(Modifier.weight(1f)) {
            CcText(title, 14, FontWeight.ExtraBold, CcInk)
            CcText(sub, 11, FontWeight.Medium, CcInkMute(0.5f))
        }
    }
}

/** Press-and-hold the bar for 5s; the fill slides left→right and completing it wipes everything. */
@Composable
private fun HoldBarToWipe(onWiped: () -> Unit) {
    val s = LocalStrings.current
    var progress by remember { mutableStateOf(0f) }
    var holding by remember { mutableStateOf(false) }
    LaunchedEffect(holding) {
        if (holding) {
            val start = System.currentTimeMillis()
            while (holding) {
                progress = min(1f, (System.currentTimeMillis() - start) / 5000f)
                if (progress >= 1f) { holding = false; onWiped(); break }
                delay(16)
            }
        } else if (progress < 1f) progress = 0f
    }
    Box(
        Modifier.fillMaxWidth().height(54.dp).clip(RoundedCornerShape(12.dp))
            .background(CcDestructive.copy(alpha = 0.14f))
            .border(1.dp, CcDestructive.copy(alpha = 0.6f), RoundedCornerShape(12.dp))
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) { holding = awaitPointerEvent().changes.any { it.pressed } }
                }
            },
    ) {
        // Sliding fill anchored to the left edge.
        Box(Modifier.align(Alignment.CenterStart).fillMaxHeight().fillMaxWidth(progress).background(CcDestructive.copy(alpha = 0.55f)))
        Row(Modifier.align(Alignment.Center), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(Icons.Filled.Delete, null, tint = CcInk, modifier = Modifier.size(18.dp))
            CcText(
                if (progress > 0f) s.keepHolding.format((progress * 100).toInt()) else s.holdToWipe,
                14, FontWeight.ExtraBold, CcInk, upper = true, letterSpacing = 0.3,
            )
        }
    }
}

// --- status (G1) --------------------------------------------------------------------------------

@Composable
private fun StatusScreen(ble: BleController, onBack: () -> Unit) {
    val s = LocalStrings.current
    Column(Modifier.fillMaxSize().background(CcBase)) {
        Row(Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 14.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            BackIcon(onBack)
            Column {
                CcText(s.statusTitle, 20, FontWeight.Black, CcInk, letterSpacing = (-0.3))
                CcText(s.statusSubtitle, 11, FontWeight.SemiBold, CcAmberText, mono = true)
            }
        }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(14.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    StatCard("${ble.peers.size}", s.statPhones, CcAmberText, Modifier.weight(1f))
                    StatCard("${ble.relayedCount.value}", s.statCarried, CcInk, Modifier.weight(1f))
                    StatCard("${ble.verifiedPeers.size}", s.statVerified, CcVerifiedText, Modifier.weight(1f))
                }
            }
            item {
                Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(CcAmber.copy(alpha = 0.1f)).border(1.dp, CcAmber.copy(alpha = 0.3f), RoundedCornerShape(13.dp)).padding(13.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(11.dp)) {
                    Icon(Icons.Filled.Place, null, tint = CcAmberText, modifier = Modifier.size(18.dp))
                    CcText(s.batteryAware, 11, FontWeight.SemiBold, CcAmberText, lineHeightMul = 1.4)
                }
            }
            item { SectionLabel(s.recentActivity, modifier = Modifier.padding(top = 4.dp)) }
            items(ble.log.reversed().take(30)) { line ->
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Box(Modifier.size(7.dp).clip(RoundedCornerShape(50)).background(CcAmber.copy(alpha = 0.6f)))
                    CcText(line, 12, FontWeight.Medium, CcInkMute(0.8f), maxLines = 2)
                }
            }
        }
    }
}

@Composable
private fun StatCard(big: String, label: String, tint: Color, modifier: Modifier) {
    Column(modifier.clip(RoundedCornerShape(13.dp)).background(CcRaised).border(1.dp, CcInkMute(0.1f), RoundedCornerShape(13.dp)).padding(14.dp)) {
        CcText(big, 26, FontWeight.Black, tint)
        Spacer(Modifier.height(5.dp))
        CcText(label, 11, FontWeight.SemiBold, CcInkMute(0.5f), mono = true, lineHeightMul = 1.3)
    }
}

// --- shared -------------------------------------------------------------------------------------

/** A message list that keeps the newest message in view as the thread grows. */
@Composable
private fun MessageList(messages: List<ChatMessage>, modifier: Modifier = Modifier, pad: Int = 12, gap: Int = 11) {
    val state = rememberLazyListState()
    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) state.animateScrollToItem(messages.size - 1)
    }
    LazyColumn(
        modifier.fillMaxWidth(),
        state = state,
        contentPadding = PaddingValues(pad.dp),
        verticalArrangement = Arrangement.spacedBy(gap.dp),
    ) { items(messages) { MessageBubble(it) } }
}

@Composable
internal fun BackIcon(onBack: () -> Unit) {
    Box(Modifier.clip(RoundedCornerShape(50)).clickable(onClick = onBack).padding(4.dp)) {
        Icon(Icons.AutoMirrored.Filled.ArrowBack, "back", tint = CcInkMute(0.7f), modifier = Modifier.size(22.dp))
    }
}
