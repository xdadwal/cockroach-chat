@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package chat.cockroach.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// --- text helper ------------------------------------------------------------------------------

@Composable
fun CcText(
    text: String,
    size: Int,
    weight: FontWeight = FontWeight.Medium,
    color: Color = CcInk,
    mono: Boolean = false,
    upper: Boolean = false,
    letterSpacing: Double = 0.0,
    lineHeightMul: Double = 1.25,
    align: TextAlign = TextAlign.Start,
    maxLines: Int = Int.MAX_VALUE,
    modifier: Modifier = Modifier,
) {
    val scale = LocalFontScale.current
    Text(
        text = if (upper) text.uppercase() else text,
        modifier = modifier,
        maxLines = maxLines,
        style = TextStyle(
            fontFamily = if (mono) JetMono else Archivo,
            fontWeight = weight,
            fontSize = (size * scale).sp,
            lineHeight = (size * lineHeightMul * scale).sp,
            letterSpacing = letterSpacing.sp,
            color = color,
            textAlign = align,
        ),
    )
}

// --- dashed border --------------------------------------------------------------------------

fun Modifier.dashedBorder(color: Color, corner: Dp, width: Dp = 1.dp): Modifier = drawBehind {
    val stroke = Stroke(
        width = width.toPx(),
        pathEffect = PathEffect.dashPathEffect(floatArrayOf(width.toPx() * 2.4f, width.toPx() * 2.2f)),
    )
    val inset = width.toPx() / 2
    drawRoundRect(
        color = color,
        topLeft = Offset(inset, inset),
        size = Size(size.width - width.toPx(), size.height - width.toPx()),
        cornerRadius = CornerRadius(corner.toPx()),
        style = stroke,
    )
}

// --- mesh dot + chip --------------------------------------------------------------------------

enum class MeshState { Live, LowPower, Off, Scanning }

@Composable
private fun PulsingDot(color: Color, size: Dp = 9.dp) {
    val t = rememberInfiniteTransition(label = "pulse")
    val scale by t.animateFloat(
        0.6f, 2.4f,
        infiniteRepeatable(tween(1800), RepeatMode.Restart), label = "s",
    )
    val alpha by t.animateFloat(
        0.7f, 0f,
        infiniteRepeatable(tween(1800), RepeatMode.Restart), label = "a",
    )
    Box(Modifier.size(size), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(size)) {
            drawCircle(color.copy(alpha = alpha), radius = (this.size.minDimension / 2f) * scale)
            drawCircle(color, radius = this.size.minDimension / 2f)
        }
    }
}

@Composable
fun MeshChip(state: MeshState, modifier: Modifier = Modifier) {
    val (tint, border) = when (state) {
        MeshState.Live -> CcAmberText to CcAmber.copy(alpha = 0.4f)
        MeshState.Scanning -> CcAmberText to CcAmber.copy(alpha = 0.35f)
        MeshState.LowPower -> CcInkMute(0.85f) to CcInkMute(0.18f)
        MeshState.Off -> CcWarningText to CcWarning.copy(alpha = 0.45f)
    }
    val label = LocalStrings.current.meshLabel(state)
    val bg = if (state == MeshState.Off || state == MeshState.LowPower) CcRaised else CcAmber.copy(alpha = 0.14f)
    Row(
        modifier
            .clip(RoundedCornerShape(20.dp))
            .background(bg)
            .border(1.dp, border, RoundedCornerShape(20.dp))
            .padding(horizontal = 11.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        when (state) {
            MeshState.Live, MeshState.Scanning -> PulsingDot(CcAmber, 8.dp)
            MeshState.LowPower -> Box(Modifier.size(8.dp).clip(RoundedCornerShape(50)).background(CcAmber.copy(alpha = 0.45f)))
            MeshState.Off -> Box(Modifier.size(8.dp).border(2.dp, CcWarning, RoundedCornerShape(50)))
        }
        CcText(label, 11, FontWeight.Bold, tint, mono = true, letterSpacing = 0.5)
    }
}

@Composable
fun ShortIdChip(shortId: String, modifier: Modifier = Modifier) {
    Row(
        modifier
            .clip(RoundedCornerShape(20.dp))
            .background(CcRaised)
            .border(1.dp, CcInkMute(0.14f), RoundedCornerShape(20.dp))
            .padding(horizontal = 9.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Canvas(Modifier.size(12.dp)) {
            val u = size.minDimension / 5f
            val c = CcInk
            drawRect(c, Offset(0f, 0f), Size(u * 2, u * 2))
            drawRect(c, Offset(u * 3, 0f), Size(u * 2, u * 2))
            drawRect(c, Offset(0f, u * 3), Size(u * 2, u * 2))
            drawRect(c, Offset(u * 3, u * 3), Size(u * 1.2f, u * 1.2f))
        }
        CcText(shortId, 11, FontWeight.SemiBold, CcInkMute(0.6f), mono = true)
    }
}

// --- broadcast glyph --------------------------------------------------------------------------

@Composable
fun BroadcastGlyph(color: Color, size: Dp = 15.dp) {
    Canvas(Modifier.size(size)) {
        val cx = this.size.width / 2f
        val cy = this.size.height / 2f
        drawCircle(color, radius = this.size.minDimension * 0.11f, center = Offset(cx, cy))
        for (r in listOf(0.28f, 0.44f)) {
            drawCircle(
                color, radius = this.size.minDimension * r, center = Offset(cx, cy),
                style = Stroke(width = this.size.minDimension * 0.07f),
            )
        }
    }
}

// --- scan-frame glyph -------------------------------------------------------------------------

@Composable
fun ScanGlyph(color: Color, size: Dp = 20.dp) {
    Canvas(Modifier.size(size)) {
        val d = this.size.minDimension
        val p = d * 0.08f
        val l = d * 0.3f
        val w = d * 0.09f
        val cap = androidx.compose.ui.graphics.StrokeCap.Round
        fun corner(cx: Float, cy: Float, dx: Float, dy: Float) {
            drawLine(color, Offset(cx, cy), Offset(cx + dx, cy), w, cap = cap)
            drawLine(color, Offset(cx, cy), Offset(cx, cy + dy), w, cap = cap)
        }
        corner(p, p, l, l)
        corner(d - p, p, -l, l)
        corner(p, d - p, l, -l)
        corner(d - p, d - p, -l, -l)
    }
}

// --- trust badge (inline, message header) -----------------------------------------------------

@Composable
fun ShieldBadge(verified: Boolean, size: Dp = 13.dp) {
    if (verified) {
        Box(Modifier.size(size).clip(RoundedCornerShape(50)).background(CcVerified), contentAlignment = Alignment.Center) {
            Icon(Icons.Filled.Check, null, tint = CcOnAmber, modifier = Modifier.size(size * 0.72f))
        }
    } else {
        Box(Modifier.size(size).dashedBorder(CcUnverified, size / 2, 1.2.dp))
    }
}

// --- buttons ----------------------------------------------------------------------------------

@Composable
fun CcPrimaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, color: Color = CcAmber) {
    Box(
        modifier
            .clip(RoundedCornerShape(13.dp))
            .background(if (enabled) color else color.copy(alpha = 0.3f))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(vertical = 16.dp),
        contentAlignment = Alignment.Center,
    ) { CcText(text, 16, FontWeight.ExtraBold, CcOnAmber, upper = true, letterSpacing = 0.4) }
}

@Composable
fun CcSecondaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Box(
        modifier
            .clip(RoundedCornerShape(13.dp))
            .border(1.dp, CcInkMute(0.3f), RoundedCornerShape(13.dp))
            .clickable(onClick = onClick)
            .padding(vertical = 14.dp),
        contentAlignment = Alignment.Center,
    ) { CcText(text, 15, FontWeight.Bold, CcInk, upper = true, letterSpacing = 0.4) }
}

// --- feed tabs (Announce / Nearby / Verified) -------------------------------------------------

enum class FeedTab(val label: String, val tint: Color, val bg: Color, val border: Color) {
    Announce("Announcement", CcPublicText, CcPublic.copy(alpha = 0.16f), CcPublic.copy(alpha = 0.45f)),
    Nearby("Channels", CcInk, CcElevated, CcInkMute(0.2f)),
    Verified("Verified", CcVerifiedText, CcVerified.copy(alpha = 0.16f), CcVerified.copy(alpha = 0.45f)),
}

@Composable
fun FeedTabs(selected: FeedTab, onSelect: (FeedTab) -> Unit, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 11.dp), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        FeedTab.values().forEach { tab ->
            val on = tab == selected
            Box(
                Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(9.dp))
                    .then(if (on) Modifier.background(tab.bg).border(1.dp, tab.border, RoundedCornerShape(9.dp)) else Modifier)
                    .clickable { onSelect(tab) }
                    .padding(vertical = 9.dp),
                contentAlignment = Alignment.Center,
            ) { CcText(LocalStrings.current.feedLabel(tab), 12, FontWeight.Bold, if (on) tab.tint else CcInkMute(0.5f), upper = true, letterSpacing = 0.1, maxLines = 1) }
        }
    }
}

// --- banners ----------------------------------------------------------------------------------

@Composable
fun PublicBanner() {
    Row(
        Modifier.fillMaxWidth().background(CcPublic.copy(alpha = 0.1f))
            .border(1.dp, CcPublic.copy(alpha = 0.25f)).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp),
    ) {
        BroadcastGlyph(CcPublicText, 15.dp)
        CcText(LocalStrings.current.publicBanner, 11, FontWeight.SemiBold, CcPublicText, lineHeightMul = 1.35)
    }
}

@Composable
fun E2EBanner() {
    Row(
        Modifier.fillMaxWidth().background(CcVerified.copy(alpha = 0.08f))
            .border(1.dp, CcVerified.copy(alpha = 0.22f)).padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(9.dp),
    ) {
        Icon(Icons.Filled.Lock, null, tint = CcVerifiedText, modifier = Modifier.size(15.dp).padding(top = 1.dp))
        CcText(LocalStrings.current.e2eBanner, 11, FontWeight.SemiBold, CcVerifiedText, lineHeightMul = 1.35)
    }
}

// --- message bubble ---------------------------------------------------------------------------

@Composable
fun MessageBubble(msg: chat.cockroach.ChatMessage, modifier: Modifier = Modifier) {
    val s = LocalStrings.current
    Column(modifier.fillMaxWidth(), horizontalAlignment = if (msg.mine) Alignment.End else Alignment.Start) {
        if (msg.mine) {
            Column(
                Modifier
                    .fillMaxWidth(0.84f)
                    .clip(RoundedCornerShape(14.dp, 4.dp, 14.dp, 14.dp))
                    .background(CcAmber.copy(alpha = 0.16f))
                    .border(1.dp, CcAmber.copy(alpha = 0.35f), RoundedCornerShape(14.dp, 4.dp, 14.dp, 14.dp))
                    .padding(horizontal = 13.dp, vertical = 11.dp),
                horizontalAlignment = Alignment.End,
            ) {
                CcText(msg.body, 14, FontWeight.Medium, CcInk, lineHeightMul = 1.45, modifier = Modifier.fillMaxWidth())
                Spacer(Modifier.height(6.dp))
                CcText("${clock(msg.timestampMs)} · ${s.msgYouSigned}", 9, FontWeight.Medium, CcInkMute(0.45f), mono = true)
            }
        } else {
            val verified = msg.verified
            val bar = if (verified) CcVerified else CcUnverified
            Column(
                Modifier
                    .fillMaxWidth(0.86f)
                    .clip(RoundedCornerShape(4.dp, 14.dp, 14.dp, 14.dp))
                    .background(if (verified) CcElevated else CcCard)
                    .drawBehind {
                        val w = 2.dp.toPx()
                        if (verified) {
                            drawRect(SolidColor(bar), size = Size(w, size.height))
                        } else {
                            drawLine(
                                bar, Offset(w / 2, 0f), Offset(w / 2, size.height), w,
                                pathEffect = PathEffect.dashPathEffect(floatArrayOf(w * 2f, w * 1.6f)),
                            )
                        }
                    }
                    .padding(start = 14.dp, top = 12.dp, end = 14.dp, bottom = 12.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    ShieldBadge(verified, 12.dp)
                    if (verified) {
                        CcText(msg.sender.ifBlank { s.someone }, 13, FontWeight.ExtraBold, CcInk)
                        CcText(s.badgeVerified, 9, FontWeight.SemiBold, CcVerifiedText, mono = true)
                    } else {
                        CcText("\"${msg.sender.ifBlank { s.unknown }}\"", 13, FontWeight.SemiBold, CcInkMute(0.6f), mono = true)
                        CcText(s.badgeUnverified, 9, FontWeight.SemiBold, CcUnverifiedText, mono = true)
                    }
                }
                Spacer(Modifier.height(6.dp))
                CcText(msg.body, 14, FontWeight.Medium, CcInk.copy(alpha = if (verified) 0.9f else 0.82f), lineHeightMul = 1.45)
                Spacer(Modifier.height(7.dp))
                CcText(
                    clock(msg.timestampMs) + if (!verified) " · ${s.msgCantConfirm}" else "",
                    9, FontWeight.Medium, CcInkMute(0.4f), mono = true,
                )
            }
        }
    }
}

private fun clock(ts: Long): String {
    if (ts <= 0L) return "now"
    val d = java.util.Date(ts)
    return java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(d)
}

// --- composer ---------------------------------------------------------------------------------

@Composable
fun Composer(
    value: String,
    onValue: (String) -> Unit,
    onSend: () -> Unit,
    placeholder: String,
    accent: Color = CcAmber,
    enabled: Boolean = true,
    modifier: Modifier = Modifier,
    leading: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(13.dp))
            .background(CcRaised)
            .border(1.dp, CcInkMute(0.14f), RoundedCornerShape(13.dp))
            .padding(start = 15.dp, top = 8.dp, end = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (leading != null) leading()
        Box(Modifier.weight(1f).padding(vertical = 6.dp), contentAlignment = Alignment.CenterStart) {
            if (value.isEmpty()) CcText(placeholder, 13, FontWeight.Medium, CcInkMute(0.4f))
            BasicTextField(
                value = value,
                onValueChange = onValue,
                singleLine = false,
                maxLines = 5,
                enabled = enabled,
                textStyle = TextStyle(fontFamily = Archivo, fontWeight = FontWeight.Medium, fontSize = 14.sp, color = CcInk, lineHeight = 19.sp),
                cursorBrush = SolidColor(accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Box(
            Modifier.size(40.dp).clip(RoundedCornerShape(10.dp))
                .background(if (enabled) accent else accent.copy(alpha = 0.3f))
                .clickable(enabled = enabled, onClick = onSend),
            contentAlignment = Alignment.Center,
        ) { Icon(Icons.Filled.Send, "send", tint = CcOnAmber, modifier = Modifier.size(18.dp)) }
    }
}

// --- section label ----------------------------------------------------------------------------

@Composable
fun SectionLabel(text: String, color: Color = CcInkMute(0.42f), modifier: Modifier = Modifier) {
    CcText(text, 11, FontWeight.SemiBold, color, mono = true, upper = true, letterSpacing = 1.1, modifier = modifier)
}

// --- safety number ----------------------------------------------------------------------------

@Composable
fun SafetyNumber(words: List<String>, modifier: Modifier = Modifier) {
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        words.forEach { w ->
            Box(
                Modifier.weight(1f).clip(RoundedCornerShape(9.dp))
                    .background(CcVerified.copy(alpha = 0.1f))
                    .border(1.dp, CcVerified.copy(alpha = 0.3f), RoundedCornerShape(9.dp))
                    .padding(vertical = 10.dp),
                contentAlignment = Alignment.Center,
            ) { CcText(w, 13, FontWeight.Bold, CcVerifiedText, mono = true) }
        }
    }
}

