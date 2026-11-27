package app.devti.android.guard

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import app.devti.android.DevtiApp
import app.devti.android.R
import app.devti.android.continuity.Alerts
import app.devti.android.continuity.Locator
import app.devti.android.core.DevtiService
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * While stolen mode is on: sends this phone's location to its owner every 30 seconds,
 * and brings the stolen screen back if anything covered it.
 */
class GuardService : LifecycleService() {
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        ServiceCompat.startForeground(
            this,
            Alerts.ID_GUARD,
            NotificationCompat.Builder(this, Alerts.CONNECTION)
                .setSmallIcon(R.drawable.ic_stat_devti)
                .setContentTitle("Stolen mode is on")
                .setContentText("This phone is locked by its owner and shares its location.")
                .setOngoing(true)
                .setSilent(true)
                .build(),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION else 0,
        )
        if (!running) {
            running = true
            val core = (application as DevtiApp).core
            lifecycleScope.launch {
                while (isActive) {
                    val g = core.guardLocal()
                    if (g?.state?.on != true) break
                    val owner = g.owner
                    if (owner != null) {
                        Locator.fix(this@GuardService) { fix -> core.pushLocation(owner, fix) }
                    }
                    // Keep the stolen screen in front.
                    if (!StolenScreen.visible) runCatching { startActivity(StolenActivity.intent(this@GuardService)) }
                    delay(30_000)
                }
                stopSelf()
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    companion object {
        @Volatile var running = false
            private set

        fun start(context: Context) {
            ContextCompat.startForegroundService(context, Intent(context, GuardService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, GuardService::class.java))
        }
    }
}

/** Whether the stolen screen is on top right now. */
object StolenScreen {
    @Volatile var visible = false
}

/** After a restart: start Devti, which puts stolen mode back if it was on. */
class GuardBoot : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val app = context.applicationContext as? DevtiApp ?: return
        // Only stolen mode (or a phone already set up) needs Devti right after boot.
        app.core.start()
        if (app.core.guardLocal()?.state?.on == true) {
            runCatching { DevtiService.start(context) }
            app.core.syncGuard()
        }
    }
}
