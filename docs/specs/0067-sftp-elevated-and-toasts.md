# Spec 0067 — Dateibrowser mit erhöhten Rechten und Erfolgsmeldungen

Status: umgesetzt
Zweck: Teil A: Der Dateibrowser kann auf Wunsch als root (oder anderer Nutzer) arbeiten, ohne dass die App je ein Sudo-Passwort anfasst. Teil B: Jede Dateibrowser-Aktion meldet ihr Ergebnis sichtbar.
Bezüge: Spec 0020 (Dateibrowser), Spec 0054 (Aktionen, „Lokal öffnen“), Spec 0058 (Meldungen), Spec 0084 §9 und Spec 0085 (Widerruf), Spec 0102 (Startverzeichnis gilt nicht im erhöhten Modus), Spec 0003 (Server-Profil).

Ein Root-Dateibrowser ist das mächtigste Werkzeug der App; die Regeln in
Teil A sind deshalb Leitplanken, keine Vorschläge.

## Entscheidung

Der erhöhte Modus startet den SFTP-Server des Servers selbst mit
`sudo -n <sftp-server-pfad>` (wie WinSCP). Er braucht eine
`NOPASSWD`-Regel für `sftp-server` und verarbeitet **nie** ein
Sudo-Passwort. Ein Weg über Sudo-Kommandos mit Passwort gibt es nicht.

---

## Teil A — Rechteerhöhung

### A1. Mechanismus

- Statt des normalen SFTP-Subsystems öffnet die App einen eigenen Kanal mit
  `sudo -n <sftp-server-pfad>` und betreibt darüber denselben SFTP-Client.
- `-n` (nicht interaktiv): Verlangt sudo ein Passwort, schlägt der Aufruf
  sofort fehl, statt zu hängen.
- Die erhöhte Sitzung ist ein **zweiter, getrennter** Kanal neben dem
  normalen; der normale Browser bleibt unverändert. Für den lokalen
  Pseudo-Server (Spec 0032) gibt es keinen erhöhten Modus.

### A2. Pfad von `sftp-server`

- Der Pfad unterscheidet sich je Distribution (`/usr/lib/openssh/sftp-server`,
  `/usr/libexec/openssh/sftp-server`, `/usr/lib/ssh/sftp-server`,
  `/usr/libexec/sftp-server`, …). Die App erkennt ihn automatisch mit einem
  kurzen Probe-Kommando auf dem Server (bekannte Pfade auf Ausführbarkeit
  prüfen).
- **Override je Server-Profil** (Experten-Feld, eingeklappt, Standard
  „automatisch“), falls die Erkennung scheitert oder der Pfad nicht
  üblich ist. Ein Override muss ein einfacher absoluter Pfad sein, dessen
  Dateiname `sftp-server` ist; sonst wird er abgelehnt, damit über das Feld
  kein beliebiges Programm mit sudo gestartet und dafür eine Regel
  vorgeschlagen werden kann.

### A3. Voraussetzungsprüfung und verständliche Fehler

Vor dem Umschalten prüft die App (Probe `sudo -n -l <pfad>`), ob passwortloses
sudo für genau diesen Befehl erlaubt ist. Scheitert das, erscheint eine
verständliche Meldung mit konkreter Anleitung statt eines kryptischen Fehlers:

- „Für den erhöhten Modus braucht der Server eine sudo-Regel ohne Passwort
  für sftp-server.“ mit der **konkreten sudoers-Zeile** zum Kopieren, auf den
  ermittelten Pfad und den Login-Nutzer zugeschnitten, z. B.
  `stefan ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server`.
- Ein **ehrlicher Hinweis**: Diese Regel gibt dem SSH-Login
  passwortlosen Root-Dateizugriff. Wer den SSH-Schlüssel hat, kann jede Datei
  lesen und schreiben. Das ist bewusst zu entscheiden.
- Weitere Fälle werden erkannt und benannt: `requiretty` in sudoers, ein
  nicht gefundener `sftp-server`, nicht installiertes sudo, ein ungültiger
  Zielnutzer oder Pfad, ein fehlgeschlagener Start.

**Eigentümer und Rechte von `sftp-server`.** Bevor die App eine sudoers-Zeile
vorschlägt (und vor dem sudo-Check), prüft sie lesend auf dem Server, dass
`sftp-server` — nach Auflösen von Symlinks — und **jedes übergeordnete
Verzeichnis bis `/`** root gehören und weder für die Gruppe noch für andere
beschreibbar sind. Die Prüfung läuft nie mit sudo und nutzt nur
POSIX-Werkzeuge. Ist ein Eintrag unsicher, oder scheitert die Prüfung oder
liefert nichts Verwertbares (im Zweifel unsicher):

- **sudo verlangt ein Passwort oder erlaubt den Befehl nicht:** Der erhöhte
  Modus bleibt aus, es wird **keine sudoers-Zeile** gezeigt. Die Meldung nennt
  den auffälligen Pfad und sagt, dass er root gehören und für Gruppe und
  andere nicht beschreibbar sein muss; auf so einem Server könnte jeder, der
  das Binary ersetzen kann, mit der Regel root werden.
- **sudo erlaubt den Befehl bereits:** Der erhöhte Modus startet wie bisher,
  im Dateibrowser steht aber eine Warnung mit demselben Text.

Die Prüfung verschärft nur: Sie kann die Zeile unterdrücken oder eine Warnung
hinzufügen, nie den erhöhten Modus dort freigeben, wo er vorher abgelehnt
wurde. Ist alles sicher, ändert sich nichts.

### A4. Anderer Nutzer

`sudo -n -u <nutzer> <sftp-server>`: Der Zielnutzer ist ein optionales Feld
im Umschalter (Standard `root`). Gültig sind übliche POSIX-Nutzernamen
(Kleinbuchstaben, Ziffern, `_`, `-`; höchstens 32 Zeichen, Beginn mit
Buchstabe oder `_`). Die sudoers-Zeile aus A3 nennt dann diesen Zielnutzer.

### A5. Sicherheits-Leitplanken (nicht verhandelbar)

- **Expliziter Umschalter** je Dateibrowser-Ansicht („Mit erhöhten
  Rechten“), **nie** standardmäßig an und **nicht gespeichert**: Nach
  Verbindungstrennung oder App-Neustart ist er wieder aus.
- **Unübersehbare Kennzeichnung**, solange aktiv: farbiger Rahmen um den
  Browser und ein Banner „Erhöhte Rechte: root“ (bzw. Zielnutzer). Die
  Kennzeichnung bleibt auch bei verborgener Dateien-Ansicht sichtbar.
- **Bestätigungen bleiben** (Löschen, Überschreiben, rekursives chmod) und
  nennen den Modus („…als root löschen?“).
- **Nur manuelle Nutzung:** KI und MCP-Agenten bekommen über diesen Kanal
  **keinen** Zugriff. Deren Datei- und Kommando-Aktionen laufen unverändert
  durch Filter-Engine und Bestätigung, auch während der erhöhte Browser offen
  ist. Der erhöhte Kanal ist ausschließlich von den Browser-Befehlen
  erreichbar.
- **Protokolliert** mit Kennzeichnung „erhöht“ und Zielnutzer (Spec 0054,
  Sicherheitsmodell).
- **Widerruf wirkt sofort:** Wird der Modus ausgeschaltet, die Sitzung
  getrennt oder für einen anderen Nutzer neu aktiviert, wird der Kanal
  verworfen und kein weiterer Befehl läuft mit den alten Rechten
  (Spec 0085).
- **„Lokal öffnen“** (Spec 0054): Wird eine Datei im erhöhten Modus
  geöffnet, läuft der spätere Upload **über denselben erhöhten Kanal**; die
  Bearbeitung merkt sich den Modus. Ist der Kanal inzwischen geschlossen,
  erscheint eine verständliche Meldung, nie ein stiller Upload als normaler
  Nutzer. Die Bearbeitungskopie ist nur für den Nutzer lesbar (0600), weil
  Root-Dateien sensibel sein können.
- Es ist kein Passwort im Spiel, also nichts zu redigieren. Die
  Fehlerausgabe von sudo gelangt trotzdem nie ungefiltert in Chat oder
  KI-Kontext.

---

## Teil B — Erfolgsmeldungen

### B1. Welche Aktionen

Jede Aktion aus Spec 0054 meldet ihr Ergebnis:

- Herunterladen (Datei: „`datei.conf` heruntergeladen nach `~/Downloads`“;
  Ordner: „Ordner `x` heruntergeladen — 42 Dateien“).
- Hochladen, Löschen, Umbenennen, Verschieben, Ordner anlegen, chmod (rekursiv
  mit Anzahl).
- Pfad kopieren, Inhalt kopieren („In die Zwischenablage kopiert“).
- Lokal öffnen und „Änderung hochgeladen“.

### B2. Form

- Die Meldung erscheint als Toast des app-weiten Mechanismus (Spec 0058).
- **Erfolg:** kurz, verschwindet nach einigen Sekunden.
- **Fehler:** bleibt stehen, bis der Nutzer ihn schließt, mit verständlichem
  Grund (Rechte, Ziel existiert, Verbindung weg).
- **Download:** Der Toast bietet „Im Finder/Explorer zeigen“ (Beschriftung je
  Betriebssystem).
- **Im erhöhten Modus** trägt die Meldung den Zusatz „(als root)“ (bzw.
  Zielnutzer).
- **Sammeloperationen** (Ordner, rekursiv, mehrere Dateien) melden **eine**
  Zusammenfassung statt eines Toasts je Datei.
- **Lang laufende Aktionen** zeigen weiter ihren Fortschritt; die
  Erfolgsmeldung kommt am Ende.
- Alle Texte gibt es auf Deutsch und Englisch.

### B3. Inhalt

Dateinamen und Zielpfade sind in Meldungen in Ordnung; **Dateiinhalt**
erscheint nie.

---

## Invarianten

- Der erhöhte Modus ist nie Standard, nie gespeichert und immer sichtbar
  gekennzeichnet.
- KI und MCP erreichen den erhöhten Kanal nie.
- Kein Sudo-Passwort wird verarbeitet (`sudo -n`); fehlende Berechtigung
  scheitert sofort und verständlich, nie durch Hängen.
- „Lokal öffnen“ lädt eine im erhöhten Modus geöffnete Datei nur erhöht
  hoch, nie still als normaler Nutzer.
- Bestätigungen bleiben und nennen den Modus.

## Grenzen

- Der erhöhte Modus braucht auf dem Server eine passende sudoers-Regel; die
  App legt sie nie selbst an.
- Das konfigurierte Startverzeichnis (Spec 0102) gilt für den Login-Nutzer,
  nicht für den erhöhten Modus.
