package app.devti.android.comms

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.app.NotificationCompat
import app.devti.android.R
import app.devti.android.continuity.Alerts
import app.devti.android.continuity.Pure
import app.devti.android.core.state.HandoffItem

/** "Continue on phone": a link (at a media position) or text from a computer. */
object Handoff {
    private const val ID = 5100

    fun show(context: Context, fromName: String, item: HandoffItem, clipboard: (String) -> Unit) {
        if (item.url.isNotEmpty()) {
            val open = Intent(Intent.ACTION_VIEW, Uri.parse(Pure.mediaUrl(item.url, item.positionMs))).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            // Android doesn't let apps open things from the background; a tap does.
            runCatching { context.startActivity(open) }
            Alerts.post(
                context,
                ID,
                NotificationCompat.Builder(context, Alerts.ALERTS)
                    .setSmallIcon(R.drawable.ic_stat_devti)
                    .setContentTitle("Continue from $fromName")
                    .setContentText(item.title.ifBlank { item.url })
                    .setPriority(NotificationCompat.PRIORITY_HIGH)
                    .setAutoCancel(true)
                    .setContentIntent(PendingIntent.getActivity(context, ID, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)),
            )
        } else if (item.text.isNotEmpty()) {
            clipboard(item.text)
        }
    }
}
