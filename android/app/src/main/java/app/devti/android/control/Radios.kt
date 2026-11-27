package app.devti.android.control

import android.bluetooth.BluetoothManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.wifi.WifiManager
import android.os.Build
import android.provider.Settings
import android.telephony.TelephonyManager
import app.devti.android.guard.Guard
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import rikka.shizuku.Shizuku
import java.util.concurrent.TimeUnit

/**
 * A computer switching this phone's Wi‑Fi, mobile data, Bluetooth, hotspot or airplane
 * mode. Android lets ordinary apps do none of that any more, so, in order:
 *
 * 1. **Shizuku** (free, open source): when it's running and Devti is allowed, the switch
 *    runs as Android's own shell (`svc wifi disable`…), like from a computer over USB.
 * 2. **Device owner** (after stolen-protection setup): Wi‑Fi only.
 * 3. Otherwise the phone opens the right Settings page, for the person holding it.
 */
object Radios {
    val ALL = listOf("wifi", "data", "bluetooth", "hotspot", "airplane")

    fun shizukuRunning(): Boolean = runCatching { Shizuku.pingBinder() }.getOrDefault(false)

    fun shizukuAllowed(): Boolean = runCatching {
        Shizuku.pingBinder() && !Shizuku.isPreV11() && Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED
    }.getOrDefault(false)

    fun requestShizuku() {
        runCatching { if (shizukuRunning()) Shizuku.requestPermission(4242) }
    }

    /** Runs one shell command through Shizuku: (exit code, output). */
    private fun sh(vararg cmd: String): Pair<Int, String> = runCatching {
        // `newProcess` is the simple way to run a shell command; Shizuku keeps it private
        // in favour of bound services, which is far more machinery for five commands.
        val m = Shizuku::class.java.getDeclaredMethod(
            "newProcess",
            Array<String>::class.java,
            Array<String>::class.java,
            String::class.java,
        )
        m.isAccessible = true
        val p = m.invoke(null, arrayOf(*cmd), null, null) as Process
        val done = p.waitFor(8, TimeUnit.SECONDS)
        val out = (p.inputStream.bufferedReader().readText() + p.errorStream.bufferedReader().readText()).trim()
        (if (done) p.exitValue() else -1) to out
    }.getOrElse { -1 to (it.message ?: "Shizuku isn't available") }

    /** The radios' state now, and how this phone can switch them (`Radios` JSON). */
    fun state(context: Context): String {
        val wifi = context.applicationContext.getSystemService(WifiManager::class.java)
        val tm = context.getSystemService(TelephonyManager::class.java)
        val bt = context.getSystemService(BluetoothManager::class.java)?.adapter
        val shizuku = shizukuAllowed()
        val owner = Guard.isDeviceOwner(context)
        return buildJsonObject {
            runCatching { wifi?.isWifiEnabled }.getOrNull()?.let { put("wifi", it) }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) runCatching { tm?.isDataEnabled }.getOrNull()?.let { put("data", it) }
            runCatching { bt?.isEnabled }.getOrNull()?.let { put("bluetooth", it) }
            hotspotOn(wifi)?.let { put("hotspot", it) }
            put("airplane", Settings.Global.getInt(context.contentResolver, Settings.Global.AIRPLANE_MODE_ON, 0) == 1)
            put(
                "direct",
                buildJsonArray {
                    when {
                        shizuku -> ALL.forEach { add(JsonPrimitive(it)) }
                        owner -> add(JsonPrimitive("wifi"))
                    }
                },
            )
            put("via", if (shizuku) "shizuku" else if (owner) "device_owner" else "settings")
        }.toString()
    }

    private fun hotspotOn(wifi: WifiManager?): Boolean? = runCatching {
        wifi?.javaClass?.getMethod("isWifiApEnabled")?.invoke(wifi) as? Boolean
    }.getOrNull()

    /** Switches [radio]; (ok, message for the computer). */
    fun set(context: Context, radio: String, on: Boolean): Pair<Boolean, String> {
        if (shizukuAllowed()) {
            val word = if (on) "enable" else "disable"
            val (code, out) = when (radio) {
                "wifi" -> sh("svc", "wifi", word)
                "data" -> sh("svc", "data", word)
                "bluetooth" -> sh("svc", "bluetooth", word).takeIf { it.first == 0 } ?: sh("cmd", "bluetooth_manager", word)
                "airplane" -> sh("cmd", "connectivity", "airplane-mode", word)
                "hotspot" -> if (on) {
                    val name = "${Build.MODEL} Devti".take(30)
                    val pass = hotspotPassword()
                    val r = sh("cmd", "wifi", "start-softap", name, "wpa2", pass)
                    if (r.first == 0) return true to "Hotspot “$name” is on. Password: $pass"
                    r
                } else {
                    sh("cmd", "wifi", "stop-softap")
                }
                else -> -1 to "unknown radio"
            }
            return if (code == 0) true to done(radio, on) else false to "Shizuku couldn't do it: ${out.take(160)}"
        }
        if (radio == "wifi" && Guard.isDeviceOwner(context)) {
            @Suppress("DEPRECATION") // device owners may still switch Wi-Fi
            val ok = runCatching { context.applicationContext.getSystemService(WifiManager::class.java)?.setWifiEnabled(on) == true }.getOrDefault(false)
            if (ok) return true to done(radio, on)
        }
        // Android only lets the person holding the phone do it: open the right page.
        val page = settingsPage(radio)
        app.devti.android.continuity.Alerts.radioRequest(context, label(radio), on, page)
        return false to "Android doesn't let apps switch ${label(radio)} by themselves. The phone shows its ${label(radio)} settings to " +
            "whoever holds it. To switch it from here, install Shizuku on the phone and allow Devti in it."
    }

    private fun done(radio: String, on: Boolean) = "${label(radio).replaceFirstChar { it.uppercase() }} is ${if (on) "on" else "off"}."

    private fun label(radio: String) = when (radio) {
        "wifi" -> "Wi-Fi"
        "data" -> "mobile data"
        "bluetooth" -> "Bluetooth"
        "hotspot" -> "the hotspot"
        "airplane" -> "airplane mode"
        else -> radio
    }

    private fun settingsPage(radio: String): Intent = when (radio) {
        "wifi" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) Intent(Settings.Panel.ACTION_WIFI) else Intent(Settings.ACTION_WIFI_SETTINGS)
        "data" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) Intent(Settings.Panel.ACTION_INTERNET_CONNECTIVITY) else Intent(Settings.ACTION_DATA_ROAMING_SETTINGS)
        "bluetooth" -> Intent(Settings.ACTION_BLUETOOTH_SETTINGS)
        "airplane" -> Intent(Settings.ACTION_AIRPLANE_MODE_SETTINGS)
        else -> Intent(Settings.ACTION_WIRELESS_SETTINGS) // the hotspot lives under network settings
    }.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    private fun hotspotPassword(): String {
        val alphabet = "abcdefghjkmnpqrstuvwxyz23456789"
        val r = java.security.SecureRandom()
        return (1..12).map { alphabet[r.nextInt(alphabet.length)] }.joinToString("")
    }
}
