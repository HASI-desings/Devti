package app.devti.android

import android.app.Application
import app.devti.android.core.DevtiCore

/** Process entry point: owns the single Devti Core instance for the whole app. */
class DevtiApp : Application() {
    lateinit var core: DevtiCore
        private set

    override fun onCreate() {
        super.onCreate()
        core = DevtiCore(this)
    }
}
