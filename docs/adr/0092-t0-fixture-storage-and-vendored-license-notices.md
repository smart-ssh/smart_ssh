# ADR 0092 — T0-Fixture als eingecheckte Binärdatei; vendorte Lizenztexte explizit zugeordnet

Status: akzeptiert · Spec: `docs/specs/0101-database-encryption.md`

Spec 0101 benennt T0 ("Fixture, vor A1") und A1 ("die erzeugten
Drittlizenzen enthalten die Lizenztexte von SQLCipher und OpenSSL") als
Ergebnis, überlässt aber den Weg dorthin dem Coder. Zwei Entscheidungen
dazu aus Commit 1 und Commit 2 dieser Spec.

## 1. Die T0-Fixture ist eine eingecheckte Binärdatei, keine zur Laufzeit erzeugte

**Entscheidung:** `crates/persistence-sqlite/tests/fixtures/t0-pre-sqlcipher.sqlite3`
wird einmalig von einem `#[ignore]`-markierten Test erzeugt und als
Binärdatei committet. Ein zweiter, normal laufender Test prüft ihren
Inhalt (14 Migrationen, die Marker aus Spec 0101 §7, mit dem festen
Test-Schlüssel entschlüsselbarer Chatinhalt) an einer Kopie, nie am
Original.

**Warum nicht zur Laufzeit erzeugen:** T0 soll eine Datenbank sein, die
der heutige, noch nicht an SQLCipher gebundene SQLite-Build geschrieben
hat (Grundlage für T4–T6, die die Umwandlung einer **bestehenden**
Installation prüfen). Ab Commit 2 dieser Spec baut dieselbe Crate
dauerhaft gegen SQLCipher — danach gibt es keinen "heutigen,
nicht-SQLCipher"-Build mehr, gegen den sich diese Datei zur Laufzeit neu
erzeugen ließe (die mitgebaute SQLite-Version sinkt dabei zusätzlich von
3.51.3 auf 3.50.4, gemessen in der Beilage zu BL-0314). Der einzige
Zeitpunkt, an dem diese Datei ehrlich entstehen kann, ist **vor** Commit
2 — danach bleibt nur noch, sie aufzuheben.

**Schutz gegen versehentliches Überschreiben:** Der Generator bricht ab,
wenn die Zieldatei schon existiert. Ohne diesen Schutz würde ein künftiges
pauschales `--ignored`-Sammelausführen (z. B. ein CI-Job, der irgendwann
alle ignorierten Tests laufen lässt) die eingefrorene Datei stillschweigend
mit dem Stand eines späteren, SQLCipher-gebundenen Builds überschreiben —
und T4–T6 würden dann nichts mehr über eine **bestehende** Installation
aussagen, ohne dass das auffiele.

**Konsequenz:** Eine Binärdatei (~150 KB) liegt im öffentlichen Repo. Das
ist die akzeptierte Kehrseite dafür, dass T4–T6 (Commit 4) tatsächlich
gegen eine Datei prüfen, die der heutige, ungecipherte Build geschrieben
hat, statt gegen eine Attrappe, die diese Eigenschaft nur behauptet.

## 2. Vendorte Lizenztexte werden explizit zugeordnet, nicht automatisch gefunden

**Entscheidung:** `scripts/generate-third-party-notices.mjs` bekommt eine
feste Liste (`VENDORED_LICENSE_FILES`) von (Crate, Datei-Pfad im
Crate-Verzeichnis, Label) — aktuell SQLCipher in `libsqlite3-sys`
(`sqlcipher/LICENSE`) und die vendorte OpenSSL-Quelle in `openssl-src`
(`openssl/LICENSE.txt`). Beide werden wie die bestehenden `NOTICE*`-Dateien
in den Abschnitt "Hinweisdateien" der generierten Ausgabe aufgenommen.
Fehlt eines der beiden Crates im Abhängigkeitsbaum, bricht der Lauf ab,
statt den Lizenztext stillschweigend wegzulassen.

**Warum nicht der bestehende automatische Scan:** Der vorhandene
`/^notice/i`-Scan liest nur das **Wurzelverzeichnis** jedes Crates.
Gemessen: SQLCipher-Lizenz und die vendorte OpenSSL-Lizenz liegen beide
eine Ebene tiefer (`sqlcipher/`, `openssl/`) — ohne diese Zuordnung
enthielt die generierte Ausgabe weder "Zetetic"/"SQLCipher" noch den
OpenSSL-Lizenztext selbst (nur die crate-eigene MIT/Apache-2.0-Lizenz von
`libsqlite3-sys`/`openssl-src`, die mit dem vendorten Code nichts zu tun
hat). Ein generischer tieferer Scan (jede Datei namens `LICENSE*` in jedem
Unterverzeichnis jedes Crates) hätte das zwar auch gefunden, aber ebenso
viele falsche Treffer in Test-Fixtures und Beispielcode anderer Crates
produziert — eine feste, kommentierte Liste für die zwei tatsächlich
relevanten Fälle ist hier die engere, nachvollziehbarere Lösung.

**Konsequenz:** Ein künftiges drittes vendortes Lizenzpaket (falls je
eines hinzukommt) braucht einen weiteren Eintrag in derselben Liste, keine
neue Mechanik.
