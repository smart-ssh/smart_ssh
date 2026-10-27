package dev.smartssh.androidhost

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject

/**
 * Foreground service that keeps the process alive while SSH sessions are open
 * (Spec 0114 §4, ADR 0149). The session count comes from the Rust side; the
 * "Disconnect all" action is reported back through [eventChannel].
 */
class SessionService : Service() {
  companion object {
    const val ACTION_UPDATE = "dev.smartssh.androidhost.action.UPDATE"
    const val ACTION_DISCONNECT_ALL = "dev.smartssh.androidhost.action.DISCONNECT_ALL"
    const val EXTRA_SESSIONS = "sessions"
    private const val CHANNEL_ID = "sessions"
    private const val NOTIFICATION_ID = 1

    @Volatile var eventChannel: Channel? = null
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_DISCONNECT_ALL) {
      val channel = eventChannel
      if (channel != null) {
        channel.send(JSObject().put("event", "disconnectAll"))
      } else {
        // The app process was recreated without a session owner: nothing to close.
        stopSelf()
      }
      return START_NOT_STICKY
    }
    val sessions = intent?.getIntExtra(EXTRA_SESSIONS, 0) ?: 0
    if (sessions <= 0) {
      stopSelf()
      return START_NOT_STICKY
    }
    val type =
      if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0
    ServiceCompat.startForeground(this, NOTIFICATION_ID, buildNotification(sessions), type)
    return START_NOT_STICKY
  }

  private fun buildNotification(sessions: Int): Notification {
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26) {
      manager.createNotificationChannel(
        NotificationChannel(
          CHANNEL_ID,
          getString(R.string.session_channel_name),
          NotificationManager.IMPORTANCE_LOW,
        )
      )
    }
    val open =
      PendingIntent.getActivity(
        this,
        0,
        packageManager.getLaunchIntentForPackage(packageName),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
      )
    val disconnectAll =
      PendingIntent.getService(
        this,
        1,
        Intent(this, SessionService::class.java).setAction(ACTION_DISCONNECT_ALL),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
      )
    return NotificationCompat.Builder(this, CHANNEL_ID)
      .setSmallIcon(applicationInfo.icon)
      .setContentTitle(resources.getQuantityString(R.plurals.session_notification_title, sessions, sessions))
      .setContentIntent(open)
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setCategory(NotificationCompat.CATEGORY_SERVICE)
      .addAction(0, getString(R.string.session_notification_disconnect_all), disconnectAll)
      .build()
  }
}
