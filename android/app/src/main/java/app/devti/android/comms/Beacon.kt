package app.devti.android.comms

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import androidx.core.content.ContextCompat

/**
 * "Let your computer sense when you walk away": a low-power BLE advertisement carrying
 * an 8-byte token that only computers paired with this phone recognise. It changes
 * every 15 minutes, so nobody else can follow the phone by it. No name, no address
 * Devti controls, nothing to connect to.
 */
class Beacon(private val context: Context, private val token: () -> ByteArray?) {
    private val main = Handler(Looper.getMainLooper())
    private var on = false
    private val callback = object : AdvertiseCallback() {}
    private val refresh = Runnable { restart() }

    fun allowed(): Boolean = Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.BLUETOOTH_ADVERTISE) == PackageManager.PERMISSION_GRANTED

    fun start() {
        on = true
        restart()
    }

    @SuppressLint("MissingPermission")
    fun stop() {
        on = false
        main.removeCallbacks(refresh)
        runCatching { advertiser()?.stopAdvertising(callback) }
    }

    private fun advertiser() = context.getSystemService(BluetoothManager::class.java)?.adapter?.bluetoothLeAdvertiser

    /** (Re)starts the advert with the current token; retries every minute (Bluetooth may be off). */
    @SuppressLint("MissingPermission")
    private fun restart() {
        main.removeCallbacks(refresh)
        if (!on) return
        main.postDelayed(refresh, 60_000)
        val adv = advertiser() ?: return
        runCatching { adv.stopAdvertising(callback) }
        if (!allowed()) return
        val bytes = token() ?: return
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_POWER)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(false)
            .build()
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceData(ParcelUuid.fromString(SERVICE_UUID), bytes)
            .build()
        runCatching { adv.startAdvertising(settings, data, callback) }
    }

    companion object {
        /** Same as devti_core::rules::PROXIMITY_SERVICE_UUID. */
        const val SERVICE_UUID = "6a1f0001-7d6e-4b8a-9e36-d3e7f1a0c0de"
    }
}
