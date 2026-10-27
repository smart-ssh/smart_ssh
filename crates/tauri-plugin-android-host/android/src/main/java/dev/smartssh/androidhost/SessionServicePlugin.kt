package dev.smartssh.androidhost

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import android.widget.Toast
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class StartServiceArgs {
  var sessions: Int = 0
  var onEvent: Channel? = null
}

@InvokeArg
class SessionCountArgs {
  var sessions: Int = 0
}

@InvokeArg
class WakeLockArgs {
  var held: Boolean = false
}

/**
 * Small Tauri mobile plugin: starts/stops the [SessionService] foreground
 * service and holds the partial wake lock while a command or transfer runs
 * (Spec 0114 §4, ADR 0149). Registered from Rust as `session-service`.
 */
@TauriPlugin(
  permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notification")]
)
class SessionServicePlugin(private val activity: Activity) : Plugin(activity) {
  private var wakeLock: PowerManager.WakeLock? = null
  private val prefs by lazy { activity.getSharedPreferences("session_service", Context.MODE_PRIVATE) }

  private fun notificationsGranted(): Boolean =
    Build.VERSION.SDK_INT < 33 ||
      ContextCompat.checkSelfPermission(activity, Manifest.permission.POST_NOTIFICATIONS) ==
        PackageManager.PERMISSION_GRANTED

  private fun launch(sessions: Int) {
    val intent =
      Intent(activity, SessionService::class.java)
        .setAction(SessionService.ACTION_UPDATE)
        .putExtra(SessionService.EXTRA_SESSIONS, sessions)
    ContextCompat.startForegroundService(activity, intent)
  }

  @Command
  fun startService(invoke: Invoke) {
    val args = invoke.parseArgs(StartServiceArgs::class.java)
    SessionService.eventChannel = args.onEvent
    pendingSessions = args.sessions
    // Ask once, at the first session. After a refusal the hint was shown and
    // we do not ask again; the service runs either way.
    if (!notificationsGranted() && !prefs.getBoolean("hint_shown", false)) {
      requestPermissionForAlias("notification", invoke, "afterNotificationPermission")
    } else {
      launch(args.sessions)
      invoke.resolve()
    }
  }

  private var pendingSessions = 0

  @PermissionCallback
  fun afterNotificationPermission(invoke: Invoke) {
    if (!notificationsGranted()) {
      prefs.edit().putBoolean("hint_shown", true).apply()
      activity.runOnUiThread {
        Toast.makeText(activity, R.string.session_notification_denied_hint, Toast.LENGTH_LONG).show()
      }
    }
    launch(pendingSessions)
    invoke.resolve()
  }

  @Command
  fun updateService(invoke: Invoke) {
    launch(invoke.parseArgs(SessionCountArgs::class.java).sessions)
    invoke.resolve()
  }

  @Command
  fun stopService(invoke: Invoke) {
    SessionService.eventChannel = null
    activity.stopService(Intent(activity, SessionService::class.java))
    applyWakeLock(false)
    invoke.resolve()
  }

  @Command
  fun setWakeLock(invoke: Invoke) {
    applyWakeLock(invoke.parseArgs(WakeLockArgs::class.java).held)
    invoke.resolve()
  }

  private fun applyWakeLock(held: Boolean) {
    if (held) {
      if (wakeLock == null) {
        val pm = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "smartssh:activity")
      }
      if (wakeLock?.isHeld == false) wakeLock?.acquire()
    } else {
      if (wakeLock?.isHeld == true) wakeLock?.release()
    }
  }
}
