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

Drei Punkte, ohne die die App unter Android beim Start abbricht:

1. **Android-Kontext (`ndk-context`) nicht selbst anlegen.** Der Schlüsselbund-Test beim Start (`probe_keychain_availability`) und die Spracheingabe (`cpal`) brauchen den Anwendungskontext. Seit **tao 0.37** legt tao ihn selbst in `onCreate` an (`ndk_context::initialize_android_context`, innerhalb von `super.onCreate`). Eine App-Crate darf ihn **nicht** noch einmal initialisieren (früherer Aufruf `Keyring.initializeNdkContext` aus Issue #795): doppelte Initialisierung lässt die Rust-Seite beim Start paniken. Die Aussage hängt an der tao-Version im `Cargo.lock`; der Test `apps/smart-ssh-community/tests/android_ndk_context.rs` schlägt fehl, wenn tao >= 0.37 ist und ein Kotlin-Aufruf von `initializeNdkContext` oder `io/crates/keyring/Keyring.kt` auftaucht, und ebenso, wenn tao unter 0.37 fällt (dann muss die App den Kontext wieder selbst anlegen).
2. **Android-Teil von Tauri-Plugins.** Die Tauri-CLI schreibt nur die Android-Projekte der *direkten* Abhängigkeiten der App-Crate in `gen/android/tauri.settings.gradle` und `app/tauri.build.gradle.kts`. Jedes Plugin mit Android-Teil, das `app-shell` registriert, muss daher auch direkte Abhängigkeit der App-Crate sein (für `tauri-plugin-fs`, Android-only, reicht ein `[target.'cfg(target_os = "android")'.dependencies]`-Eintrag). Sonst fehlt die Kotlin-Klasse (`com.plugin.fs.FsPlugin`) im APK. Prüfen: `tauri.settings.gradle` nennt `:tauri-plugin-fs`.
3. **Fenster-Insets (Issue #802).** Die App wird randlos gezeichnet (`enableEdgeToEdge()`, unter `targetSdk` 36 ohnehin erzwungen); das Webview deckt den ganzen Bildschirm, und CSS-`env(safe-area-inset-*)` greift im Android-Webview für Systemleisten nicht zuverlässig. Die `MainActivity` jeder App-Crate setzt deshalb nach `super.onCreate` einen `OnApplyWindowInsetsListener` auf `android.R.id.content`, der `systemBars() | displayCutout() | ime()` als Padding anwendet (siehe `applySystemInsets()` in `apps/smart-ssh-community/gen/android/app/src/main/java/com/smartssh/desktop/MainActivity.kt`), ruft `enableEdgeToEdge` mit `SystemBarStyle.dark(Color.TRANSPARENT)` auf (helle Symbole; das App-Design ist immer dunkel) und setzt `android:windowBackground` im App-Theme (`values/themes.xml`, `values-night/themes.xml`) auf die App-Hintergrundfarbe (`app_background`, `#090C14`). Eine andere App-Crate kopiert das; ohne es liegen Kopfzeile und unterste Schaltflächen unter den Systemleisten.

### Checkliste: was eine App-Crate aus `gen/android` braucht

- Kotlin-Klassen `SessionServicePlugin`, `SessionService` und `MicrophonePlugin` unter `com/smartssh/desktop/`.
- Manifest: die Berechtigungen dieser Klassen und der `<service>`-Eintrag für `SessionService`.
- Die von ihnen verwendeten Strings (`values/strings.xml`).
- Direkte Abhängigkeit auf `tauri-plugin-fs` (Punkt 2).
- Die Insets-Behandlung in `MainActivity` (Punkt 3).
- Keinen Aufruf, der den `ndk-context` initialisiert (Punkt 1).
