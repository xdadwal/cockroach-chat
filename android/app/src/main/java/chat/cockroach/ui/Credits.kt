package chat.cockroach.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/**
 * One credited third-party resource.
 *
 * [name], [by] and [license] are proper nouns and legal identifiers — never translated, for the
 * same reason display names and fingerprints aren't (see [Strings]). [role] is kept to a short
 * technical phrase so it reads the same in either language.
 */
data class Credit(val name: String, val role: String, val by: String, val license: String)

// The lists below are maintained by hand and cover what we bundle directly. They are not the full
// transitive tree — 145 crates deep, most of them build-time — which lives in NOTICE.md in the
// source repository. If you add a dependency that ships in the APK, add it here too.

/** Typefaces. Both used unmodified; their OFL texts ship inside the APK at assets/licenses/. */
private val TYPE = listOf(
    Credit("Archivo", "Interface typeface", "Omnibus-Type · Hector Gatti", "SIL OFL 1.1"),
    Credit("JetBrains Mono", "Monospace typeface", "JetBrains · Philipp Nurullin, Konstantin Bulenkov", "SIL OFL 1.1"),
)

/** The security of this app rests on these. We hand-roll no primitives. */
private val CRYPTO = listOf(
    Credit("snow", "Noise XX protocol", "Jake McGinty & contributors", "MIT / Apache-2.0"),
    Credit("ed25519-dalek", "Signatures", "dalek-cryptography", "BSD-3-Clause"),
    Credit("x25519-dalek", "Key agreement", "dalek-cryptography", "BSD-3-Clause"),
    Credit("ChaCha20-Poly1305, SHA-2", "Ciphers and hashing", "RustCrypto", "MIT / Apache-2.0"),
    Credit("zeroize", "Erasing key material", "RustCrypto", "MIT / Apache-2.0"),
    Credit("SQLCipher", "Encrypted database", "Zetetic LLC", "BSD-style"),
    Credit("OpenSSL", "Crypto backend for SQLCipher", "The OpenSSL Project", "Apache-2.0"),
)

/** The shared Rust core and its bridge to Kotlin. */
private val CORE = listOf(
    Credit("UniFFI", "Rust ↔ Kotlin bindings", "Mozilla", "MPL-2.0"),
    Credit("JNA", "Native library loading", "Java Native Access project", "Apache-2.0"),
    Credit("rusqlite", "SQLite bindings", "The rusqlite developers", "MIT"),
    Credit("lz4_flex", "Payload compression", "Pascal Seitz", "MIT"),
    Credit("rand, getrandom", "Randomness", "The Rand Project", "MIT / Apache-2.0"),
    Credit("thiserror", "Error types", "David Tolnay", "MIT / Apache-2.0"),
)

/** Everything you can see and touch. */
private val ANDROID = listOf(
    Credit("Jetpack Compose", "UI toolkit", "Google / AOSP", "Apache-2.0"),
    Credit("Material Symbols", "Interface icons", "Google", "Apache-2.0"),
    Credit("AndroidX", "Activity, lifecycle", "Google / AOSP", "Apache-2.0"),
    Credit("Kotlin & kotlinx.coroutines", "Language and concurrency", "JetBrains", "Apache-2.0"),
    Credit("ZXing + zxing-android-embedded", "QR generation and scanning", "ZXing Authors · JourneyApps", "Apache-2.0"),
)

/** Not shipped in the app, but this wouldn't be trustworthy without them. */
private val TOOLING = listOf(
    Credit("cargo-fuzz / libFuzzer", "Fuzzing the parsers", "rust-fuzz · LLVM", "MIT / Apache-2.0"),
    Credit("cargo-deny", "Dependency and licence auditing", "Embark Studios", "MIT / Apache-2.0"),
    Credit("JUnit", "Unit tests", "The JUnit Team", "EPL-1.0"),
)

/**
 * Credits — a scrollable acknowledgement of everything this app is built on.
 *
 * Reached from the footer of the Me screen. The bundled font licences also ship verbatim inside
 * the APK under `assets/licenses/`, which is what the OFL requires; this screen is the readable
 * form of the same acknowledgement.
 */
@Composable
fun CreditsScreen(onBack: () -> Unit) {
    val s = LocalStrings.current
    val sections = listOf(
        s.creditsType to TYPE,
        s.creditsCrypto to CRYPTO,
        s.creditsCore to CORE,
        s.creditsAndroid to ANDROID,
        s.creditsTooling to TOOLING,
    )
    Column(Modifier.fillMaxSize().background(CcBase)) {
        // Same detail-screen header shape as StatusScreen / ChannelScreen.
        Row(
            Modifier.fillMaxWidth().bottomHairline().padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            BackIcon(onBack)
            Column {
                CcText(s.creditsTitle, 20, FontWeight.Black, CcInk, letterSpacing = (-0.3))
                CcText(s.creditsSubtitle, 11, FontWeight.SemiBold, CcAmberText, mono = true)
            }
        }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(14.dp)) {
            item {
                CcText(s.creditsIntro, 13, FontWeight.Medium, CcInkMute(0.62f), lineHeightMul = 1.55)
                Spacer(Modifier.height(6.dp))
            }
            sections.forEach { (title, entries) ->
                item {
                    Spacer(Modifier.height(14.dp))
                    SectionLabel(title, CcAmberText, Modifier.padding(start = 2.dp, bottom = 8.dp))
                }
                items(entries.size) { i -> CreditRow(entries[i]) }
            }
            item {
                Spacer(Modifier.height(18.dp))
                Column(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp))
                        .background(CcRaised).border(1.dp, CcInkMute(0.1f), RoundedCornerShape(13.dp))
                        .padding(15.dp),
                ) {
                    CcText(s.creditsThanks, 13, FontWeight.Medium, CcInkMute(0.72f), lineHeightMul = 1.55)
                    Spacer(Modifier.height(10.dp))
                    CcText(s.creditsFull, 11, FontWeight.Medium, CcInkMute(0.45f), lineHeightMul = 1.5)
                }
                Spacer(Modifier.height(20.dp))
            }
        }
    }
}

@Composable
private fun CreditRow(c: Credit) {
    Column(
        Modifier.fillMaxWidth().padding(bottom = 8.dp).clip(RoundedCornerShape(12.dp))
            .background(CcRaised).border(1.dp, CcInkMute(0.08f), RoundedCornerShape(12.dp))
            .padding(horizontal = 14.dp, vertical = 12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            CcText(c.name, 14, FontWeight.ExtraBold, CcInk, modifier = Modifier.weight(1f))
            CcText(c.license, 10, FontWeight.SemiBold, CcInkMute(0.42f), mono = true)
        }
        Spacer(Modifier.height(3.dp))
        CcText(c.role, 12, FontWeight.Medium, CcInkMute(0.55f))
        Spacer(Modifier.height(5.dp))
        CcText(c.by, 11, FontWeight.Medium, CcAmberText.copy(alpha = 0.75f))
    }
}
