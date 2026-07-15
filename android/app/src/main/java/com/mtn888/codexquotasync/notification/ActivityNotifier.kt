package com.mtn888.codexquotasync.notification

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import com.mtn888.codexquotasync.R
import com.mtn888.codexquotasync.data.ActivityTransition
import com.mtn888.codexquotasync.ui.MainActivity

object ActivityNotifier {
    private const val CHANNEL_ID = "codex_activity_changes_v1"
    private const val NOTIFICATION_ID = 4201
    private const val OPEN_REQUEST_CODE = 4202

    fun ensureChannel(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.notification_channel_name),
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                description = context.getString(R.string.notification_channel_description)
                enableVibration(true)
            },
        )
    }

    fun notify(context: Context, transition: ActivityTransition) {
        ensureChannel(context)
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return

        val title = when {
            transition.completed > 0 && transition.pendingIncrease > 0 ->
                context.getString(R.string.notification_title_combined)
            transition.pendingIncrease > 0 -> context.getString(R.string.notification_title_pending)
            else -> context.getString(R.string.notification_title_completed)
        }
        val message = when {
            transition.completed > 0 && transition.pendingIncrease > 0 -> context.getString(
                R.string.notification_message_combined,
                transition.completed,
                transition.pendingIncrease,
                transition.currentExecuting,
                transition.currentPending,
            )
            transition.pendingIncrease > 0 -> context.getString(
                R.string.notification_message_pending,
                transition.previousPending,
                transition.currentPending,
            )
            else -> context.getString(
                R.string.notification_message_completed,
                transition.previousExecuting,
                transition.currentExecuting,
            )
        }
        val openStatus = Intent(context, MainActivity::class.java).apply {
            action = "com.mtn888.codexquotasync.action.OPEN_ACTIVITY_NOTIFICATION"
            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val contentIntent = PendingIntent.getActivity(
            context,
            OPEN_REQUEST_CODE,
            openStatus,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(message)
            .setStyle(Notification.BigTextStyle().bigText(message))
            .setContentIntent(contentIntent)
            .setAutoCancel(true)
            .setCategory(Notification.CATEGORY_REMINDER)
            .setOnlyAlertOnce(false)
            .build()
        context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
    }
}
