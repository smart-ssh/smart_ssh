# ADR 0090 — Drittlizenz-Liste: Verpackung, Quelle der erlaubten Lizenzen, Grenzen der Hand-Prüfung

Status: akzeptiert · Spec: `docs/specs/0099-third-party-licenses.md`

Spec 0099, Abschnitt 5 ("Design") überlässt drei Fragen ausdrücklich dem
Coder: den Weg der Datei ins Paket (A2.3), ob die Release-Matrix selbst
erzeugt oder einen vorgelagerten Job nutzt, und welche devDependencies
Code ins Bundle bringen. Hier stehen die getroffenen Entscheidungen sowie
die Grenzen der Hand-Prüfung für T12.

## 1. Die Datei ist ein statisches Frontend-Asset, kein Tauri-`bundle.resources`

**Entscheidung:** `scripts/generate-third-party-notices.mjs` schreibt nach
`apps/smart-ssh-community/frontend/public/third-party-notices.txt`. Vite
kopiert `public/*` unverändert nach `dist/` (wie bereits `favicon.svg` und
`icons.svg`) — `vite build` nimmt die Datei damit automatisch in den
Webview-Bundle-Inhalt auf, ohne dass `tauri.conf.json` ein
`bundle.resources` bräuchte oder eine neue Tauri-Kommandoschnittstelle
(IPC) entstehen müsste.

**Warum nicht `bundle.resources` + ein eigener Tauri-Command:** Das hätte
eine neue IPC-Schnittstelle gebraucht (Lesen einer Datei aus dem
Ressourcenverzeichnis über `app.path().resource_dir()`), zusätzliche
Angriffsfläche für eine Funktion, die nichts weiter tut als Text
auszuliefern, der ohnehin schon ungeprüft aus dem Webview-Bundle kommt.
Der gewählte Weg nutzt exakt den Mechanismus, über den auch `index.html`,
das gebaute JS/CSS und die Schriftdateien bereits ausgeliefert werden —
A2.3 ("Teil des ausgelieferten App-Pakets, ohne Netzwerk lesbar") gilt für
diese Assets schon heute, ohne dass diese Spec sie geändert hätte.

**Folge für den Ladeweg (A4):** Die Oberfläche lädt die Datei per
`fetch("/third-party-notices.txt")` — derselbe Origin wie der Rest der
Webview, die bestehende CSP (`connect-src 'self' ipc:`) muss dafür nicht
geändert werden (A4.4 gilt bereits durch die Existenz der Regel, nicht
durch eine neue Ausnahme).

## 2. `about.toml` wird zur Laufzeit erzeugt, nie eingecheckt

**Entscheidung:** Das Skript baut aus `deny.toml`s `[licenses] allow`-Array
bei jedem Lauf ein `about.toml` in einem temporären Verzeichnis und löscht
es danach wieder. Es gibt keine eingecheckte `about.toml` im Repo.

**Warum:** A1.5 verlangt eine einzige Quelle für die erlaubten Lizenzen.
Eine eingecheckte `about.toml` mit eigenem `accepted`-Feld wäre eine
zweite, von Hand zu pflegende Liste gewesen — genau der Fall, den A1.5 als
Alternative vorsieht ("oder es gibt eine zweite Liste und ein Test in der
CI scheitert, sobald beide voneinander abweichen"). Die gewählte erste
Alternative ("liest das Skript sie aus `deny.toml`") braucht keinen
Vergleichstest, weil es die zweite Liste gar nicht gibt — ein Entfernen
einer benutzten Lizenz aus `deny.toml` lässt `cargo-about` selbst
abbrechen (Spec, T4, im Bericht mit Gegenbeweis belegt).

## 3. Jede Release-Plattform erzeugt ihre eigene Kopie

**Entscheidung:** Kein vorgelagerter Job, der die Datei einmal erzeugt und
als Artefakt an die drei Plattform-Jobs verteilt — jede der drei Zeilen
der Release-Matrix (`release.yml`) installiert `cargo-about` selbst und
ruft `npm run generate-notices` vor `tauri-action` auf.

**Warum:** Der Inhalt hängt laut Spec, Abschnitt 5, nicht von der
Bauplattform ab ("`cargo-about` sammelt über alle Zielplattformen"). Ein
vorgelagerter Job hätte zusätzlichen Konfigurationsaufwand (Artefakt-
Upload/Download zwischen drei Betriebssystemen, Pfad-/Zeilenende-
Handling) für denselben Inhalt bedeutet, den jede Plattform ohnehin in
unter einer Minute selbst erzeugt.

## 4. T12 — was die Hand-Prüfung in dieser Umgebung zeigen konnte und was nicht

T12 verlangt: Skript fahren, Release-Bundle lokal bauen, die Datei darin
nachweisen, das gebaute Paket **ohne Netz starten und die Liste im
Über-Bereich öffnen**.

**Gezeigt:** `npm run generate-notices` erzeugt die Datei; `vite build`
kopiert sie nach `dist/third-party-notices.txt` (Befehl und Ausgabe im
Bericht) — der Bundle-Inhalt, den `cargo tauri build` anschließend
einpackt, enthält die Datei also nachweislich.

**Nicht gezeigt — `cargo tauri build` blieb in dieser Umgebung zweimal an
derselben Stelle stehen**, unabhängig von dieser Spec: Beide Versuche
(sowie ein dritter, reiner `cargo build --workspace --release` ohne jeden
Tauri-/Frontend-Bezug) scheiterten identisch beim Kompilieren von `sqlx`
mit einem `dlopen`-Fehler auf `libsqlx_macros-*.dylib` ("mis-aligned
LINKEDIT string pool") — derselbe Dateiname, dieselbe Fehlermeldung, auch
nach Löschen der Datei zwischen den Versuchen. Das zeigt: Der Fehler liegt
an dieser lokalen `target/`-Kompilierung (Sandbox-Dateisystem-Eigenheit
oder beschädigter Toolchain-Zustand in diesem Checkout), nicht am Code
dieser Spec — `sqlx` taucht in keinem der Commits dieses Schritts auf, und
derselbe Fehler reproduziert ohne jede Tauri-Beteiligung. Keine
Signing-Umgebungsvariable war gesetzt (die echten Credentials liegen laut
`README_DEV.md` bewusst außerhalb des Repos); das allein hätte `cargo
tauri build` nicht am Kompilieren gehindert, nur an Signierung/Notarisierung
danach.

Damit konnte in diesem Lauf weder das gebaute `.app`-Bundle inspiziert noch
„ohne Netz gestartet und der Über-Bereich geöffnet" werden — das deckt sich
mit dem im Auftrag benannten Rechte-Vorbehalt für den App-Start bei T12.
Der Bericht nennt die genauen Befehle (inkl. der Reproduktion des
`sqlx`-Fehlers) für eine Person mit Display-Zugriff und einer sauberen
`target/`-Kompilierung.
