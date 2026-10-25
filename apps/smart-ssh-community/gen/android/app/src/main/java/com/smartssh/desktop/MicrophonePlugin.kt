package com.smartssh.desktop

import android.Manifest
import android.app.Activity
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

private const val MICROPHONE = "microphone"

/** Asks for RECORD_AUDIO when voice input starts (Spec 0108, 5.7). */
@TauriPlugin(
  permissions = [Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = MICROPHONE)]
)
class MicrophonePlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun requestMicrophone(invoke: Invoke) {
    if (getPermissionState(MICROPHONE) == PermissionState.GRANTED) {
      resolveState(invoke)
    } else {
      requestPermissionForAlias(MICROPHONE, invoke, "microphoneCallback")
    }
  }

  @PermissionCallback
  fun microphoneCallback(invoke: Invoke) {
    resolveState(invoke)
  }

  private fun resolveState(invoke: Invoke) {
    val result = JSObject()
    result.put("granted", getPermissionState(MICROPHONE) == PermissionState.GRANTED)
    invoke.resolve(result)
  }
}
