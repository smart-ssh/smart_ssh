package com.smartssh.desktop

import android.content.res.Configuration
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import io.crates.keyring.Keyring

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Issues #802, #956: transparent bars over the window background; the icon
    // contrast and the background follow the appearance setting. The last
    // choice is stored natively so a cold start in Light shows no dark flash.
    applyAppearance(resolveAppearance(storedChoice()))
    // Issue #795: the ndk-context must exist before the Rust side starts
    // (credential store probe, audio input). One call; copy it into the
    // MainActivity of any app crate that embeds `app-shell`.
    Keyring.initializeNdkContext(applicationContext)
    super.onCreate(savedInstanceState)
    applySystemInsets()
    // Window exists now; the first frame is not drawn yet.
    window.setBackgroundDrawable(ColorDrawable(backgroundFor(resolveAppearance(storedChoice()))))
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.addJavascriptInterface(AppearanceBridge(), "SmartSshAppearance")
  }

  /**
   * Issue #956: the frontend hands over the appearance choice and the theme it
   * resolved. Only styles the system bars and the window background.
   */
  inner class AppearanceBridge {
    @JavascriptInterface
    fun set(choice: String, resolved: String) {
      if (choice !in CHOICES || resolved !in THEMES) return
      getSharedPreferences(PREFS, MODE_PRIVATE).edit().putString(KEY_CHOICE, choice).apply()
      runOnUiThread {
        applyAppearance(resolved == "dark")
        window.setBackgroundDrawable(ColorDrawable(backgroundFor(resolved == "dark")))
      }
    }
  }

  private fun storedChoice(): String =
    getSharedPreferences(PREFS, MODE_PRIVATE).getString(KEY_CHOICE, "system") ?: "system"

  /** true = dark. "system" follows the OS setting. */
  private fun resolveAppearance(choice: String): Boolean =
    when (choice) {
      "light" -> false
      "dark" -> true
      else ->
        (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) !=
          Configuration.UI_MODE_NIGHT_NO
    }

  private fun backgroundFor(dark: Boolean): Int =
    if (dark) COLOR_DARK_BACKGROUND else COLOR_LIGHT_BACKGROUND

  private fun applyAppearance(dark: Boolean) {
    // Dark app: light icons. Light app: dark icons.
    val style =
      if (dark) SystemBarStyle.dark(Color.TRANSPARENT)
      else SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
    enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
  }

  private companion object {
    const val PREFS = "appearance"
    const val KEY_CHOICE = "choice"
    val CHOICES = setOf("system", "light", "dark")
    val THEMES = setOf("light", "dark")
    // `--surface-sunken` of theme/dark.css and theme/light.css.
    val COLOR_DARK_BACKGROUND = 0xFF090C14.toInt()
    val COLOR_LIGHT_BACKGROUND = 0xFFF4F6FA.toInt()
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
