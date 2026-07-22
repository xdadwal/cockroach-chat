@file:OptIn(androidx.compose.ui.text.ExperimentalTextApi::class)

package chat.cockroach.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import chat.cockroach.R

// --- palette (from the design system: warm near-black surfaces, paper-cream ink) --------------

val CcBase = Color(0xFF0D0C0A)      // base surface
val CcRaised = Color(0xFF16130E)    // raised
val CcCard = Color(0xFF1C1813)      // card
val CcElevated = Color(0xFF241F18)  // elevated
val CcNav = Color(0xFF100E0B)       // bottom nav / ticker
val CcScan = Color(0xFF080805)      // camera/scan backdrop
val CcInk = Color(0xFFF4EBD7)       // ink
val CcOnAmber = Color(0xFF17130E)   // ink on amber fills

// semantic family — one chroma/lightness family, never a rainbow (oklch → sRGB)
val CcVerified = Color(0xFF76CF8A)
val CcVerifiedText = Color(0xFF90E9A3)
val CcVerifiedBright = Color(0xFF96F0AA)
val CcUnverified = Color(0xFF8B9AAB)
val CcUnverifiedText = Color(0xFFB0C0D1)
val CcPublic = Color(0xFF52C1E1)
val CcPublicText = Color(0xFF71DFFF)
val CcAmber = Color(0xFFF4B85B)
val CcAmberText = Color(0xFFFFC568)
val CcWarning = Color(0xFFFA934E)
val CcWarningText = Color(0xFFFFAC69)
val CcDestructive = Color(0xFFF1453F)
val CcDestructiveText = Color(0xFFFF8A7C)

fun CcInkMute(alpha: Float): Color = CcInk.copy(alpha = alpha)

// --- type: Archivo carries voice, JetBrains Mono carries fact (variable fonts) ----------------

private fun archivo(weight: Int) =
    Font(R.font.archivo, FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight)))

private fun mono(weight: Int) =
    Font(R.font.jetbrainsmono, FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight)))

val Archivo = FontFamily(archivo(400), archivo(500), archivo(600), archivo(700), archivo(800), archivo(900))
val JetMono = FontFamily(mono(400), mono(500), mono(600), mono(700))

private val ccTypography = Typography(
    bodyLarge = TextStyle(fontFamily = Archivo, fontWeight = FontWeight.Medium, fontSize = 15.sp),
    bodyMedium = TextStyle(fontFamily = Archivo, fontWeight = FontWeight.Medium, fontSize = 13.sp),
    labelLarge = TextStyle(fontFamily = Archivo, fontWeight = FontWeight.Bold, fontSize = 14.sp),
    titleLarge = TextStyle(fontFamily = Archivo, fontWeight = FontWeight.Black, fontSize = 22.sp),
)

private val ccColors = darkColorScheme(
    primary = CcAmber,
    onPrimary = CcOnAmber,
    background = CcBase,
    onBackground = CcInk,
    surface = CcRaised,
    onSurface = CcInk,
    surfaceVariant = CcCard,
    onSurfaceVariant = CcInkMute(0.6f),
    error = CcDestructive,
)

@Composable
fun CockroachTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = ccColors, typography = ccTypography, content = content)
}
