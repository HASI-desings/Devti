package app.devti.android.continuity

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import androidx.core.content.ContextCompat
import app.devti.android.core.state.SharedFolder

/**
 * Which of this phone's folders computers may browse (Phase 7), and the storage access
 * that needs. Android only lets an app read other apps' files by path with "All files
 * access" (Android 11+) or the storage permission (8–10), so sharing asks for it and
 * says why. Nothing is shared until the user turns it on; computers can only read.
 */
object PhoneFolders {
    private val DIRS = listOf(
        "Camera" to Environment.DIRECTORY_DCIM,
        "Pictures" to Environment.DIRECTORY_PICTURES,
        "Downloads" to Environment.DIRECTORY_DOWNLOADS,
        "Documents" to Environment.DIRECTORY_DOCUMENTS,
        "Music" to Environment.DIRECTORY_MUSIC,
        "Movies" to Environment.DIRECTORY_MOVIES,
    )

    fun canRead(context: Context): Boolean = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        Environment.isExternalStorageManager()
    } else {
        ContextCompat.checkSelfPermission(context, Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED
    }

    /** Android 11+: the "All files access" page for Devti (8–10 use a runtime permission). */
    fun accessIntent(context: Context): Intent? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:${context.packageName}"))
    } else {
        null
    }

    /** The usual media and document folders that exist on this phone. */
    @Suppress("DEPRECATION") // the public-directory paths are what the core reads
    fun defaults(): List<SharedFolder> = DIRS.mapNotNull { (name, dir) ->
        Environment.getExternalStoragePublicDirectory(dir).takeIf { it.isDirectory }?.let { SharedFolder(name, it.absolutePath) }
    }
}
