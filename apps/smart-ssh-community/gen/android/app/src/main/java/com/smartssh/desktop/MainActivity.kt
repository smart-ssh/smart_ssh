package com.smartssh.desktop

import android.graphics.Color
import android.os.Bundle
import android.view.View
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Issue #802: the app theme is always dark (`--color-slate-950`), so the
    // bars are transparent over the dark window background with light icons,
    // independent of the system theme.
    val dark = SystemBarStyle.dark(Color.TRANSPARENT)
    enableEdgeToEdge(statusBarStyle = dark, navigationBarStyle = dark)
    // The ndk-context is created by tao (>= 0.37) inside `super.onCreate`; do
    // not initialize it here again (see docs/android-build.md).
    super.onCreate(savedInstanceState)
    applySystemInsets()
  }

  /**
   * Issue #802: the WebView is drawn edge to edge (enforced on Android 15+),
   * so pad its container by the system bars, display cutout and on-screen
   * keyboard. The padding area shows the window background (themes.xml), which
   * matches the app background. Copy into the MainActivity of any app crate
   * that embeds `app-shell`.
   */
  private fun applySystemInsets() {
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val types =
        WindowInsetsCompat.Type.systemBars() or
          WindowInsetsCompat.Type.displayCutout() or
          WindowInsetsCompat.Type.ime()
      val bars = insets.getInsets(types)
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)
  }
}
