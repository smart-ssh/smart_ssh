# Spec: Dateibrowser mit erhöhten Rechten + Erfolgsmeldungen

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, `crates/ssh-transport` (SFTP über Exec-Kanal),
`crates/app-shell` (Commands, Session-Zustand), Frontend (Umschalter,
Kennzeichnung, Toasts)
Abhängigkeiten: SFTP-Browser (0020), Dateibrowser-Aktionen (0054, inkl.
„Lokal öffnen"-Flow), Audit-Erfassbarkeit (0054), Toast-Mechanismus (0058),
Server-Profile

> Zwei Teile im selben Code:
> **Teil A — Rechteerhöhung:** Der Dateibrowser kann optional als root (bzw.
> anderer Nutzer) arbeiten, indem der SFTP-Server selbst per `sudo -n`
> gestartet wird (Weg A — wie WinSCP). **Nur passwortloses sudo** für
> `sftp-server` — die App fasst nie ein Sudo-Passwort an.
> **Teil B — Erfolgsmeldungen:** Jede Dateibrowser-Operation meldet ihr
> Ergebnis sichtbar („Datei x heruntergeladen").
>
> **Priorität ERHÖHT** (Teil A: ein Root-Dateibrowser ist das mächtigste
> Werkzeug der App).

## Entscheidung (Stefan)

Nur **Weg A** (SFTP-Server per `sudo -n` starten). **Kein** Weg B
(sudo-Kommandos mit Passwort) — erfordert eine `NOPASSWD`-Regel für
`sftp-server` auf dem Server, dafür volle SFTP-Funktionalität,
Geschwindigkeit und keine Passwort-Handhabung.

---

## Teil A — Rechteerhöhung

### A1. Mechanismus

- Statt des normalen SFTP-Subsystems öffnet die App einen **Exec-Kanal** mit
  `sudo -n <sftp-server-pfad>` und betreibt den SFTP-Client über diesen
  Kanal. Der bestehende SFTP-Client-Code wird wiederverwendet — nur die
  Kanal-Erzeugung unterscheidet sich.
- `-n` (non-interactive): Verlangt sudo ein Passwort, schlägt der Aufruf
  **sofort** fehl statt zu hängen.
- Die erhöhte SFTP-Sitzung ist ein **zweiter, separater** Kanal neben dem
  normalen — der normale Browser bleibt unverändert.

### A2. Pfad von `sftp-server` ermitteln

- Der Pfad unterscheidet sich je Distribution (`/usr/lib/openssh/sftp-server`,
  `/usr/libexec/openssh/sftp-server`, `/usr/lib/ssh/sftp-server`,
  `/usr/libexec/sftp-server`, …).
- **Automatische Erkennung** per kurzem Probe-Kommando über einen Exec-Kanal
  (bekannte Pfade auf `-x` prüfen). Zusätzlich
  `sshd`-Konfiguration als Hinweisquelle, falls einfach.
- **Override pro Server-Profil** (Experten-Feld, eingeklappt, Default
  „automatisch"), falls die Erkennung scheitert oder ein Nicht-Standardpfad
  genutzt wird.

### A3. Voraussetzungs-Prüfung und verständliche Fehler

Vor dem Umschalten prüfen (Probe-Kommando `sudo -n -l <pfad>`), ob
passwortloses sudo für genau diesen Befehl erlaubt ist. Scheitert es,
**verständliche Meldung mit konkreter Anleitung** statt kryptischem Fehler:
- „Für den erhöhten Modus braucht der Server eine sudo-Regel ohne Passwort für
  sftp-server." + die **konkrete sudoers-Zeile** zum Kopieren, auf den
  ermittelten Pfad und den Login-Nutzer zugeschnitten, z. B.
  `stefan ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server`.
- **Ehrlicher Hinweis bei der Anleitung:** Diese Regel gibt dem SSH-Login
  **passwortlosen Root-Dateizugriff** — wer den SSH-Schlüssel hat, kann damit
  jede Datei lesen und schreiben. Das bewusst entscheiden.
- Weitere Fehlerfälle erkennen und benennen: `requiretty` in sudoers (sudo
  verweigert ohne Terminal), `sftp-server` nicht gefunden, sudo nicht
  installiert.

### A4. Anderer Nutzer (optional, gleicher Mechanismus)

`sudo -n -u <nutzer> <sftp-server>` — Ziel-Nutzer als optionales Feld im
Umschalter (Default: root). Nur umsetzen, wenn es mit dem Mechanismus
trivial mitkommt; sonst dokumentieren und weglassen.

### A5. Sicherheits-Leitplanken (nicht verhandelbar)

- **Expliziter Umschalter** pro Dateibrowser-Ansicht („Mit erhöhten
  Rechten"), **nie** standardmäßig an, **nicht persistiert** — nach
  Verbindungstrennung bzw. App-Neustart wieder aus.
- **Unübersehbare Kennzeichnung**, solange aktiv: farbiger Rahmen um den
  Browser + Banner „Erhöhte Rechten: root" (bzw. Zielnutzer). Man darf nie
  vergessen, in welchem Modus man ist.
- **Bestätigungen bleiben** (Löschen, Überschreiben, rekursives chmod) und
  nennen den Modus („…als root löschen?").
- **Nur manuelle Nutzung**: KI und MCP-Agenten bekommen über diesen Kanal
  **keinen** Zugriff. Deren Datei- und Kommando-Aktionen laufen unverändert
  durch Filter-Engine + Confirm, auch während der erhöhte Browser offen ist.
  Der erhöhte Kanal ist ausschließlich von den Browser-Commands erreichbar.
- **Audit-erfassbar** mit Kennzeichnung „erhöht" + Zielnutzer (baut auf der
  0054-Struktur auf).
- **„Lokal öffnen"-Flow** (0054): Wird eine Datei im erhöhten Modus geöffnet,
  muss der spätere Upload **über denselben erhöhten Kanal** laufen (sonst
  scheitert er an Rechten). Die Edit-Session merkt sich den Modus. Ist der
  erhöhte Kanal inzwischen geschlossen → verständliche Meldung, kein stiller
  Upload als normaler Nutzer. Temp-Dateien bleiben 0600 (Root-Dateien können
  sensibel sein).
- Kein Passwort im Spiel → nichts zu redigieren; trotzdem: stderr von sudo
  nicht ungefiltert in den Chat/Kontext.

---

## Teil B — Erfolgsmeldungen für alle Dateibrowser-Operationen

### B1. Welche Operationen

Jede Aktion aus 0054 meldet ihr Ergebnis:
- Herunterladen (Datei: „`datei.conf` heruntergeladen nach `~/Downloads`";
  Ordner: „Ordner `x` heruntergeladen — 42 Dateien")
- Hochladen, Löschen, Umbenennen, Verschieben, Ordner anlegen, chmod
  (inkl. rekursiv mit Anzahl)
- Pfad kopieren / Inhalt kopieren („In die Zwischenablage kopiert")
- Lokal öffnen / Änderung hochgeladen (Edit-Flow)

### B2. Form

- **Toast** über den bestehenden app-weiten Mechanismus (0058), kein neues
  UI-Element.
- **Erfolg**: kurz, verschwindet automatisch nach einigen Sekunden.
- **Fehler**: bleibt stehen, bis der Nutzer ihn schließt, mit verständlichem
  Grund (Rechte, Ziel existiert, Verbindung weg, …).
- **Download**: Aktion „Im Finder/Explorer zeigen" im Toast (nutzt den
  bestehenden Opener).
- **Im erhöhten Modus** trägt die Meldung den Zusatz „(als root)".
- **Sammeloperationen** (Ordner, rekursiv, mehrere Dateien): **eine**
  Zusammenfassung statt eines Toasts pro Datei — sonst Toast-Flut.
- **Lang laufende Operationen**: bestehende Fortschrittsanzeige bleibt; die
  Erfolgsmeldung kommt am Ende.
- **DE + EN** über die Locale-Dateien.

### B3. Inhalt

Dateinamen und Zielpfade sind in Ordnung; **keine Dateiinhalte** in den
Meldungen.

---

## Invarianten / Sicherheit
- Erhöhter Modus nie Default, nie persistiert, immer sichtbar gekennzeichnet.
- KI/MCP erreichen den erhöhten Kanal nie.
- Kein Sudo-Passwort wird verarbeitet (`sudo -n`), fehlende Berechtigung
  scheitert sofort und verständlich, nie Hängen.
- Edit-Flow lädt eine erhöht geöffnete Datei nur erhöht hoch, nie still als
  normaler Nutzer.
- Bestätigungen bleiben und nennen den Modus.

## Testbarkeit
- Kanal-Erzeugung: erhöhter Modus nutzt Exec mit `sudo -n <pfad>`, normaler
  Modus unverändert das Subsystem.
- Pfad-Erkennung + Override.
- Fehlerfälle (sudo verlangt Passwort, requiretty, sftp-server fehlt) →
  jeweils verständliche Meldung, sudoers-Zeile korrekt zugeschnitten.
- **KI-/MCP-Pfad kann den erhöhten Kanal nicht nutzen** (Regressionstest).
- Umschalter nach Trennung/Neustart wieder aus.
- Edit-Flow: erhöht geöffnet → Upload nur erhöht; Kanal weg → Meldung.
- Toasts: je Operation Erfolg/Fehler, Sammel-Toast bei Ordnern, Zusatz „(als
  root)" im erhöhten Modus.

## Reihenfolge
1. Teil B (Erfolgsmeldungen) — unabhängig, sofort nützlich, klein.
2. A1–A3 (Mechanismus, Pfad, Voraussetzungs-Prüfung).
3. A5 (Leitplanken, Kennzeichnung, Edit-Flow-Kopplung).
4. A4 (anderer Nutzer), falls trivial.

## Abschluss
- `spec-reviewer` ERHÖHT, adversarial für Teil A: Kann die KI oder ein
  MCP-Agent auf irgendeinem Weg den erhöhten Kanal erreichen? Kann der
  erhöhte Modus unbemerkt aktiv bleiben?
- CHANGELOG.
- Manuelle Testabläufe für Stefan (Teil A braucht einen Server mit
  eingerichteter sudoers-Regel).
