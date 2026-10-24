# Android-Build (technischer Test)

Voraussetzungen: Android SDK und NDK (`ANDROID_HOME`, `NDK_HOME`), Rust-Targets, `cargo-tauri`, Perl (für das mitgebaute OpenSSL von SQLCipher), JDK 17.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
# Einmalig: erzeugt apps/smart-ssh-community/gen/android
cd apps/smart-ssh-community && cargo tauri android init
# Umgebung für SQLCipher + OpenSSL (vendored); Beispiel für Linux-Hosts:
export NDK_HOME=$ANDROID_HOME/ndk/<version>
export TOOLCHAIN=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$TOOLCHAIN/aarch64-linux-android26-clang
export AR_aarch64_linux_android=$TOOLCHAIN/llvm-ar
export CC_x86_64_linux_android=$TOOLCHAIN/x86_64-linux-android26-clang
export AR_x86_64_linux_android=$TOOLCHAIN/llvm-ar
# Nur Rust prüfen:
cargo build -p smart-ssh-community --target aarch64-linux-android
cargo build -p smart-ssh-community --target x86_64-linux-android
# APK:
cargo tauri android build --debug
```

Mindest-SDK ist 26. Siehe Spec 0114 und ADR 0148.
