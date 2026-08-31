# 0019-custom-titlebar-decoration-plugin

## Status
Akzeptiert

## Kontext

`docs/specs/0014-custom-titlebar.md` beschreibt die Ablösung der nativen Titelleiste durch eine ins App-Design integrierte Kopfzeile auf allen Zielplattformen (macOS, Windows, Linux), wobei die nativen Fenster-Bedienelemente (macOS-Ampel, Windows Snap-Layout-fähige Minimieren/Maximieren/Schließen-Buttons, Linux-Controls) erhalten bleiben müssen.

Die Spec nannte als technologische Basis `tauri-plugin-decoration` bzw. das verwandte `tauri-plugin-decorum`, mit der Vorgabe zu prüfen, welches der Plugins aktuell die gepflegte und tragfähige Wahl für Tauri v2 ist.

## Entscheidungen

**1. Wahl von `tauri-plugin-decoration` (crates.io, Version 3.0.5) anstelle von `tauri-plugin-decorum`.**
Ein Evaluierungs-Build mit `tauri-plugin-decorum` (v0.1.6, Stand 2024) scheiterte unter modernen Rust-Toolchains (Rust 1.88+) und aktuellem macOS SDK an inkompatiblen `objc`/`cocoa`-Bindings (`error[E0277]: the trait bound c_void: Message is not satisfied`). Das Plugin wird vom ursprünglichen Autor nicht mehr aktiv gepflegt.
`tauri-plugin-decoration` (v3.0.5) ist aktiv gepflegt, explizit für Tauri 2.10+ und Rust 1.88+ ausgelegt und bietet:
- Vollständige macOS AppKit-Unterstützung für Overlay-Titelleisten und asynchrone Ampel-Positionierung via `set_traffic_lights_inset(x, y)`.
- Windows 10/11 Fenster-Buttons inkl. Unterstützung für das native Windows 11 Snap-Layout-Flyout über HTML/CSS-Hit-Targets.
- Wayland-kompatible Controls auf Linux.
- Dynamische Bereitstellung gemessener Clearances über CSS-Variablen (`--tauri-plugin-decoration-left-clearance` und `--tauri-plugin-decoration-right-clearance`).

**2. API-Anpassung: `activate_decoration()` und `set_traffic_lights_inset()` im Setup- & Command-Pfad.**
Während Spec 0014 noch den Methodennamen `create_overlay_titlebar()` aus `decorum` skizzierte, nutzt `tauri-plugin-decoration` das asynchrone `WebviewWindowExt::activate_decoration()`. Die Initialisierung wird im `setup`-Hook von Tauri sowie über den registrierten `create_overlay_titlebar`-Command abgewickelt, der auf macOS zusätzlich `set_traffic_lights_inset(12.0, 16.0)` anwendet.

**3. Plattformweiche im Frontend `<AppHeader />` mit Fallback-Mechanismus.**
Das Frontend nutzt zur Bestimmung des linken/rechten Header-Paddings den Backend-Command `get_platform` in Kombination mit `navigator.userAgent`-Fallback sowie den vom Plugin publizierten CSS-Clearances (`max(78px, var(--tauri-plugin-decoration-left-clearance, 78px))` auf macOS, `max(140px, var(--tauri-plugin-decoration-right-clearance, 140px))` auf Windows/Linux). Die gesamte nicht-interaktive Fläche ist mit `data-tauri-drag-region` markiert.

## Konsequenzen

**Positiv:**
- Das Projekt baut und linkt fehlerfrei auf modernen Plattformen und Toolchains ohne veraltete C-Bindings.
- Native Plattform-Interaktionen (macOS Traffic Light Hover/Click, Windows 11 Snap Layout) bleiben 100% nativ erhalten, ohne Nachbau im Webview.
- Der Header ist strukturell vorbereitet für zukünftige Session-Tabs, ohne dass die Drag- und Clearance-Logik umgebaut werden muss.

**Negativ / Trade-off:**
- Zusätzliche Abhängigkeit zu `tauri-plugin-decoration` und den zugehörigen Fenster-Capabilities (`decoration:default`, `core:window:allow-start-dragging`, `core:window:allow-internal-toggle-maximize`).
- Die Content Security Policy (`csp` in `tauri.conf.json`) muss Style-Origins für das Plugin (`tauri-plugin-decoration:`, `http://tauri-plugin-decoration.localhost`) erlauben.
