package chat.cockroach

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The fingerprint a user reads off the share screen must match what `apksigner verify
 * --print-certs` prints (lowercase hex), because SECURITY.md tells people to compare the two by
 * eye. Grouping is display-only and must never drop or reorder a byte.
 */
class ApkSharerTest {

    @Test
    fun `formats digest as lowercase hex in groups of four`() {
        val digest = byteArrayOf(
            0xAB.toByte(), 0xCD.toByte(), 0x00, 0x12,
            0xFF.toByte(), 0x7F, 0x80.toByte(), 0x01,
        )
        assertEquals("abcd 0012 ff7f 8001", ApkSharer.formatFingerprint(digest))
    }

    @Test
    fun `sha256 digest length formats to eight groups per half`() {
        // A SHA-256 digest is 32 bytes → 64 hex chars → 16 groups of 4, 15 separating spaces.
        val formatted = ApkSharer.formatFingerprint(ByteArray(32))
        assertEquals(64 + 15, formatted.length)
        assertEquals("0000", formatted.split(" ").first())
        assertEquals(16, formatted.split(" ").size)
    }

    @Test
    fun `stripping spaces recovers the exact apksigner output`() {
        val digest = ByteArray(32) { it.toByte() }
        val apksignerStyle = digest.joinToString("") { "%02x".format(it) }
        assertEquals(apksignerStyle, ApkSharer.formatFingerprint(digest).replace(" ", ""))
    }
}
