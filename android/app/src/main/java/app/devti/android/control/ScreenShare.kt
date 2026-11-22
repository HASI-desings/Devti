package app.devti.android.control

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.pm.PackageManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import app.devti.android.DevtiApp
import app.devti.android.R
import app.devti.android.comms.AudioShareService
import app.devti.android.comms.CameraShareService
import app.devti.android.continuity.Alerts
import java.io.ByteArrayOutputStream
import org.json.JSONObject

/**
 * Asks before anything leaves the phone, then starts the right service:
 * * screen (default) — Android's own "Start now" dialog, which can't be skipped or remembered;
 * * camera — Devti's "Let … use this camera?" (skipped right after you offered it), then
 *   the camera permission;
 * * mic — the microphone permission; relay — the microphone permission, then Android's
 *   casting dialog, which is what lets an app hear other apps' sound.
 */
class ScreenShareActivity : Activity() {
    private val core get() = (application as DevtiApp).core
    private val mode get() = intent.getStringExtra(EXTRA_MODE) ?: MODE_SCREEN
    private val device get() = intent.getStringExtra(EXTRA_DEVICE).orEmpty()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        when (mode) {
            MODE_CAMERA -> if (core.cameraOfferedRecently()) needPermission(Manifest.permission.CAMERA) else confirmCamera()
            MODE_MIC, MODE_RELAY -> needPermission(Manifest.permission.RECORD_AUDIO)
            else -> askProjection()
        }
    }

    private fun confirmCamera() {
        val name = intent.getStringExtra(EXTRA_FROM_NAME) ?: "Your computer"
        AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert)
            .setTitle(getString(R.string.camera_request_title, name))
            .setMessage(R.string.camera_request_body)
            .setPositiveButton("Allow") { _, _ -> needPermission(Manifest.permission.CAMERA) }
            .setNegativeButton("Don't allow") { _, _ -> decline() }
            .setOnCancelListener { decline() }
            .show()
    }

    private fun needPermission(permission: String) {
        if (checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED) granted()
        else requestPermissions(arrayOf(permission), REQUEST_PERMISSION)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) granted() else decline()
    }

    private fun granted() {
        when (mode) {
            MODE_CAMERA -> {
                CameraShareService.start(this, intent.getBooleanExtra(EXTRA_BACK, false))
                finish()
            }
            MODE_MIC -> {
                AudioShareService.startMic(this, device)
                finish()
            }
            MODE_RELAY -> askProjection()
            else -> finish()
        }
    }

    private fun askProjection() {
        val mpm = getSystemService(MediaProjectionManager::class.java)
        @Suppress("DEPRECATION")
        startActivityForResult(mpm.createScreenCaptureIntent(), REQUEST_PROJECTION)
    }

    private fun decline() {
        if (mode == MODE_CAMERA || mode == MODE_SCREEN) core.declineScreenShare(camera = mode == MODE_CAMERA)
        finish()
    }

    @Deprecated("Activity result API is fine for a one-shot translucent activity")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST_PROJECTION && resultCode == RESULT_OK && data != null) {
            if (mode == MODE_RELAY) AudioShareService.startRelay(this, device, resultCode, data)
            else ScreenCaptureService.start(this, resultCode, data)
            finish()
        } else {
            decline()
        }
    }

    companion object {
        private const val REQUEST_PROJECTION = 41
        private const val REQUEST_PERMISSION = 42
        const val EXTRA_MODE = "mode"
        const val EXTRA_DEVICE = "device"
        const val EXTRA_FROM_NAME = "from_name"
        const val EXTRA_BACK = "back"
        const val MODE_SCREEN = "screen"
        const val MODE_CAMERA = "camera"
        const val MODE_MIC = "mic"
        const val MODE_RELAY = "relay"

        fun intent(context: Context, mode: String, device: String = "", fromName: String = "", back: Boolean = false): Intent =
            Intent(context, ScreenShareActivity::class.java)
                .putExtra(EXTRA_MODE, mode)
                .putExtra(EXTRA_DEVICE, device)
                .putExtra(EXTRA_FROM_NAME, fromName)
                .putExtra(EXTRA_BACK, back)
    }
}

/**
 * Captures the screen into JPEG frames for the computers watching it.
 *
 * A VirtualDisplay mirrors the screen into an ImageReader at the width the core asks
 * for ([app.devti.android.core.DevtiCore.screenTarget], which adapts to the network).
 * Each new image becomes a JPEG on a background thread, at most at the requested frame
 * rate; Devti Core only ever sends the newest one, so a slow network lowers the frame
 * rate rather than building a delay. Android shows its own "sharing your screen" chip
 * the whole time, and the notification here has a Stop button.
 */
class ScreenCaptureService : Service() {
    private var projection: MediaProjection? = null
    private var display: VirtualDisplay? = null
    private var reader: ImageReader? = null
    private var thread: HandlerThread? = null
    private var lastFrame = 0L

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            (application as DevtiApp).core.stopSharingScreen()
            stopSelf()
            return START_NOT_STICKY
        }
        val code = intent?.getIntExtra(EXTRA_CODE, 0) ?: 0
        @Suppress("DEPRECATION")
        val data: Intent? = intent?.getParcelableExtra(EXTRA_DATA)
        ServiceCompat.startForeground(
            this,
            Alerts.ID_SCREEN,
            notification(),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION else 0,
        )
        if (data == null || projection != null) return START_NOT_STICKY
        val mpm = getSystemService(MediaProjectionManager::class.java)
        val p = mpm.getMediaProjection(code, data) ?: run {
            stopSelf()
            return START_NOT_STICKY
        }
        projection = p
        p.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() {
                (application as DevtiApp).core.stopSharingScreen()
                stopSelf()
            }
        }, null)
        startCapture(p)
        running = true
        return START_NOT_STICKY
    }

    private fun startCapture(p: MediaProjection) {
        val core = (application as DevtiApp).core
        val target = core.screenTarget()
        val metrics = resources.displayMetrics
        @Suppress("DEPRECATION")
        val real = android.util.DisplayMetrics().also { (getSystemService(WINDOW_SERVICE) as android.view.WindowManager).defaultDisplay.getRealMetrics(it) }
        val sw = real.widthPixels.takeIf { it > 0 } ?: metrics.widthPixels
        val sh = real.heightPixels.takeIf { it > 0 } ?: metrics.heightPixels
        val scale = minOf(1f, target.optInt("max_width", 1080).toFloat() / sw)
        val w = (sw * scale).toInt().coerceAtLeast(2) and 1.inv()
        val h = (sh * scale).toInt().coerceAtLeast(2) and 1.inv()
        val t = HandlerThread("devti-capture").apply { start() }
        thread = t
        val handler = Handler(t.looper)
        val r = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 2)
        reader = r
        val out = ByteArrayOutputStream(256 * 1024)
        var bitmap: Bitmap? = null
        r.setOnImageAvailableListener({ ir ->
            val image = runCatching { ir.acquireLatestImage() }.getOrNull() ?: return@setOnImageAvailableListener
            try {
                val now = SystemClock.elapsedRealtime()
                val goal = core.screenTarget()
                val minGap = 1000L / goal.optInt("fps", 30).coerceIn(1, 60)
                if (now - lastFrame < minGap) return@setOnImageAvailableListener
                lastFrame = now
                val plane = image.planes[0]
                val rowPx = plane.rowStride / plane.pixelStride
                val bmp = bitmap?.takeIf { it.width == rowPx && it.height == h }
                    ?: Bitmap.createBitmap(rowPx, h, Bitmap.Config.ARGB_8888).also { bitmap = it }
                bmp.copyPixelsFromBuffer(plane.buffer)
                val frame = if (rowPx == w) bmp else Bitmap.createBitmap(bmp, 0, 0, w, h)
                out.reset()
                frame.compress(Bitmap.CompressFormat.JPEG, goal.optInt("quality", 70).coerceIn(30, 90), out)
                if (frame !== bmp) frame.recycle()
                core.pushScreenFrame(w, h, out.toByteArray())
            } catch (_: Throwable) {
                // A frame we couldn't read; the next one will do.
            } finally {
                image.close()
            }
        }, handler)
        display = p.createVirtualDisplay(
            "devti-screen",
            w,
            h,
            metrics.densityDpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            r.surface,
            null,
            handler,
        )
    }

    private fun notification() = NotificationCompat.Builder(this, Alerts.CONNECTION)
        .setSmallIcon(R.drawable.ic_stat_devti)
        .setContentTitle(getString(R.string.screen_sharing_title))
        .setContentText(getString(R.string.screen_sharing_body))
        .setOngoing(true)
        .setContentIntent(Alerts.openApp(this))
        .addAction(
            0,
            getString(R.string.screen_sharing_stop),
            android.app.PendingIntent.getService(
                this,
                3,
                Intent(this, ScreenCaptureService::class.java).setAction(ACTION_STOP),
                android.app.PendingIntent.FLAG_IMMUTABLE,
            ),
        )
        .build()

    override fun onDestroy() {
        running = false
        display?.release()
        reader?.close()
        projection?.stop()
        thread?.quitSafely()
        display = null
        reader = null
        projection = null
        super.onDestroy()
    }

    companion object {
        private const val EXTRA_CODE = "code"
        private const val EXTRA_DATA = "data"
        private const val ACTION_STOP = "app.devti.android.STOP_SCREEN"

        @Volatile var running = false
            private set

        fun start(context: Context, code: Int, data: Intent) {
            ContextCompat.startForegroundService(
                context,
                Intent(context, ScreenCaptureService::class.java).putExtra(EXTRA_CODE, code).putExtra(EXTRA_DATA, data),
            )
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, ScreenCaptureService::class.java))
        }
    }
}

/** Parses the core's screen target JSON; defaults if anything is off. */
fun screenTargetOf(json: String?): JSONObject = runCatching { JSONObject(json ?: "{}") }.getOrDefault(JSONObject())
