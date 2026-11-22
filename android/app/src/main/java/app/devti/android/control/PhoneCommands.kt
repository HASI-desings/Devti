package app.devti.android.control

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.media.AudioManager
import android.provider.MediaStore
import android.view.KeyEvent
import app.devti.android.core.state.Catalog
import app.devti.android.core.state.CatalogApp
import app.devti.android.core.state.CatalogCommand

/**
 * What this phone offers to a computer's remote panel: its apps, a few built-in
 * commands, lock and volume. Media keys and volume use AudioManager directly; lock,
 * screenshots and taps need the Devti control accessibility service.
 */
class PhoneCommands(private val context: Context) {
    private val audio = context.getSystemService(AudioManager::class.java)
    @Volatile private var torchOn = false

    fun catalog(): Catalog = Catalog(
        apps = launchableApps(),
        commands = listOf(
            CatalogCommand("torch", "Flashlight on / off"),
            CatalogCommand("screenshot", "Take a screenshot"),
            CatalogCommand("camera", "Open the camera"),
        ),
        system = listOf("lock", "set_volume"),
    )

    private fun launchableApps(): List<CatalogApp> {
        val pm = context.packageManager
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        @Suppress("DEPRECATION")
        val list = runCatching { pm.queryIntentActivities(intent, 0) }.getOrDefault(emptyList())
        return list.map { CatalogApp(it.activityInfo.packageName, it.loadLabel(pm).toString()) }
            .filter { it.id != context.packageName }
            .distinctBy { it.id }
            .sortedBy { it.name.lowercase() }
            .take(500)
    }

    fun launch(pkg: String): Boolean {
        val intent = context.packageManager.getLaunchIntentForPackage(pkg) ?: return false
        return runCatching { context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }.isSuccess
    }

    /** Runs a built-in command: `(ok, output)`. */
    fun run(id: String): Pair<Boolean, String> = when (id) {
        "torch" -> torch()
        "screenshot" -> {
            val a = ControlAccessibility.running()
            if (a?.nav("screenshot") == true) true to "Screenshot taken." else false to NEEDS_ACCESSIBILITY
        }
        "camera" -> {
            val ok = runCatching {
                context.startActivity(Intent(MediaStore.INTENT_ACTION_STILL_IMAGE_CAMERA).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }.isSuccess
            ok to if (ok) "Camera opened." else "Android didn't let Devti open the camera from the background."
        }
        else -> false to "This phone has no command called $id."
    }

    private fun torch(): Pair<Boolean, String> {
        if (!context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_FLASH)) return false to "This phone has no flashlight."
        val cm = context.getSystemService(CameraManager::class.java)
        val id = cm.cameraIdList.firstOrNull { cm.getCameraCharacteristics(it).get(CameraCharacteristics.FLASH_INFO_AVAILABLE) == true }
            ?: return false to "This phone has no flashlight."
        return runCatching {
            torchOn = !torchOn
            cm.setTorchMode(id, torchOn)
            true to if (torchOn) "Flashlight on." else "Flashlight off."
        }.getOrElse { false to "The flashlight is in use by another app." }
    }

    fun media(action: String) {
        val code = when (action) {
            "play_pause" -> KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE
            "next" -> KeyEvent.KEYCODE_MEDIA_NEXT
            "previous" -> KeyEvent.KEYCODE_MEDIA_PREVIOUS
            "stop" -> KeyEvent.KEYCODE_MEDIA_STOP
            "volume_up" -> return audio.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_RAISE, AudioManager.FLAG_SHOW_UI)
            "volume_down" -> return audio.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_LOWER, AudioManager.FLAG_SHOW_UI)
            "mute" -> return audio.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_TOGGLE_MUTE, AudioManager.FLAG_SHOW_UI)
            else -> return
        }
        audio.dispatchMediaKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
        audio.dispatchMediaKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
    }

    fun setVolume(percent: Int) {
        val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, (max * percent.coerceIn(0, 100) + 50) / 100, AudioManager.FLAG_SHOW_UI)
    }

    companion object {
        const val NEEDS_ACCESSIBILITY = "Turn on Devti control in the phone's Settings → Accessibility first."
    }
}
