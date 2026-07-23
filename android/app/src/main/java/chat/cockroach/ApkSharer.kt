package chat.cockroach

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.FileProvider
import java.io.File
import java.security.MessageDigest

/**
 * Shares the installed APK itself, so the app can spread phone-to-phone with no internet —
 * the same distribution channel the mesh already assumes (threat-model: the download channel
 * is centralised and blockable; this is the workaround).
 *
 * The receiver has stock Android and nothing else, so the transfer must ride an OS-level
 * channel: we stage a copy of our own APK in the cache dir and hand it to the system share
 * sheet (Bluetooth, Quick Share, whatever is installed).
 */
object ApkSharer {

    /** Filename the peer sees. Keep the .apk extension — stock Android installs it on tap. */
    const val SHARED_APK_NAME = "cockroach-chat.apk"

    private const val STAGE_DIR = "apk"

    /**
     * Display form of a certificate digest: lowercase hex in groups of four, matching what
     * `apksigner verify --print-certs` prints (modulo the spaces, which eyes skip over).
     */
    fun formatFingerprint(digest: ByteArray): String =
        digest.joinToString("") { "%02x".format(it) }.chunked(4).joinToString(" ")

    /**
     * SHA-256 of the signing certificate of the *installed* app — computed live, never
     * hardcoded, so a tampered copy shows its own (different) fingerprint rather than ours.
     * Null only if the package manager lookup fails (should not happen for our own package).
     */
    fun signingCertSha256(context: Context): String? = runCatching {
        val pm = context.packageManager
        val cert: ByteArray = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val info = pm.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
            info.signingInfo.apkContentsSigners.firstOrNull()?.toByteArray()
        } else {
            @Suppress("DEPRECATION")
            val info = pm.getPackageInfo(context.packageName, PackageManager.GET_SIGNATURES)
            @Suppress("DEPRECATION")
            info.signatures.firstOrNull()?.toByteArray()
        } ?: return null
        formatFingerprint(MessageDigest.getInstance("SHA-256").digest(cert))
    }.getOrNull()

    /**
     * True when the install is split into multiple APKs (app-bundle style). We only ship
     * universal APKs, but a repacked copy could be split — sharing just the base would then
     * produce a file that won't install, and the UI must say so instead of pretending.
     */
    fun hasSplits(context: Context): Boolean =
        !context.applicationInfo.splitSourceDirs.isNullOrEmpty()

    /**
     * Copies the installed APK into the cache dir under a friendly name. FileProvider can't
     * serve `/data/app/...` directly, and "cockroach-chat.apk" beats "base.apk" on the
     * receiving phone. Re-copies only when the source changed (app updated). Call off the
     * main thread; returns null if the copy fails (e.g. disk full).
     */
    fun stageApk(context: Context): File? = runCatching {
        val source = File(context.applicationInfo.sourceDir)
        val staged = File(File(context.cacheDir, STAGE_DIR).apply { mkdirs() }, SHARED_APK_NAME)
        if (!staged.exists() || staged.length() != source.length() ||
            staged.lastModified() < source.lastModified()
        ) {
            source.copyTo(staged, overwrite = true)
        }
        staged
    }.getOrNull()

    /** Share-sheet intent for a staged APK. `title` is the chooser title, already localized. */
    fun shareIntent(context: Context, staged: File, title: String): Intent {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", staged)
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "application/vnd.android.package-archive"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        return Intent.createChooser(send, title)
    }
}
