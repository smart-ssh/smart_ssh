package com.smartssh.desktop

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import io.crates.keyring.Keyring

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // Issue #795: the ndk-context must exist before the Rust side starts
    // (credential store probe, audio input). One call; copy it into the
    // MainActivity of any app crate that embeds `app-shell`.
    Keyring.initializeNdkContext(applicationContext)
    super.onCreate(savedInstanceState)
  }
}
