# Spec 0099 — Lizenzen: Projektlizenz und Drittlizenzen

Status: umgesetzt
Zweck: Smart SSH steht unter der Apache License 2.0. Jedes Release-Paket
enthält die Lizenztexte aller ausgelieferten Rust- und npm-Abhängigkeiten
und der mitgelieferten Schriften, und die App zeigt sie offline im
Über-Bereich der Einstellungen an.
Bezüge: Spec 0090 (CI und Release, erlaubte Lizenzen A12), ADR 0060
(Lizenzwechsel), ADR 0090 (Weg der Liste ins Paket), ADR 0100 (eine Quelle
für die Kennzeile).

Diese Spec nimmt die frühere Spec 0070 (Lizenzwechsel) auf.

## 1. Projektlizenz

- **L1:** Das gesamte Repository steht unter der Apache License 2.0
  (SPDX `Apache-2.0`). Der Lizenztext liegt unverändert, mit nicht
  ausgefüllten Platzhaltern im Anhang, als `LICENSE` im
  Wurzelverzeichnis.
- **L2:** Jede Stelle, die eine Lizenz für dieses Projekt deklariert
  (Rust-Manifest des Workspace, Frontend-Manifest, README), nennt
  übereinstimmend `Apache-2.0`.
- **L3:** `NOTICE` im Wurzelverzeichnis enthält nur Produktname und
  Urheberrechtsvermerk. Sie ist kein Sammelort für Drittlizenzen.
- Quelldateien tragen keine Lizenz-Kopfzeilen.
- Die Apache License 2.0 gilt auch für alle früher veröffentlichten
  Versionen.

## 2. Liste der Drittlizenzen

Die Liste entsteht beim Bau aus dem tatsächlichen Abhängigkeitsbaum; sie
ist nicht eingecheckt.

- **A1.1:** Ein Skript erzeugt **eine** Textdatei mit den Lizenzen
  (a) aller Rust-Abhängigkeiten des Workspace, (b) aller
  Produktionsabhängigkeiten des Frontends, (c) der Entwicklungs-
  abhängigkeiten, deren Code ins gebaute Frontend gelangt (heute
  `tailwindcss` und `vite`), (d) der Schriften (A6). Je Lizenztext stehen
  die Pakete (Name, Version), die ihn verwenden. Pakete des eigenen
  Workspace und lokale npm-Pakete (Mitglieder eines npm-Workspace, per
  `file:` verlinkte Verzeichnisse) sind keine Drittpakete: Sie stehen nicht
  in der Liste und werden nicht gegen die erlaubten Lizenzen geprüft; ihre
  Produktionsabhängigkeiten stehen darin, jede genau einmal. Ein aus einem
  lokalen Tarball installiertes Paket (`file:…/*.tgz`, `.tar.gz`, `.tar`)
  ist dagegen ein Drittpaket: Es steht in der Liste und wird geprüft wie
  ein Paket aus der Registry. Ein Eintrag ohne Auflösung mit Name und
  Version des Tarball-Pakets gilt nicht als lokal.
- **A1.2:** Hat ein Paket eine Hinweisdatei (`NOTICE*`, Groß-/Kleinschreibung
  egal), steht ihr Inhalt mit Paketname in der Liste.
- **A1.3:** Das Skript löscht zu Beginn eine vorhandene Liste. Es bricht mit
  Rückgabewert ≠ 0 ab und hinterlässt keine Liste, wenn ein Paket (Rust
  **oder** npm) eine nicht erlaubte Lizenz hat, wenn für ein Paket kein
  Lizenztext gefunden wird oder wenn ein Werkzeug fehlt oder in der
  falschen Version vorliegt.
- **A1.4:** Ausgabepfad, Rust-Workspace, Frontend und die Datei mit den
  erlaubten Lizenzen lassen sich als Parameter übergeben; ohne Parameter
  gelten die Pfade dieses Repositorys. Als Frontend darf auch die Wurzel
  eines npm-Workspace übergeben werden, der das Frontend als Mitglied
  enthält; die Pakete werden dort gefunden, wohin npm sie installiert hat
  (an der Wurzel oder verschachtelt, auch unter einem Mitglied).
- **A1.5:** Die erlaubten Lizenzen haben **eine** Quelle: dieselbe Liste,
  die die Dependency-Prüfung der CI benutzt (Spec 0090, A12). Sie gilt
  auch für die npm-Seite.
- **A1.6:** Das Werkzeug für die Rust-Seite (`cargo-about`) hat eine feste
  Version, lokal und in der CI dieselbe.
- **A1.7:** Die erzeugte Liste wird von Git ignoriert.
- **A1.8:** Die Liste beginnt mit einer festen Kennzeile, an der die App
  sie erkennt (A4.3). Die Kennzeile hat eine einzige maßgebliche Quelle;
  App und Release-Prüfung werden gegen sie getestet.
- Das Skript läuft unter Linux, macOS und Windows (auch unter PowerShell).
- **A6 — Schriften:** Neben den mitgelieferten Schriften (Barlow, Barlow
  Condensed, JetBrains Mono) liegt eine Lizenzdatei mit ihren
  Urheberrechtsvermerken und dem vollständigen Text der SIL Open Font
  License 1.1. Ihr Inhalt steht in der Liste. Die Schriftlizenz wird nicht
  gegen die erlaubten Lizenzen geprüft.

## 3. Release und CI

- **A2.1:** Jeder Release-Build (macOS, Windows, Linux) erzeugt die Liste
  vor dem Bau der App.
- **A2.2:** Scheitert die Erzeugung, ist die Liste leer oder fehlt ihr die
  Kennzeile, scheitert der Release-Build dieser Plattform; es entsteht kein
  Paket ohne Lizenzen.
- **A2.3:** Die Liste ist Teil des ausgelieferten App-Pakets und ohne
  Netzwerk lesbar.
- **A2.4:** Frontend und App lassen sich ohne die Liste bauen
  (Entwicklung, Test-Job der CI).
- **A3.1:** Die CI erzeugt die Liste bei jedem Lauf auf allen drei
  Betriebssystemen und wird rot, wenn das scheitert (Spec 0090, A11). Die
  Frontend-Tests prüfen danach die echte Liste.

## 4. Anzeige in der App

- **A4.1:** Der Über-Bereich der Einstellungen hat einen Eintrag
  „Drittanbieter-Lizenzen" (EN „Third-party licenses"), der die Liste in
  einer scrollbaren Ansicht innerhalb der App öffnet.
- **A4.2:** Die Liste wird als reiner Text dargestellt, nie als HTML oder
  Markdown. Markup in einer fremden Lizenzdatei erscheint wörtlich.
- **A4.3:** Ob die Liste vorhanden ist, erkennt die App an der Kennzeile,
  nicht nur an einem Ladefehler. Fehlt sie (Entwicklungs-Build, oder der
  Ladeweg liefert etwas anderes, etwa die Startseite der App), zeigt die
  Ansicht „Drittanbieter-Lizenzen sind nur in Release-Builds enthalten."
  — kein leeres Feld, keinen fremden Inhalt, keinen Absturz.
- **A4.4:** Das Laden der Liste fragt keinen fremden Host an.
- **A4.5:** Alle Texte gibt es auf Deutsch und Englisch.

## 5. Sicherheitszusagen

- **Fremder Inhalt bleibt Text.** Lizenz- und Hinweisdateien stammen aus
  fremden Paketen und erreichen die Oberfläche nur als Text (A4.2), damit
  kein Paket über seine Lizenzdatei Markup oder Skript einschleust.
- **Kein Netzwerk ohne Nutzeraktion:** A4.4; als zweite Linie erlaubt die
  Content-Security-Policy der App keine Verbindung zu fremden Hosts.
- **Lieferkette des Release-Laufs:** Werkzeuge, die der Release-Lauf für
  die Liste installiert, haben eine feste Version (A1.6).

## 6. Grenzen

- Icons und Bilder des Projekts selbst stehen nicht in der Liste.
- Andere Build-Wege, die diese App bauen, müssen das Skript selbst
  aufrufen (A1.4).
- Installer zeigen keine Lizenzseite.
