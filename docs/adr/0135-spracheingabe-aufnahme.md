# ADR 0135 — Spracheingabe: Aufnahme und lokale Erkennung

Status: angenommen · Issue: #417 · Spec 0108

## Kontext

Die Spracheingabe nimmt Audio vom Mikrofon auf und erkennt es lokal mit Whisper. Das Issue lässt offen, wie die Aufnahme gegen Geräte abgegrenzt wird, wie die Höchstdauer durchgesetzt wird und welche Lizenzen und Systempakete die neuen Abhängigkeiten mitbringen.

## Entscheidungen

1. **Zwei Traits an der Kante.** `AudioSource`/`AudioCapture` (Gerät) und `Transcriber` (Engine) sind Traits; der Aufnahmeablauf kennt nur sie. Tests speisen eine mitgelieferte kurze WAV-Datei über eine Test-Quelle ein und prüfen so denselben Pfad ohne Gerät und ohne Modell. Die echte Engine hat einen ignorierten Test, der mit einem heruntergeladenen Modell läuft (`SMART_SSH_WHISPER_MODEL`).
2. **Das Gerät wird erst im Startbefehl geöffnet.** Ohne bereites Modell scheitert `start` vor dem Öffnen. Der cpal-Stream lebt auf einem eigenen Thread (er ist nicht `Send`) und endet mit `finish`, Abbruch oder Drop.
3. **Höchstdauer doppelt abgesichert.** Der Puffer der Aufnahme ist auf die Höchstdauer begrenzt (Speicher bleibt beschränkt), und ein Zeitgeber beendet die Aufnahme und startet die Erkennung. Das Ergebnis wartet auf den nächsten `stop`-Aufruf; ein Ereignis `finished` meldet es. Der Text steht nie in einem Ereignis, damit er genau einmal ausgeliefert wird.
4. **Erkennung auf dem Blocking-Pool.** Das Laden des Modells und die Inferenz laufen in `spawn_blocking`; das Modell wird je Erkennung geladen und danach freigegeben (Speicher nur während der Erkennung, einfacher Zustand). Preis: ein paar Sekunden Ladezeit bei `small`.
5. **Nur CPU.** Keine GPU-Funktionen (Metal u. a.); das wäre ein Merkmal mit eigener Build-Konfiguration je Plattform.
6. **Sprache.** Einstellung `auto` oder ein von Whisper bekannter Code; ein unbekannter Code wird beim Setzen abgelehnt, ein unbrauchbarer gespeicherter Wert fällt auf `auto` zurück.
7. **Lizenz.** `whisper-rs` und `whisper-rs-sys` stehen unter der Unlicense (Verzicht zugunsten der Allgemeinheit, permissiver als MIT). Statt die Unlicense allgemein zu erlauben, gilt sie als Ausnahme nur für diese beiden Crates in `deny.toml`.
8. **Systempakete.** whisper.cpp braucht cmake und libclang (bindgen), cpal unter Linux libasound2-dev. Die CI-Linux-Jobs erhalten sie über einen Workflow-Vorschlag.
9. **macOS.** `NSMicrophoneUsageDescription` in der von Tauri eingemischten `Info.plist` und das Entitlement `com.apple.security.device.audio-input`, beides nur für die Spracheingabe.

## Konsequenzen

- Die App enthält whisper.cpp; Build-Zeit und Binärgröße wachsen.
- Es gibt keine Netzwerkverbindung durch diese Funktion; `docs/netzwerkverbindungen.md` bleibt unverändert.
