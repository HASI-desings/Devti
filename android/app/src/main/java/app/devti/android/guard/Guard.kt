package app.devti.android.guard

import android.app.Activity
import android.app.KeyguardManager
import android.app.admin.DeviceAdminReceiver
import android.app.admin.DevicePolicyManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Build
import android.os.UserManager
import app.devti.android.continuity.Locator
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Receives Android's device-admin callbacks. Devti becomes the phone's *device owner*
 * only through the setup on the computer (`dpm set-device-owner …/.guard.GuardAdmin`).
 */
class GuardAdmin : DeviceAdminReceiver() {
    override fun onEnabled(context: Context, intent: Intent) {
        Guard.reportLater(context)
    }

    override fun onDisabled(context: Context, intent: Intent) {
        Guard.reportLater(context)
    }
}

/**
 * Stolen mode on Android.
 *
 * As **device owner**, Devti can lock the phone into one screen (lock task with no
 * features: no home, recents, notifications, quick settings or power menu), switch USB
 * data off, block safe mode, factory reset, airplane mode, new users, app installs and
 * Bluetooth, and keep Wi‑Fi and location on. Without device owner it can only show the
 * stolen screen and report location; the owner computer is told exactly which.
 */
object Guard {
    fun admin(context: Context) = ComponentName(context, GuardAdmin::class.java)

    private fun dpm(context: Context) = context.getSystemService(DevicePolicyManager::class.java)

    fun isDeviceOwner(context: Context): Boolean =
        runCatching { dpm(context).isDeviceOwnerApp(context.packageName) }.getOrDefault(false)

    /** User restrictions that hold while stolen mode is on. */
    private fun restrictions(): List<String> = buildList {
        add(UserManager.DISALLOW_DEBUGGING_FEATURES) // no adb from a computer
        add(UserManager.DISALLOW_USB_FILE_TRANSFER)
        add(UserManager.DISALLOW_MOUNT_PHYSICAL_MEDIA) // no OTG drives
        add(UserManager.DISALLOW_BLUETOOTH) // Bluetooth off, nothing new can pair
        add(UserManager.DISALLOW_CONFIG_BLUETOOTH)
        add(UserManager.DISALLOW_SAFE_BOOT) // safe mode would stop Devti
        add(UserManager.DISALLOW_FACTORY_RESET)
        add(UserManager.DISALLOW_ADD_USER)
        add(UserManager.DISALLOW_INSTALL_APPS)
        add(UserManager.DISALLOW_UNINSTALL_APPS)
        add(UserManager.DISALLOW_CONFIG_WIFI) // Wi-Fi stays as it is (on)
        add(UserManager.DISALLOW_CONFIG_MOBILE_NETWORKS)
        add(UserManager.DISALLOW_CONFIG_TETHERING)
        add(UserManager.DISALLOW_NETWORK_RESET)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            add(UserManager.DISALLOW_AIRPLANE_MODE)
            add(UserManager.DISALLOW_CONFIG_LOCATION)
            add(UserManager.DISALLOW_USER_SWITCH)
            add(UserManager.DISALLOW_CONFIG_DATE_TIME)
        }
    }

    /** Locks the phone down (device owner only). Safe to call repeatedly. */
    fun applyPolicy(context: Context) {
        if (!isDeviceOwner(context)) return
        val dpm = dpm(context)
        val admin = admin(context)
        // Wi-Fi and location on before their settings are locked.
        @Suppress("DEPRECATION") // device owners may still switch Wi-Fi
        runCatching { context.applicationContext.getSystemService(WifiManager::class.java)?.isWifiEnabled = true }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) runCatching { dpm.setLocationEnabled(admin, true) }
        for (r in restrictions()) runCatching { dpm.addUserRestriction(admin, r) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            runCatching { if (dpm.canUsbDataSignalingBeDisabled()) dpm.setUsbDataSignalingEnabled(false) }
        }
        runCatching { dpm.setStatusBarDisabled(admin, true) }
        runCatching { dpm.setLockTaskPackages(admin, arrayOf(context.packageName)) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            // No home, recents, notifications, system info, power menu or keyguard features.
            runCatching { dpm.setLockTaskFeatures(admin, DevicePolicyManager.LOCK_TASK_FEATURE_NONE) }
        }
    }

    /** Undoes [applyPolicy]. */
    fun liftPolicy(context: Context) {
        if (!isDeviceOwner(context)) return
        val dpm = dpm(context)
        val admin = admin(context)
        for (r in restrictions()) runCatching { dpm.clearUserRestriction(admin, r) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            runCatching { if (dpm.canUsbDataSignalingBeDisabled()) dpm.setUsbDataSignalingEnabled(true) }
        }
        runCatching { dpm.setStatusBarDisabled(admin, false) }
        runCatching { dpm.setLockTaskPackages(admin, emptyArray()) }
    }

    /** Pins [activity] (the stolen screen) so nothing else can be reached. */
    fun pin(activity: Activity) {
        if (isDeviceOwner(activity)) runCatching { activity.startLockTask() }
    }

    fun unpin(activity: Activity) {
        runCatching { activity.stopLockTask() }
    }

    /** What this phone can and can't protect, for the owner computer (`GuardReport`). */
    fun report(context: Context): String {
        val owner = isDeviceOwner(context)
        val secure = context.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true
        val usbOff = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            runCatching { dpm(context).canUsbDataSignalingBeDisabled() }.getOrDefault(false)
        val protections = mutableListOf("Black stolen screen with your message", "Live location every 30 seconds")
        val missing = mutableListOf<String>()
        if (owner) {
            protections += listOf(
                "The stolen screen can't be left: no home, recents or notifications",
                "Power menu blocked",
                if (usbOff) "USB data off: no computer or OTG drive can connect" else "No file transfer, debugging or OTG drives over USB",
                "Bluetooth off, nothing new can pair",
                "Safe mode, factory reset and airplane mode blocked",
                "Wi-Fi and location kept on",
            )
            if (!usbOff) missing += "This phone can't switch USB data off completely (charging still works; file transfer and debugging are blocked)"
        } else {
            missing += "Power menu, USB, safe mode and factory reset stay available: run the USB setup on your computer for full protection"
            missing += "The stolen screen can be left with the home button"
        }
        if (!secure) missing += "No screen lock: set a PIN or pattern, or a restart gets past stolen mode"
        if (!Locator.permittedInBackground(context)) missing += "Location only while Devti is open: allow location “All the time”"
        missing += "Holding the power button for about 15 seconds forces a restart; no app can stop that"
        missing += "Mobile data can't be forced on; with no SIM and no known Wi-Fi it can't report its location"
        return buildJsonObject {
            put("level", if (owner) "device_owner" else "basic")
            put("protections", buildJsonArray { protections.forEach { add(kotlinx.serialization.json.JsonPrimitive(it)) } })
            put("missing", buildJsonArray { missing.forEach { add(kotlinx.serialization.json.JsonPrimitive(it)) } })
        }.toString()
    }

    /** Re-sends the report once the core is up (device-admin changes, permission changes). */
    fun reportLater(context: Context) {
        val app = context.applicationContext as? app.devti.android.DevtiApp ?: return
        app.core.syncGuard()
    }
}
