# Android-Build (technischer Test)

Voraussetzungen: Android SDK und NDK (`ANDROID_HOME`, `NDK_HOME`), Rust-Targets, `cargo-tauri`, Perl (für das mitgebaute OpenSSL von SQLCipher), JDK 17.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
# Nur nötig, wenn apps/smart-ssh-community/gen/android fehlt (es ist eingecheckt;
# Build-Ausgaben darin sind ignoriert):
cd apps/smart-ssh-community && cargo tauri android init --ci
# Umgebung für SQLCipher + OpenSSL (vendored); Beispiel für Linux-Hosts:
export NDK_HOME=$ANDROID_HOME/ndk/<version>
export TOOLCHAIN=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$TOOLCHAIN/aarch64-linux-android26-clang
export AR_aarch64_linux_android=$TOOLCHAIN/llvm-ar
export CC_x86_64_linux_android=$TOOLCHAIN/x86_64-linux-android26-clang
export AR_x86_64_linux_android=$TOOLCHAIN/llvm-ar
# Auf einem reinen Linux-Host zusätzlich (vendored OpenSSL, whisper.cpp, bindgen):
export RANLIB_aarch64_linux_android=$TOOLCHAIN/llvm-ranlib
export RANLIB_x86_64_linux_android=$TOOLCHAIN/llvm-ranlib
export ANDROID_NDK_ROOT=$NDK_HOME   # CMake-Build von whisper.cpp
SYSROOT=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android="--sysroot=$SYSROOT"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_linux_android="--sysroot=$SYSROOT"
# Nur Rust prüfen:
cargo build -p smart-ssh-community --target aarch64-linux-android
cargo build -p smart-ssh-community --target x86_64-linux-android
# APK:
cargo tauri android build --debug --apk --target aarch64 --ci
```

16-KB-Seiten: Android 15+ verlangt für alle `.so` eine LOAD-Ausrichtung von mindestens 0x4000. NDK 27 (`27.3.13750724`, in CI gepinnt) linkt standardmäßig mit 4 KB; `.cargo/config.toml` setzt deshalb je Android-Target `-Wl,-z,max-page-size=16384` (gilt auch für OpenSSL/whisper.cpp, die statisch in die Bibliothek gelinkt werden). CI prüft das im Job `android-build` mit `llvm-readelf -lW` für jede `.so` im APK. Lokal: `$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf -lW <lib>.so | grep LOAD` (letzte Spalte `0x4000`). Wird die NDK-Version geändert, in Workflow und hier gemeinsam.

Mindest-SDK ist 26. Siehe Spec 0114 und ADR 0148.

TLS: Unter Android nutzen die HTTP-Clients (KI-Anbieter, Modell-Download) die eingebauten webpki-Wurzeln statt des Plattform-Verifiers (ADR 0148). Emulator-/Geräteprüfung ist manuell.

## App-Crates, die `app-shell` einbetten (Issue #795)

Zwei Punkte, ohne die die App unter Android beim Start abbricht:

1. **Android-Kontext (`ndk-context`).** Der Schlüsselbund-Test beim Start (`probe_keychain_availability`) und die Spracheingabe (`cpal`) brauchen den Anwendungskontext, bevor ein Webview existiert; Tauri/tao legen ihn nicht an. Die App-Crate bindet ihn deshalb selbst ein: `gen/android/app/src/main/java/io/crates/keyring/Keyring.kt` (Klasse und Paket sind durch den exportierten JNI-Symbolnamen von `android-native-keyring-store` fest vorgegeben; `System.loadLibrary` nennt die App-Bibliothek, hier `smart_ssh_community_lib`) und in `MainActivity.onCreate` **vor** `super.onCreate` der eine Aufruf `Keyring.initializeNdkContext(applicationContext)`. Eine andere App-Crate kopiert beides und passt den Bibliotheksnamen an.
2. **Android-Teil von Tauri-Plugins.** Die Tauri-CLI schreibt nur die Android-Projekte der *direkten* Abhängigkeiten der App-Crate in `gen/android/tauri.settings.gradle` und `app/tauri.build.gradle.kts`. Jedes Plugin mit Android-Teil, das `app-shell` registriert, muss daher auch direkte Abhängigkeit der App-Crate sein (für `tauri-plugin-fs`, Android-only, reicht ein `[target.'cfg(target_os = "android")'.dependencies]`-Eintrag). Sonst fehlt die Kotlin-Klasse (`com.plugin.fs.FsPlugin`) im APK. Prüfen: `tauri.settings.gradle` nennt `:tauri-plugin-fs`.
