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
