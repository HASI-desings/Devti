package app.devti.android.continuity

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.core.content.edit
import java.io.File
import java.util.UUID

/**
 * Files in and out of the phone.
 *
 * Out: other apps hand Devti `content://` URIs that only stay readable briefly, while a
 * transfer may pause and resume minutes later. So each file is first copied into the
 * app's cache (`cache/outgoing/<random>/<name>`) and sent from there; the copy is
 * deleted when the transfer finishes or is cancelled.
 *
 * In: Devti Core writes received files into app storage and verifies them (SHA-256).
 * Finished files are then moved to the shared **Downloads/Devti** folder (MediaStore on
 * Android 10+, the public folder on 8–9 when storage permission was granted), so they
 * show up in Files and Gallery like any download.
 */
class Files(private val context: Context) {
    private val prefs = context.getSharedPreferences("devti_files", Context.MODE_PRIVATE)
    val incomingDir: File = File(context.getExternalFilesDir(null) ?: context.filesDir, "incoming").apply { mkdirs() }
    private val outgoingRoot = File(context.cacheDir, "outgoing")

    /** Copies a shared/picked document into the cache; returns the path to send. */
    fun stage(uri: Uri): File {
        val name = Pure.safeName(displayName(uri))
        val dir = File(outgoingRoot, UUID.randomUUID().toString()).apply { mkdirs() }
        val dest = File(dir, name)
        val input = context.contentResolver.openInputStream(uri) ?: error("couldn't open that file")
        input.use { src -> dest.outputStream().use { src.copyTo(it, 256 * 1024) } }
        return dest
    }

    /** Removes a staged copy once its transfer is over. */
    fun unstage(path: String) {
        val f = File(path)
        if (f.canonicalPath.startsWith(outgoingRoot.canonicalPath + File.separator)) {
            f.delete()
            f.parentFile?.delete()
        }
    }

    /** Deletes staged copies left behind by a crash (older than a day). */
    fun cleanStale() {
        val cutoff = System.currentTimeMillis() - 24 * 3600_000L
        outgoingRoot.listFiles()?.filter { it.lastModified() < cutoff }?.forEach { it.deleteRecursively() }
    }

    private fun displayName(uri: Uri): String? =
        runCatching {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull() ?: uri.lastPathSegment

    /** A received, verified file: move it to Downloads/Devti. Returns where it is now. */
    fun publish(transferId: Long, path: String): Published? {
        published(transferId)?.let { return it }
        val src = File(path)
        if (!src.isFile) return null
        val mime = mimeOf(src.name)
        val uri: Uri? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            toMediaStore(src, mime)
        } else {
            toPublicFolder(src)
        }
        val result = if (uri != null) {
            src.delete()
            Published(uri, mime)
        } else {
            // Kept in app storage; still openable through Devti's FileProvider.
            Published(FileProvider.getUriForFile(context, "${context.packageName}.files", src), mime)
        }
        prefs.edit { putString(transferId.toString(), "${result.uri}|${result.mime}") }
        return result
    }

    /** Where a received transfer ended up, if Devti already moved it. */
    fun published(transferId: Long): Published? {
        val v = prefs.getString(transferId.toString(), null) ?: return null
        val (u, m) = v.split('|', limit = 2).let { it[0] to it.getOrElse(1) { "*/*" } }
        return Published(Uri.parse(u), m)
    }

    private fun toMediaStore(src: File, mime: String): Uri? = runCatching {
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, src.name)
            put(MediaStore.Downloads.MIME_TYPE, mime)
            put(MediaStore.Downloads.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/Devti")
            put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: return null
        try {
            resolver.openOutputStream(uri)?.use { out -> src.inputStream().use { it.copyTo(out, 256 * 1024) } }
                ?: error("no output stream")
            resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            uri
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }
    }.getOrNull()

    @Suppress("DEPRECATION") // the public folder API is the only way on Android 8–9
    private fun toPublicFolder(src: File): Uri? {
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
            PackageManager.PERMISSION_GRANTED
        if (!granted) return null
        return runCatching {
            val dir = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "Devti").apply { mkdirs() }
            var dest = File(dir, src.name)
            var n = 1
            while (dest.exists()) dest = File(dir, "${src.nameWithoutExtension} ($n).${src.extension}".removeSuffix("."))
                .also { n++ }
            src.copyTo(dest)
            MediaScannerConnection.scanFile(context, arrayOf(dest.absolutePath), null, null)
            FileProvider.getUriForFile(context, "${context.packageName}.files", dest)
        }.getOrNull()
    }

    data class Published(val uri: Uri, val mime: String)

    companion object {
        fun mimeOf(name: String): String =
            Pure.extension(name)?.let { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it) } ?: "application/octet-stream"
    }
}
