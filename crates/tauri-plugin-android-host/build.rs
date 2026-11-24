// The Kotlin plugins are only called from Rust (`run_mobile_plugin`), so no
// webview commands are declared; the build script only wires up `android/`.
fn main() {
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .build();
}
