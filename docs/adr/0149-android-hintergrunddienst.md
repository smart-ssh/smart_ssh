# ADR 0149 — Android: Vordergrunddienst für offene Sitzungen

Status: angenommen · Issue: #677 · Spec 0114 §4

## Kontext

Android beendet den Prozess einer App im Hintergrund schnell; offene SSH-Sitzungen gingen verloren. Sie sollen weiterlaufen.

## Entscheidungen

1. **Dienst-Typ `specialUse`.** `dataSync` hat ab Android 15 ein Zeitlimit (rund 6 Stunden je 24 Stunden) und würde Sitzungen nach Stunden beenden; `connectedDevice` verlangt Bluetooth-/USB-Berechtigungen, die es hier nicht gibt; `remoteMessaging` passt nicht zu interaktiven Shells. `specialUse` hat kein Limit und verlangt eine Begründung im Manifest (`PROPERTY_SPECIAL_USE_FGS_SUBTYPE`), die dort steht.
2. **Entscheidungslogik ohne Plattform.** `app_logic::background_keepalive` entscheidet (erste Sitzung → starten, Zahl ändert sich → aktualisieren, letzte weg → stoppen; Wake-Lock nur bei laufendem Befehl/Transfer) und ist mit einem Mock testbar. Android implementiert nur das Trait `ForegroundService`; auf anderen Plattformen ist nichts installiert und alles ein No-op.
3. **Sitzungszahl** kommt aus einem `watch`-Kanal im `SessionManager` (`insert`/`remove`), nicht aus einzelnen Aufrufstellen.
4. **Wake-Lock-Marker als Prozess-Singleton.** Befehlsausführung und Dateiübertragung halten eine `ActivityGuard` (`background_activity()`), die ohne installierten Controller nichts tut. Ein Singleton, weil beide Stellen sonst einen weiteren Parameter durch viele Aufrufer bräuchten; die Guard hat keine Wirkung außerhalb von Android.
5. **Kleines Tauri-Plugin in Kotlin** direkt im eingecheckten `gen/android`-Projekt (`SessionServicePlugin`, `SessionService`), von Rust zur Laufzeit nach dem Entsperren registriert. Kein eigenes Gradle-Modul: das Plugin hat keine JS-Seite und keine Rechte-Datei. Rust-Aufrufe ans Plugin laufen nacheinander auf einem eigenen Thread (der Aufruf blockiert).
6. **„Disconnect all“** meldet der Dienst über einen Tauri-`Channel` an Rust, das dieselbe `disconnect`-Funktion wie ein Tab-Schließen je Sitzung aufruft (kein zweiter Trennpfad).
7. **Hinweis bei verweigerter Berechtigung** ist ein nativer Android-Toast mit lokalisierbarer Ressource; er und das „schon gefragt“-Merkmal liegen in Android-`SharedPreferences`. So braucht es keinen neuen Frontend-Text.
8. **Kein Datenmodell, kein Schema.** Der Keepalive bleibt bei 15 Sekunden.
