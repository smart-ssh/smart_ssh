package io.crates.keyring

import android.content.Context

// Binding for the JNI function exported by `android-native-keyring-store`
// (linked into the app library). The class and package name are fixed by the
// exported symbol name. Issue #795, docs/android-build.md.
class Keyring {
  companion object {
    init {
      // The store crate is part of the app library, so that one library is
      // loaded here (Tauri loads the same one).
      System.loadLibrary("smart_ssh_community_lib")
    }

    external fun initializeNdkContext(context: Context)
  }
}
