package app.devti.android.continuity

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import android.os.CancellationSignal
import android.os.Handler
import android.os.Looper
import android.telephony.TelephonyManager
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.util.concurrent.Executors

/**
 * Find my device on Android: one location fix from the phone's own location providers
 * (GPS, then Wi-Fi/cell network), plus the Wi-Fi name or mobile carrier. No Google Play
 * services, so it works on every Android. The core adds the IP addresses and battery.
 */
object Locator {
    private val executor = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())

    fun permitted(context: Context): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED

    /** Whether Android lets Devti locate while it's in the background ("Allow all the time"). */
    fun permittedInBackground(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.Q ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION) == PackageManager.PERMISSION_GRANTED

    /**
     * Gets a fix (at most ~12 s) and hands back a `LocationFix` as JSON. Never throws:
     * failures come back with `error` set, so the computer always hears an answer.
     */
    @SuppressLint("MissingPermission") // checked by permitted()
    fun fix(context: Context, done: (String) -> Unit) {
        val network = networkName(context)
        if (!permitted(context)) {
            done(json(null, network, "Devti isn't allowed to use this phone's location"))
            return
        }
        val lm = context.getSystemService(LocationManager::class.java)
        if (lm == null || !LocationManagerCompat.isLocationEnabled(lm)) {
            done(json(lastKnown(lm), network, "location is turned off on the phone"))
            return
        }
        val providers = listOf(
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) LocationManager.FUSED_PROVIDER else null,
            LocationManager.GPS_PROVIDER,
            LocationManager.NETWORK_PROVIDER,
        ).filterNotNull().filter { runCatching { lm.isProviderEnabled(it) }.getOrDefault(false) }
        if (providers.isEmpty()) {
            done(json(lastKnown(lm), network, "no location provider is on"))
            return
        }
        var answered = false
        var best: Location? = null
        val signals = mutableListOf<CancellationSignal>()
        fun finish() {
            if (answered) return
            answered = true
            signals.forEach { it.cancel() }
            val loc = best ?: lastKnown(lm)
            done(json(loc, network, if (loc == null) "no fix yet: try again outdoors" else null))
        }
        var pending = providers.size
        for (p in providers) {
            val signal = CancellationSignal()
            signals += signal
            LocationManagerCompat.getCurrentLocation(lm, p, signal, executor) { loc ->
                main.post {
                    if (loc != null && (best == null || loc.accuracy < best!!.accuracy)) best = loc
                    pending -= 1
                    // A good GPS-grade fix, or every provider answered: done.
                    if ((best?.accuracy ?: Float.MAX_VALUE) < 25f || pending == 0) finish()
                }
            }
        }
        main.postDelayed({ finish() }, 12_000)
    }

    @SuppressLint("MissingPermission")
    private fun lastKnown(lm: LocationManager?): Location? {
        lm ?: return null
        return runCatching {
            lm.getProviders(true).mapNotNull { lm.getLastKnownLocation(it) }.maxByOrNull { it.time }
        }.getOrNull()
    }

    private fun json(loc: Location?, network: String?, error: String?): String = buildJsonObject {
        if (loc != null) {
            put("lat", loc.latitude)
            put("lon", loc.longitude)
            if (loc.hasAccuracy()) put("accuracy_m", loc.accuracy)
            put("ts", loc.time)
            put("source", if (loc.provider == LocationManager.GPS_PROVIDER) "gps" else "network")
        } else {
            put("source", "none")
        }
        if (network != null) put("network", network)
        if (error != null) put("error", error)
    }.toString()

    /** "Wi-Fi: Hostel 2G" or the mobile carrier's name. */
    @Suppress("DEPRECATION") // connectionInfo is the only SSID source without a callback.
    fun networkName(context: Context): String? {
        val cm = context.getSystemService(ConnectivityManager::class.java) ?: return null
        val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return null
        return when {
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> {
                val ssid = runCatching {
                    context.applicationContext.getSystemService(WifiManager::class.java)?.connectionInfo?.ssid
                }.getOrNull()?.trim('"')
                if (ssid.isNullOrBlank() || ssid == "<unknown ssid>") "Wi-Fi" else "Wi-Fi: $ssid"
            }
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> {
                val carrier = context.getSystemService(TelephonyManager::class.java)?.networkOperatorName
                if (carrier.isNullOrBlank()) "Mobile data" else "Mobile data: $carrier"
            }
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "Ethernet"
            else -> null
        }
    }
}
