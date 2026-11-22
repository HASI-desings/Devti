package app.devti.android.comms

import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.os.Build
import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import app.devti.android.DevtiApp
import app.devti.android.R
import app.devti.android.continuity.Alerts
import app.devti.android.continuity.Pure
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

/**
 * The phone's camera as the computer's webcam: CameraX analysis frames (latest only) →
 * upright JPEG at the core's adaptive quality → the same frame path as screen sharing.
 * The computer shows it as a camera (v4l2loopback on Linux). Android shows its own
 * green camera indicator, and the notification has a Stop button.
 */
class CameraShareService : LifecycleService() {
    private val worker = Executors.newSingleThreadExecutor()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        if (intent?.action == ACTION_STOP) {
            (application as DevtiApp).core.stopCamera()
            stopSelf()
            return START_NOT_STICKY
        }
        ServiceCompat.startForeground(
            this,
            Alerts.ID_CAMERA,
            NotificationCompat.Builder(this, Alerts.CONNECTION)
                .setSmallIcon(R.drawable.ic_stat_devti)
                .setContentTitle("Your computer is using this phone's camera")
                .setOngoing(true)
                .setContentIntent(Alerts.openApp(this))
                .addAction(
                    0,
                    "Stop",
                    android.app.PendingIntent.getService(this, 6, Intent(this, CameraShareService::class.java).setAction(ACTION_STOP), android.app.PendingIntent.FLAG_IMMUTABLE),
                )
                .build(),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA else 0,
        )
        val back = intent?.getBooleanExtra(EXTRA_BACK, false) ?: false
        val future = ProcessCameraProvider.getInstance(this)
        future.addListener({
            val provider = future.get()
            @Suppress("DEPRECATION") // ResolutionSelector picks the same 720p here, with more ceremony.
            val analysis = ImageAnalysis.Builder()
                .setTargetResolution(Size(1280, 720))
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
            analysis.setAnalyzer(worker) { image -> image.use { send(it) } }
            provider.unbindAll()
            runCatching {
                provider.bindToLifecycle(this, if (back) CameraSelector.DEFAULT_BACK_CAMERA else CameraSelector.DEFAULT_FRONT_CAMERA, analysis)
            }.onFailure { stopSelf() }
        }, ContextCompat.getMainExecutor(this))
        running = true
        (application as DevtiApp).core.cameraSharing(true)
        return START_NOT_STICKY
    }

    private fun send(image: ImageProxy) {
        val core = (application as DevtiApp).core
        val quality = core.screenTarget().optInt("quality", 70).coerceIn(30, 90)
        val frame = Nv21.from(image) ?: return
        val upright = Nv21.rotate(frame, image.imageInfo.rotationDegrees)
        val out = ByteArrayOutputStream(128 * 1024)
        YuvImage(upright.data, ImageFormat.NV21, upright.width, upright.height, null)
            .compressToJpeg(Rect(0, 0, upright.width, upright.height), quality, out)
        core.pushScreenFrame(upright.width, upright.height, out.toByteArray())
    }

    override fun onDestroy() {
        running = false
        (application as DevtiApp).core.cameraSharing(false)
        worker.shutdown()
        super.onDestroy()
    }

    companion object {
        private const val ACTION_STOP = "app.devti.android.STOP_CAMERA"
        const val EXTRA_BACK = "back"

        @Volatile var running = false
            private set

        fun start(context: Context, back: Boolean = false) {
            ContextCompat.startForegroundService(context, Intent(context, CameraShareService::class.java).putExtra(EXTRA_BACK, back))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, CameraShareService::class.java))
        }
    }
}

/** NV21 frames: the layout YuvImage encodes. */
class Nv21(val data: ByteArray, val width: Int, val height: Int) {
    companion object {
        /** YUV_420_888 → NV21, honouring row and pixel strides. */
        fun from(image: ImageProxy): Nv21? {
            if (image.format != ImageFormat.YUV_420_888) return null
            val (w, h) = image.width to image.height
            val out = ByteArray(w * h * 3 / 2)
            val y = image.planes[0]
            var pos = 0
            for (row in 0 until h) {
                y.buffer.position(row * y.rowStride)
                y.buffer.get(out, pos, w)
                pos += w
            }
            val u = image.planes[1]
            val v = image.planes[2]
            for (row in 0 until h / 2) {
                for (col in 0 until w / 2) {
                    out[pos++] = v.buffer.get(row * v.rowStride + col * v.pixelStride)
                    out[pos++] = u.buffer.get(row * u.rowStride + col * u.pixelStride)
                }
            }
            return Nv21(out, w, h)
        }

        /** Upright copy: clockwise by 0, 90, 180 or 270 degrees (what the sensor reports). */
        fun rotate(f: Nv21, degrees: Int): Nv21 {
            val d = ((degrees % 360) + 360) % 360
            if (d == 0) return f
            val data = Pure.rotateNv21(f.data, f.width, f.height, d)
            return if (d == 180) Nv21(data, f.width, f.height) else Nv21(data, f.height, f.width)
        }
    }
}
