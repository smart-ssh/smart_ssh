# Spec: Versions- & Build-Anzeige

Status: Entwurf
Repo: **öffentlich** `smart_ssh` (Einbettung, Log, Über-Dialog, Titelzeile) —
Zwei-Repo-Aspekt bei der Official-Titelzeile prüfen (siehe §5)
Abhängigkeiten: Startup-Logging (0047/0016), Settings-Neustruktur (0050,
"Über"-Kategorie), Titelleiste (0014), Versionierung (0048)

> Macht auf einen Blick sichtbar, **welcher Build genau** läuft — Version +
> eindeutiger Commit-Hash. Zweck: In der Testphase trägt jeder Bug-Report/jedes
> Log automatisch die exakte Kennung, statt sie mühsam zu erfragen. Version
> allein reicht nicht (mehrere Builds können dieselbe Version tragen); erst
> Version + Commit-Hash identifiziert den Stand eindeutig.

## 1. Was angezeigt/geloggt wird

Zwei Angaben, überall im selben Format:
- **Version** aus der maßgeblichen Quelle (`tauri.conf.json`, Spec 0048) —
  z. B. `0.4.1`.
- **Kurzer Commit-Hash** des gebauten Stands — z. B. `a5b3e01`.
- **Format**: `0.4.1 (a5b3e01)`. Optional Edition-Kennung, falls billig:
  `0.4.1 (a5b3e01) · Official` bzw. `· Community`.

## 2. Commit-Hash zur Build-Zeit einbetten (die Kern-Entscheidung)

Der Hash muss beim Bauen ins Binary — die App kann ihn zur Laufzeit nicht
selbst ermitteln (kein Git im Nutzer-System). Ansatz (Coder wählt den
saubersten, begründet):
- **Cargo build-script** (`build.rs`): liest `git rev-parse --short HEAD`
  beim Kompilieren, exponiert es als `env!("…")`-Konstante. Vorteil:
  funktioniert lokal **und** in CI gleich, kein Extra-Schritt.
  **Fallstrick**: In einem Build ohne Git (z. B. aus einem Tarball) muss ein
  Fallback greifen (`"unknown"` statt Build-Abbruch). Und: `build.rs` sollte
  bei Hash-Änderung neu laufen (`cargo:rerun-if-changed` auf `.git/HEAD` bzw.
  die passende Ref) — sonst zeigt ein inkrementeller Build einen veralteten
  Hash.
- **Alternative**: Hash als Umgebungsvariable aus CI/Release-Skript
  (`SMART_SSH_BUILD_HASH`) injizieren. Vorteil: explizit. Nachteil: lokale
  Builds ohne die Variable haben keinen Hash → Fallback nötig, und zwei Wege
  (lokal vs. CI) statt einem.
- Empfehlung: **build.rs mit Git-Abfrage + `"unknown"`-Fallback + korrektem
  rerun-Trigger** — ein Weg, lokal wie CI. Der Coder bestätigt, dass es mit
  dem macOS-Release-Skript und der `official.yml` zusammenspielt (beide
  bauen aus einem Git-Checkout, also ist der Hash verfügbar).

Die Version wird weiterhin aus der bestehenden Quelle gelesen (nicht neu
eingebettet) — nur der Hash kommt hinzu.

## 3. Wo es erscheint

### 3.1 Log (wichtigste Ebene)
Die erste Startzeile (Spec 0047 B1) enthält bereits OS/Datenpfad —
**Version + Hash ergänzen/prüfen**: Steht die Version schon drin? Falls ja,
Hash dazu; falls nein, beides. So trägt jedes an einen Bug-Report angehängte
Log automatisch die exakte Build-Kennung.

### 3.2 Über-Dialog / Settings-"Über"-Kategorie (für den Tester)
Version + Hash **sichtbar und kopierbar** (der Tester soll es in den
Bug-Report kopieren können — ein Klick-zum-Kopieren wäre ideal, mind. aber
markierbarer Text). Natürlicher Ort: die "Über"-Kategorie der neuen
zweispaltigen Settings (Spec 0050), wo auch NOTICE/Drittlizenzen hingehören
(Release-Gate F).

### 3.3 Titelzeile (nur 0.x-Phase, für Stefans Multi-Build-Komfort)
Die Fenstertitelzeile zeigt zusätzlich Version (+ ggf. Hash), z. B.
`Smart SSH 0.4.1 (a5b3e01) — Early Access`. Nutzen: Wer mehrere Builds
parallel testet (Cross-Plattform), sieht sofort welcher läuft.
- **Bewusst als 0.x-Merkmal**: Für die spätere 1.0 will man das evtl.
  reduzieren (nur "Smart SSH", ohne Version/Hash in der Leiste). Deshalb so
  bauen, dass es leicht abschaltbar/anpassbar ist (z. B. an die
  Early-Access-Kennzeichnung gekoppelt), nicht fest verdrahtet.
- Berührt die custom Titelleiste (Spec 0014) — die Version ist Text *in* der
  Leiste, nicht die Fenster-Buttons; sollte mit dem Windows/Linux-
  Titelleisten-Stand (0049) verträglich sein (prüfen, dass der Text die
  Drag-Region/die Buttons nicht stört).

## 4. Nicht-Ziele

- Keine echte fortlaufende "Build-Nummer" (CI-Zähler o. Ä.) — der Commit-Hash
  ist die eindeutige Build-Kennung und braucht keine zusätzliche Nummer.
- Kein Auto-Update-Bezug, kიne Telemetrie — reine lokale Anzeige.

## 5. Zwei-Repo-Aspekt

- Die **Einbettung (build.rs), das Log und der Über-Dialog** sind
  öffentlicher Kern — die Community Edition profitiert genauso.
- Die **Titelzeile**: Falls die Official-Edition einen eigenen Titel-/Shell-
  Aufbau hat, muss die Version-in-Titelzeile ggf. auch dort gesetzt werden.
  Prüfen und melden — wenn die Titelzeile rein öffentlich gerendert wird,
  reicht der öffentliche Teil; sonst privater Folgeschritt (die Edition-
  Kennung "Official" käme ohnehin von der privaten Seite).
- Die **Edition-Kennung** (`· Official`/`· Community`) kommt aus dem
  `Edition`-Feld des `Wiring` (Spec 0038) — die kennt der öffentliche Code
  bereits als Enum; nur der konkrete Wert wird pro Binary gesetzt.

## 6. Testbarkeit

- build.rs: Ein Build mit Git liefert einen Hash; ein simuliertes Build ohne
  Git-Zugriff liefert `"unknown"` statt Fehler.
- Log: erste Zeile enthält Version + Hash (Format-Assertion).
- Über-Dialog: zeigt Version + Hash, Text ist selektierbar/kopierbar.
- Titelzeile: enthält die Version; abschaltbar-Mechanismus greift.

## 7. Reihenfolge

1. build.rs-Einbettung + `"unknown"`-Fallback (die Grundlage).
2. Log ergänzen (0047-Startzeile).
3. Über-Dialog (Settings-"Über").
4. Titelzeile (mit Abschalt-Kopplung an Early-Access).
5. Zwei-Repo-Prüfung Titelzeile/Edition-Kennung — melden, ob privat nötig.
