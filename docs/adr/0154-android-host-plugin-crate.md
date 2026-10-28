# ADR 0154 — Android code of the core ships as a Tauri plugin crate

## Status

Accepted · Issue: #888 · Spec 0114 §4

## Context

`app-shell` registers Kotlin plugins (the foreground session service and the microphone permission request) that it did not ship. The Kotlin classes, their manifest entries, strings and the `lifecycle-process` dependency lived in the community app's `gen/android` tree, and every other app crate embedding `app-shell` had to copy them (Issue #795).

## Decision

1. Android code that belongs to the core's Rust code ships as a Tauri plugin crate with its own Android library module. The first one is `crates/tauri-plugin-android-host` (library namespace `dev.smartssh.androidhost`): `SessionService`, `SessionServicePlugin`, `MicrophonePlugin`, their permissions, the `<service>` entry and the strings. The Tauri CLI adds the library to the app's Gradle build, Android merges the manifest and resources.
2. The embedding app crate lists the plugin crate as a direct dependency (the CLI only reads direct dependencies, Issue #795). Nothing is copied.
3. Embedding apps keep only their identity in `gen/android`: namespace and application id, `MainActivity`, label, icons.
4. Two plugin constructors (`session_service()`, `microphone()`) instead of one plugin: the microphone plugin keeps its place in the after-unlock plugin list, the session service keeps its own lifecycle in `android_service::start`. Both are empty plugins on non-Android targets.
5. The notification small icon is the host app's icon (`applicationInfo.icon`) read at runtime, so no resource of the app is referenced from the library.
6. CI verifies the result in the built APK (`scripts/check-android-apk-manifest.sh`) and that the generated `tauri.settings.gradle` lists the library.

## Consequences

Behaviour is unchanged. The service's intent actions now use the library namespace as prefix; they are internal. A new Android-side feature of the core goes into this crate, not into an app's `gen/android` tree.
