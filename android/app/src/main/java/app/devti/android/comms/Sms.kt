package app.devti.android.comms

import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.ContactsContract
import android.provider.Telephony
import android.telephony.SmsManager
import androidx.core.content.ContextCompat
import app.devti.android.DevtiApp
import app.devti.android.core.state.SmsMessage
import app.devti.android.core.state.SmsThread

/**
 * The phone's texts for the computer: conversations and messages read from Android's SMS
 * store, texts sent with SmsManager (so they come from this phone's number), and new
 * texts forwarded as they arrive. Needs the SMS permissions the user grants on the
 * Home screen; without them every call returns nothing and says why.
 */
class SmsBridge(private val context: Context) {
    private val names = HashMap<String, String>()

    fun allowed(): Boolean = listOf(Manifest.permission.READ_SMS, Manifest.permission.SEND_SMS).all {
        ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
    }

    /** Newest conversations first (scans the latest 5 000 texts). */
    fun threads(): List<SmsThread> {
        if (!allowed()) return emptyList()
        val out = LinkedHashMap<String, SmsThread>()
        val unread = HashMap<String, Int>()
        context.contentResolver.query(
            Telephony.Sms.CONTENT_URI,
            arrayOf(Telephony.Sms.THREAD_ID, Telephony.Sms.ADDRESS, Telephony.Sms.BODY, Telephony.Sms.DATE, Telephony.Sms.READ, Telephony.Sms.TYPE),
            null,
            null,
            "${Telephony.Sms.DATE} DESC",
        )?.use { c ->
            var rows = 0
            while (c.moveToNext() && rows++ < 5000 && out.size < 200) {
                val thread = c.getString(0) ?: continue
                val address = c.getString(1).orEmpty()
                if (c.getInt(4) == 0 && c.getInt(5) == Telephony.Sms.MESSAGE_TYPE_INBOX) unread[thread] = (unread[thread] ?: 0) + 1
                if (thread in out) continue
                out[thread] = SmsThread(
                    id = thread,
                    addresses = listOf(address),
                    names = listOfNotNull(nameOf(address)),
                    snippet = c.getString(2).orEmpty().take(200),
                    ts = c.getLong(3),
                )
            }
        }
        return out.values.map { it.copy(unread = unread[it.id] ?: 0) }
    }

    /** The latest 200 messages of a conversation, oldest first. */
    fun messages(thread: String): List<SmsMessage> {
        if (!allowed()) return emptyList()
        val out = ArrayList<SmsMessage>()
        context.contentResolver.query(
            Telephony.Sms.CONTENT_URI,
            arrayOf(Telephony.Sms._ID, Telephony.Sms.ADDRESS, Telephony.Sms.BODY, Telephony.Sms.DATE, Telephony.Sms.TYPE),
            "${Telephony.Sms.THREAD_ID} = ?",
            arrayOf(thread),
            "${Telephony.Sms.DATE} DESC",
        )?.use { c ->
            while (c.moveToNext() && out.size < 200) {
                val address = c.getString(1).orEmpty()
                val outgoing = c.getInt(4) != Telephony.Sms.MESSAGE_TYPE_INBOX
                out += SmsMessage(
                    id = c.getString(0),
                    thread = thread,
                    address = address,
                    name = if (outgoing) "" else nameOf(address).orEmpty(),
                    body = c.getString(2).orEmpty(),
                    ts = c.getLong(3),
                    outgoing = outgoing,
                )
            }
        }
        return out.reversed()
    }

    /** Sends a text; returns null on success or a sentence saying why not. */
    fun send(to: List<String>, text: String): String? {
        if (!allowed()) return "Allow SMS for Devti on the phone first (Home → Calls and messages)."
        val sms = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            context.getSystemService(SmsManager::class.java)
        } else {
            @Suppress("DEPRECATION")
            SmsManager.getDefault()
        } ?: return "This phone can't send texts."
        return runCatching {
            for (number in to) {
                val parts = sms.divideMessage(text)
                if (parts.size == 1) sms.sendTextMessage(number, null, text, null, null)
                else sms.sendMultipartTextMessage(number, null, parts, null, null)
            }
            null
        }.getOrElse { "The phone couldn't send it: ${it.message ?: "unknown error"}" }
    }

    /** Android's conversation id for a number, matching [threads] (created if new). */
    fun threadOf(address: String): String = runCatching {
        if (!allowed()) null else Telephony.Threads.getOrCreateThreadId(context, address).toString()
    }.getOrNull() ?: "addr:$address"

    /** The text as the computer should see it once sent, so the conversation updates. */
    fun sent(to: String, text: String): SmsMessage {
        val now = System.currentTimeMillis()
        return SmsMessage(id = "out-$now-${to.hashCode()}", thread = threadOf(to), address = to, body = text, ts = now, outgoing = true)
    }

    fun nameOf(address: String): String? {
        if (address.isBlank()) return null
        names[address]?.let { return it.ifEmpty { null } }
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.READ_CONTACTS) == PackageManager.PERMISSION_GRANTED
        val name = if (!granted) null else runCatching {
            val uri = Uri.withAppendedPath(ContactsContract.PhoneLookup.CONTENT_FILTER_URI, Uri.encode(address))
            context.contentResolver.query(uri, arrayOf(ContactsContract.PhoneLookup.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull()
        names[address] = name.orEmpty()
        return name
    }
}

/** New texts, forwarded to connected computers as they arrive. */
class SmsReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
        val core = (context.applicationContext as DevtiApp).core
        core.start()
        val parts = Telephony.Sms.Intents.getMessagesFromIntent(intent) ?: return
        val address = parts.firstOrNull()?.originatingAddress.orEmpty()
        val body = parts.joinToString("") { it.messageBody.orEmpty() }
        val ts = parts.firstOrNull()?.timestampMillis ?: System.currentTimeMillis()
        core.publishSms(
            SmsMessage(
                id = "in-$ts-${address.hashCode()}",
                thread = core.sms.threadOf(address),
                address = address,
                name = core.sms.nameOf(address).orEmpty(),
                body = body,
                ts = ts,
                outgoing = false,
            ),
        )
    }
}
