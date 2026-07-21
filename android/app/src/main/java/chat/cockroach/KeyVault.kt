package chat.cockroach

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** The two long-lived secrets: the SQLCipher database key and the identity seed. */
data class Secrets(val dbKey: ByteArray, val seed: Long)

/**
 * Generates the app's long-lived secrets once and protects them with a **hardware-backed**
 * Android Keystore key (AES-GCM). Only the wrapped (encrypted) blob is written to disk — the
 * plaintext DB key never touches storage, so a seized device can't read the SQLCipher database.
 * Destroying the keystore key (via [wipe]) makes the database unrecoverable ciphertext.
 */
object KeyVault {
    private const val KEYSTORE = "AndroidKeyStore"
    private const val ALIAS = "cockroach_kek"
    private const val VAULT = "vault.bin"
    const val DB_NAME = "mesh.db"

    fun loadOrCreate(context: Context): Secrets {
        val file = File(context.filesDir, VAULT)
        return if (file.exists()) runCatching { load(file) }.getOrElse { create(file) } else create(file)
    }

    /** Cryptographic erasure: destroy the wrapping key and delete the encrypted database. */
    fun wipe(context: Context) {
        listOf(VAULT, DB_NAME, "$DB_NAME-wal", "$DB_NAME-shm").forEach {
            File(context.filesDir, it).delete()
        }
        runCatching {
            KeyStore.getInstance(KEYSTORE).apply { load(null) }.deleteEntry(ALIAS)
        }
    }

    private fun kek(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(
                ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return gen.generateKey()
    }

    private fun create(file: File): Secrets {
        val rnd = SecureRandom()
        val dbKey = ByteArray(32).also { rnd.nextBytes(it) }
        val seedBytes = ByteArray(8).also { rnd.nextBytes(it) }

        val plaintext = dbKey + seedBytes // 40 bytes
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, kek())
        val iv = cipher.iv
        val ct = cipher.doFinal(plaintext)
        file.outputStream().use { it.write(iv.size); it.write(iv); it.write(ct) }

        return Secrets(dbKey, seedFromBytes(seedBytes))
    }

    private fun load(file: File): Secrets {
        val bytes = file.readBytes()
        val ivLen = bytes[0].toInt() and 0xff
        val iv = bytes.copyOfRange(1, 1 + ivLen)
        val ct = bytes.copyOfRange(1 + ivLen, bytes.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, kek(), GCMParameterSpec(128, iv))
        val plaintext = cipher.doFinal(ct)
        return Secrets(plaintext.copyOfRange(0, 32), seedFromBytes(plaintext.copyOfRange(32, 40)))
    }

    private fun seedFromBytes(b: ByteArray): Long {
        var v = 0L
        for (i in 0 until 8) v = v or ((b[i].toLong() and 0xff) shl (8 * i))
        return v
    }
}
